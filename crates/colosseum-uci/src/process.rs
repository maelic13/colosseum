//! An async handle to a running UCI engine process.
//!
//! Responsibilities: spawn (with args/workdir/env), the `uci`/`isready` handshake,
//! `setoption`/`ucinewgame`, running a search to `bestmove` under a deadline, and
//! clean shutdown. Each child is owned by a kill-on-close Windows Job Object or
//! dedicated Unix process group, and `kill_on_drop(true)` is retained as a second
//! guard for the direct child.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use colosseum_core::UciOption;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use crate::error::UciError;
use crate::parse;
use crate::position::{GoLimits, UciPosition};
use crate::score::Score;

/// How to launch an engine.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}

impl SpawnOptions {
    /// Convenience constructor from just an executable path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }
}

/// Identity and capabilities discovered during the handshake.
#[derive(Debug, Clone, Default)]
pub struct HandshakeInfo {
    pub name: Option<String>,
    pub author: Option<String>,
    pub options: Vec<UciOption>,
}

/// The outcome of one search.
#[derive(Debug, Clone)]
pub struct SearchOutput {
    /// Best move in UCI long algebraic (e.g. `e2e4`), or `(none)`/`0000`.
    pub best_move: String,
    /// Last reported score (side-to-move perspective), if any.
    pub score: Option<Score>,
    /// Last reported nps, if any.
    pub nps: Option<u64>,
    /// Last reported depth, if any.
    pub depth: Option<u32>,
    /// The engine's predicted reply (`bestmove … ponder <move>`), if any.
    pub ponder: Option<String>,
    /// Wall-clock time actually spent on this search.
    pub elapsed: Duration,
}

/// How many recent protocol lines are kept for incident forensics.
const TRANSCRIPT_CAP: usize = 120;
/// How many recent stderr lines are kept (crash/assert messages).
const STDERR_CAP: usize = 40;
/// Maximum accepted UCI protocol line, excluding the newline.
pub const MAX_PROTOCOL_LINE_BYTES: usize = 64 * 1024;
/// Maximum bytes retained for one stderr line.
pub const MAX_STDERR_LINE_BYTES: usize = 16 * 1024;

/// Return whether a PID still denotes a running process. This is intentionally
/// narrow and exists so the executable self-test can verify reaping.
#[must_use]
pub fn process_is_alive(pid: u32) -> bool {
    process_alive_platform(pid)
}

/// A running UCI engine.
pub struct EngineProcess {
    containment: ProcessContainment,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    info: HandshakeInfo,
    /// Recent protocol traffic ("> sent" / "< received"), for incident
    /// reports. Consecutive `info` lines are collapsed to the latest so the
    /// buffer isn't all search spam.
    transcript: VecDeque<String>,
    /// Recent stderr output, collected by a background task — engines print
    /// their panic/assert messages there.
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    /// A `bestmove` that arrived while pondering (an engine bailing out of
    /// `go ponder` early); consumed by `ponderhit`/`stop_ponder`.
    ponder_early: Option<(String, Option<String>)>,
}

impl EngineProcess {
    /// Spawn the engine process and wire up its stdio pipes.
    pub async fn spawn(options: SpawnOptions) -> Result<Self, UciError> {
        // Build via std::process::Command so we can set Windows creation flags, then
        // convert to tokio's Command to apply kill_on_drop.
        let mut std_cmd = std::process::Command::new(&options.path);
        std_cmd
            .args(&options.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &options.working_dir {
            std_cmd.current_dir(dir);
        }
        for (key, value) in &options.env {
            std_cmd.env(key, value);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            std_cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: don't pop up a console for each engine.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const CREATE_SUSPENDED: u32 = 0x0000_0004;
            std_cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        }

        let mut command = Command::from(std_cmd);
        command.kill_on_drop(true);

        let mut child = command.spawn()?;
        let containment = ProcessContainment::attach(&child)?;
        #[cfg(windows)]
        containment.resume(&child)?;
        let stdin = child.stdin.take().ok_or(UciError::Terminated)?;
        let stdout = child.stdout.take().ok_or(UciError::Terminated)?;
        let stdout = BufReader::new(stdout);

        // Drain stderr in the background into a small tail buffer — engines
        // print crash/assert messages there, which is exactly what an
        // incident report needs.
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&stderr_tail);
            tokio::spawn(async move {
                drain_stderr(stderr, tail).await;
            });
        }

        Ok(Self {
            containment,
            child,
            stdin,
            stdout,
            info: HandshakeInfo::default(),
            transcript: VecDeque::new(),
            stderr_tail,
            ponder_early: None,
        })
    }

    /// Recent protocol traffic (oldest first) for incident reports.
    #[must_use]
    pub fn transcript(&self) -> Vec<String> {
        self.transcript.iter().cloned().collect()
    }

    /// Recent stderr output (oldest first) for incident reports.
    #[must_use]
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .map(|t| t.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Record one protocol line into the transcript ring buffer.
    fn record(&mut self, prefix: &str, line: &str) {
        // Collapse runs of `info` spam: keep only the latest.
        if prefix == "<"
            && line.starts_with("info ")
            && self
                .transcript
                .back()
                .is_some_and(|last| last.starts_with("< info "))
        {
            self.transcript.pop_back();
        }
        if self.transcript.len() >= TRANSCRIPT_CAP {
            self.transcript.pop_front();
        }
        self.transcript.push_back(format!("{prefix} {line}"));
    }

    /// The engine's reported name, once handshaken.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.info.name.as_deref()
    }

    /// The engine's reported author, once handshaken.
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.info.author.as_deref()
    }

    /// The option schema detected during the handshake.
    #[must_use]
    pub fn options(&self) -> &[UciOption] {
        &self.info.options
    }

    /// Perform the `uci` handshake, collecting id/options until `uciok`.
    pub async fn handshake(&mut self, deadline: Duration) -> Result<(), UciError> {
        self.send("uci").await?;
        let until = Instant::now() + deadline;
        let mut info = HandshakeInfo::default();
        loop {
            let line = self
                .read_line_until(until, UciError::HandshakeTimeout)
                .await?;
            let line = line.trim();
            if line == "uciok" {
                break;
            } else if let Some(rest) = line.strip_prefix("id name ") {
                info.name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("id author ") {
                info.author = Some(rest.trim().to_string());
            } else if line.starts_with("option ")
                && let Some(option) = parse::parse_option_line(line)
            {
                info.options.push(option);
            }
        }
        self.info = info;
        Ok(())
    }

    /// Send `isready` and wait for `readyok`.
    pub async fn is_ready(&mut self, deadline: Duration) -> Result<(), UciError> {
        self.send("isready").await?;
        let until = Instant::now() + deadline;
        loop {
            let line = self
                .read_line_until(until, UciError::HandshakeTimeout)
                .await?;
            if line.trim() == "readyok" {
                return Ok(());
            }
        }
    }

    /// Set a UCI option (`value` is omitted for `button` options).
    pub async fn set_option(&mut self, name: &str, value: Option<&str>) -> Result<(), UciError> {
        let command = match value {
            Some(value) => format!("setoption name {name} value {value}"),
            None => format!("setoption name {name}"),
        };
        self.send(&command).await
    }

    /// Tell the engine a new game is starting.
    pub async fn new_game(&mut self) -> Result<(), UciError> {
        self.send("ucinewgame").await
    }

    /// Run a search: set the position, issue `go`, and read until `bestmove`, tracking
    /// the last score/nps/depth. Every parsed `info` line is also handed to
    /// `on_info` mid-search (live view); pass `|_| {}` when not observing.
    /// Fails with [`UciError::MoveTimeout`] if `deadline` elapses first, or
    /// [`UciError::Terminated`] if the engine exits mid-search.
    pub async fn search(
        &mut self,
        position: &UciPosition,
        limits: &GoLimits,
        deadline: Duration,
        on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        self.start_search(position, limits).await?;
        self.await_bestmove(Instant::now(), deadline, on_info).await
    }

    /// Start a normal search and return immediately, leaving `bestmove` to be
    /// collected by [`Self::stop_search`].
    pub async fn start_search(
        &mut self,
        position: &UciPosition,
        limits: &GoLimits,
    ) -> Result<(), UciError> {
        self.send(&position.to_command()).await?;
        self.send(&limits.to_command()).await
    }

    /// Issue `stop` for an active normal search and require a bounded
    /// `bestmove` response.
    pub async fn stop_search(
        &mut self,
        deadline: Duration,
        on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        self.send("stop").await?;
        self.await_bestmove(Instant::now(), deadline, on_info).await
    }

    /// Start pondering: set the position (played move + predicted reply
    /// included) and issue `go ponder …`. Returns immediately; the search
    /// output stream should then be pumped with [`Self::drain_ponder`] and
    /// resolved with [`Self::ponderhit`] or [`Self::stop_ponder`].
    pub async fn start_ponder(
        &mut self,
        position: &UciPosition,
        limits: &GoLimits,
    ) -> Result<(), UciError> {
        self.send(&position.to_command()).await?;
        let go = limits.to_command();
        let go_ponder = format!("go ponder{}", go.strip_prefix("go").unwrap_or_default());
        self.send(&go_ponder).await?;
        self.ponder_early = None;
        Ok(())
    }

    /// Pump the engine's output while it ponders, feeding `info` lines to
    /// `on_info`. Never completes normally — it is meant to be raced
    /// (`tokio::select!`) against the opponent's search and dropped. If the
    /// engine sends a premature `bestmove` (some engines bail out of ponder),
    /// it is stashed for [`Self::ponderhit`]/[`Self::stop_ponder`] and the
    /// future then parks forever.
    pub async fn drain_ponder(&mut self, mut on_info: impl FnMut(&parse::InfoLine)) {
        loop {
            // Effectively no deadline: pondering lasts as long as the
            // opponent thinks.
            let far = Instant::now() + Duration::from_secs(24 * 3600);
            match self.read_line_until(far, UciError::MoveTimeout).await {
                Ok(line) => {
                    let line = line.trim();
                    if let Some(best) = parse::parse_bestmove_ponder(line) {
                        self.ponder_early = Some(best);
                        break;
                    }
                    if line.starts_with("info ")
                        && let Some(info) = parse::parse_info_line(line)
                    {
                        on_info(&info);
                    }
                }
                Err(_) => break, // terminated/IO: surface via the next command
            }
        }
        std::future::pending::<()>().await;
    }

    /// The predicted move was played: convert the ponder search into the real
    /// one (`ponderhit`) and await its result. `deadline` covers from now —
    /// the engine has been thinking for free until this moment.
    pub async fn ponderhit(
        &mut self,
        deadline: Duration,
        on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        if let Some((best_move, ponder)) = self.ponder_early.take() {
            // The engine already finished during ponder; its move is free.
            return Ok(SearchOutput {
                best_move,
                score: None,
                nps: None,
                depth: None,
                ponder,
                elapsed: Duration::ZERO,
            });
        }
        self.send("ponderhit").await?;
        self.await_bestmove(Instant::now(), deadline, on_info).await
    }

    /// The prediction missed: abort the ponder search and discard its result.
    pub async fn stop_ponder(&mut self, deadline: Duration) -> Result<(), UciError> {
        if self.ponder_early.take().is_some() {
            return Ok(());
        }
        self.send("stop").await?;
        let until = Instant::now() + deadline;
        loop {
            let line = self.read_line_until(until, UciError::MoveTimeout).await?;
            if parse::parse_bestmove(line.trim()).is_some() {
                return Ok(());
            }
        }
    }

    /// Read engine output until `bestmove`, tracking the last
    /// score/nps/depth and feeding `info` lines to `on_info`.
    async fn await_bestmove(
        &mut self,
        start: Instant,
        deadline: Duration,
        mut on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        let until = start + deadline;
        let mut score = None;
        let mut nps = None;
        let mut nodes = None;
        let mut depth = None;

        loop {
            let line = self.read_line_until(until, UciError::MoveTimeout).await?;
            let line = line.trim();
            if let Some((best_move, ponder)) = parse::parse_bestmove_ponder(line) {
                let elapsed = start.elapsed();
                // Some engines report a literal `nps 0` on every info line
                // (Fruit 2.1 does) — treat that as unreported and derive the
                // real speed from nodes over wall-clock time instead.
                let nps = nps.or_else(|| {
                    nodes.map(|n: u64| (n as f64 / elapsed.as_secs_f64().max(0.001)).round() as u64)
                });
                return Ok(SearchOutput {
                    best_move,
                    score,
                    nps,
                    depth,
                    ponder,
                    elapsed,
                });
            }
            if line.starts_with("info ")
                && let Some(info) = parse::parse_info_line(line)
            {
                if info.score.is_some() {
                    score = info.score;
                }
                if let Some(n) = info.nps
                    && n > 0
                {
                    nps = Some(n);
                }
                if info.nodes.is_some() {
                    nodes = info.nodes;
                }
                if info.depth.is_some() {
                    depth = info.depth;
                }
                on_info(&info);
            }
        }
    }

    /// Gracefully request shutdown (`quit`), waiting up to `deadline` for exit before
    /// killing. Consumes the handle.
    pub async fn quit(mut self, deadline: Duration) -> Result<(), UciError> {
        let _ = self.send("quit").await;
        match timeout(deadline, self.child.wait()).await {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(err)) => Err(UciError::Io(err)),
            Err(_elapsed) => {
                self.containment.terminate();
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
                Err(UciError::ShutdownTimeout)
            }
        }
    }

    /// Immediately kill the engine (for Force-Stop). Waits for the process to be reaped.
    pub async fn kill(&mut self) -> Result<(), UciError> {
        self.containment.terminate();
        let _ = self.child.start_kill();
        self.child.wait().await.map(|_| ()).map_err(UciError::Io)
    }

    /// OS process identifier, used by containment acceptance tests.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Write a command line to the engine and flush.
    async fn send(&mut self, line: &str) -> Result<(), UciError> {
        tracing::trace!(target: "uci", direction = "send", "{line}");
        self.record(">", line);
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read the next line, enforcing an absolute deadline. Maps a timeout to
    /// `timeout_err`, EOF to [`UciError::Terminated`].
    async fn read_line_until(
        &mut self,
        until: Instant,
        timeout_err: UciError,
    ) -> Result<String, UciError> {
        // On the `None` path we diverge by returning, so `timeout_err` is only ever
        // moved once and needs no `Clone`.
        let Some(remaining) = until.checked_duration_since(Instant::now()) else {
            return Err(timeout_err);
        };
        match timeout(remaining, read_bounded_line(&mut self.stdout)).await {
            Err(_elapsed) => Err(timeout_err),
            Ok(Ok(Some(line))) => {
                tracing::trace!(target: "uci", direction = "recv", "{line}");
                self.record("<", &line);
                Ok(line)
            }
            Ok(Ok(None)) => Err(UciError::Terminated),
            Ok(Err(err)) => Err(err),
        }
    }
}

async fn read_bounded_line(
    reader: &mut BufReader<ChildStdout>,
) -> Result<Option<String>, UciError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content = if newline.is_some() { take - 1 } else { take };
        if bytes.len().saturating_add(content) > MAX_PROTOCOL_LINE_BYTES {
            return Err(UciError::Protocol(format!(
                "protocol line exceeds {MAX_PROTOCOL_LINE_BYTES} bytes"
            )));
        }
        bytes.extend_from_slice(&available[..content]);
        reader.consume(take);
        if newline.is_some() {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut buffer = [0_u8; 4096];
    let mut line = Vec::new();
    let mut truncated = false;
    while let Ok(count) = stderr.read(&mut buffer).await {
        if count == 0 {
            if !line.is_empty() || truncated {
                push_stderr(&tail, &line, truncated);
            }
            break;
        }
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                push_stderr(&tail, &line, truncated);
                line.clear();
                truncated = false;
            } else if line.len() < MAX_STDERR_LINE_BYTES {
                line.push(*byte);
            } else {
                truncated = true;
            }
        }
    }
}

fn push_stderr(tail: &Arc<Mutex<VecDeque<String>>>, line: &[u8], truncated: bool) {
    if let Ok(mut tail) = tail.lock() {
        if tail.len() >= STDERR_CAP {
            tail.pop_front();
        }
        let mut text = String::from_utf8_lossy(line).into_owned();
        if truncated {
            text.push_str("…[truncated]");
        }
        tail.push_back(text);
    }
}

#[cfg(unix)]
struct ProcessContainment {
    process_group: i32,
}

#[cfg(unix)]
impl ProcessContainment {
    fn attach(child: &Child) -> Result<Self, UciError> {
        Ok(Self {
            process_group: child.id().ok_or(UciError::Terminated)? as i32,
        })
    }

    fn terminate(&self) {
        // SAFETY: a negative PID targets the process group created for this engine.
        unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
    }
}

#[cfg(unix)]
impl Drop for ProcessContainment {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(unix)]
fn process_alive_platform(pid: u32) -> bool {
    // SAFETY: signal zero performs existence/permission checking only.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
struct ProcessContainment {
    job: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the owned job HANDLE has no thread affinity and access is immutable;
// CloseHandle is performed exactly once by Drop.
#[cfg(windows)]
unsafe impl Send for ProcessContainment {}

#[cfg(windows)]
impl ProcessContainment {
    fn attach(child: &Child) -> Result<Self, UciError> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        // SAFETY: handles are checked and closed on every branch.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(UciError::Io(std::io::Error::last_os_error()));
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                windows_sys::Win32::Foundation::CloseHandle(job);
                return Err(UciError::Io(std::io::Error::last_os_error()));
            }
            let process = OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_TERMINATE,
                0,
                child.id().ok_or(UciError::Terminated)?,
            );
            if process.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(job);
                return Err(UciError::Io(std::io::Error::last_os_error()));
            }
            let assigned = AssignProcessToJobObject(job, process);
            windows_sys::Win32::Foundation::CloseHandle(process);
            if assigned == 0 {
                windows_sys::Win32::Foundation::CloseHandle(job);
                return Err(UciError::Io(std::io::Error::last_os_error()));
            }
            Ok(Self { job })
        }
    }

    fn resume(&self, child: &Child) -> Result<(), UciError> {
        // Tokio exposes only the process ID, so enumerate-free resumption uses
        // NtResumeProcess through ntdll below.
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtResumeProcess(process: windows_sys::Win32::Foundation::HANDLE) -> i32;
        }
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};
        // SAFETY: process handle is checked, used for one OS operation and closed.
        unsafe {
            let process = OpenProcess(
                PROCESS_SUSPEND_RESUME,
                0,
                child.id().ok_or(UciError::Terminated)?,
            );
            if process.is_null() {
                return Err(UciError::Io(std::io::Error::last_os_error()));
            }
            let status = NtResumeProcess(process);
            windows_sys::Win32::Foundation::CloseHandle(process);
            if status < 0 {
                return Err(UciError::Io(std::io::Error::from_raw_os_error(status)));
            }
            Ok(())
        }
    }

    fn terminate(&self) {
        // SAFETY: job is owned by this object and remains valid until Drop.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessContainment {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE terminates every descendant still in the job.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job) };
    }
}

#[cfg(windows)]
fn process_alive_platform(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    // SAFETY: handle is checked and closed exactly once.
    unsafe {
        let process = OpenProcess(SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        let result = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        CloseHandle(process);
        result
    }
}
