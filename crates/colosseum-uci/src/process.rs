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

use crate::cpu_time::ProcessSample;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::error::UciError;
use crate::parse;
use crate::position::{GoLimits, UciPosition};
use crate::score::Score;
use crate::timing::SearchTiming;

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
    /// Last node count reported by the engine, if any.
    pub reported_nodes: Option<u64>,
    /// Last search time reported by the engine, if any.
    pub reported_time_ms: Option<u64>,
    /// Last NPS value reported by the engine, excluding harness-derived fallback.
    pub reported_nps: Option<u64>,
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

/// Install a process-lifetime guard for every subsequently created descendant.
/// The CLI calls this before parsing or launching work, closing the small gap
/// before a per-engine job can be attached when the owner is killed abruptly.
pub fn install_process_tree_guard() -> Result<(), UciError> {
    install_process_tree_guard_platform()
}

/// A running UCI engine.
pub struct EngineProcess {
    containment: ProcessContainment,
    child: Child,
    stdin: ChildStdin,
    /// Engine output, one line at a time, each stamped with the instant its
    /// newline arrived. A dedicated reader thread owns the pipe, so a line is
    /// timed when the engine sent it and not when a busy harness got round to
    /// reading it.
    lines: mpsc::UnboundedReceiver<PipeEvent>,
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
    /// The stamps of the most recent search, answered or not, until the
    /// caller takes them.
    last_timing: Option<SearchTiming>,
    /// When the game task last took a line off the reader's channel.
    last_consumed: Instant,
    /// A line that arrived after the deadline it was read against. It is not
    /// an answer, but when it came is evidence.
    late_line: Option<ArrivedLine>,
    /// The process's consumed CPU time, where the platform can read it
    /// precisely: set against a search's charged time, it says how long the
    /// engine was not running while its clock ran.
    cpu: Option<crate::cpu_time::ProcessCpu>,
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
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let parent = unsafe { libc::getpid() };
            // SAFETY: pre_exec performs only async-signal-safe libc calls.
            unsafe {
                std_cmd.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "engine owner exited during spawn",
                        ));
                    }
                    Ok(())
                });
            }
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
        let lines = spawn_pipe_reader(stdout)?;
        let cpu = child.id().and_then(crate::cpu_time::ProcessCpu::open);

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
            lines,
            info: HandshakeInfo::default(),
            transcript: VecDeque::new(),
            stderr_tail,
            ponder_early: None,
            last_timing: None,
            cpu,
            last_consumed: Instant::now(),
            late_line: None,
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
                .await?
                .text;
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
                .await?
                .text;
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
        self.send(&position.to_command()).await?;
        // The charged interval begins as the `go` is written and ends when the
        // `bestmove` arrives. Position setup stays outside it. The start is
        // taken before the write, not after it: the reader thread stamps an
        // answer the moment it lands, and a fast engine can answer before this
        // task returns from the write, which would otherwise make the interval
        // end before it began.
        let process_at_go = self.process_sample();
        let charged_from = Instant::now();
        self.send(&limits.to_command()).await?;
        let write_returned = Instant::now();
        self.await_bestmove(
            charged_from,
            process_at_go,
            write_returned,
            deadline,
            on_info,
        )
        .await
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
        let process_at_go = self.process_sample();
        let charged_from = Instant::now();
        self.send("stop").await?;
        let write_returned = Instant::now();
        let result = self
            .await_bestmove(
                charged_from,
                process_at_go,
                write_returned,
                deadline,
                on_info,
            )
            .await;
        self.mark_earlier_origin();
        result
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
                    let line = line.text.trim();
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
            // The engine already finished during ponder; its move is free,
            // and there was no round trip to time.
            self.last_timing = None;
            return Ok(SearchOutput {
                best_move,
                score: None,
                nps: None,
                reported_nodes: None,
                reported_time_ms: None,
                reported_nps: None,
                depth: None,
                ponder,
                elapsed: Duration::ZERO,
            });
        }
        let process_at_go = self.process_sample();
        let charged_from = Instant::now();
        self.send("ponderhit").await?;
        let write_returned = Instant::now();
        let result = self
            .await_bestmove(
                charged_from,
                process_at_go,
                write_returned,
                deadline,
                on_info,
            )
            .await;
        self.mark_earlier_origin();
        result
    }

    /// The search just timed was charged from a later command than the one
    /// that started the engine's clock.
    fn mark_earlier_origin(&mut self) {
        if let Some(timing) = self.last_timing.as_mut() {
            timing.earlier_origin = true;
        }
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
            if parse::parse_bestmove(line.text.trim()).is_some() {
                return Ok(());
            }
        }
    }

    /// The engine process's CPU time, kernel time and page faults, where the
    /// platform can read them precisely.
    fn process_sample(&self) -> Option<ProcessSample> {
        self.cpu.as_ref()?.sample()
    }

    /// The stamps of the most recent search, taken so the next search starts
    /// clean. `None` when the last move needed no round trip.
    pub fn take_search_timing(&mut self) -> Option<SearchTiming> {
        self.last_timing.take()
    }

    /// After a search missed its deadline, learn when its `bestmove` came.
    ///
    /// The game is already decided; this reads at most `window` more of the
    /// engine's output so the forensic can say whether the answer was a few
    /// milliseconds late or never came. It changes no result.
    pub async fn await_late_bestmove(&mut self, window: Duration) {
        let Some(mut timing) = self.last_timing.take() else {
            return;
        };
        if timing.bestmove_arrived.is_none() {
            if let Some(line) = self.late_line.take()
                && parse::parse_bestmove(line.text.trim()).is_some()
            {
                timing.bestmove_arrived = Some(line.arrived);
                timing.consumed = Some(self.last_consumed);
                timing.process_at_answer = self.process_sample();
                timing.late = true;
            } else {
                let until = Instant::now() + window;
                loop {
                    match self.read_line_until(until, UciError::MoveTimeout).await {
                        Ok(line) if parse::parse_bestmove(line.text.trim()).is_some() => {
                            timing.bestmove_arrived = Some(line.arrived);
                            timing.consumed = Some(self.last_consumed);
                            timing.process_at_answer = self.process_sample();
                            timing.late = true;
                            break;
                        }
                        Ok(_) => {}
                        // One bad line fails one read; the answer may still
                        // come behind it.
                        Err(UciError::Protocol(_)) => {}
                        Err(_) => break,
                    }
                }
            }
        }
        self.last_timing = Some(timing);
    }

    /// Read engine output until `bestmove`, tracking the last
    /// score/nps/depth and feeding `info` lines to `on_info`. The search's
    /// stamps are kept for [`Self::take_search_timing`] whether or not it
    /// answered in time.
    async fn await_bestmove(
        &mut self,
        start: Instant,
        process_at_go: Option<ProcessSample>,
        write_returned: Instant,
        deadline: Duration,
        on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        let mut timing = SearchTiming::new(start, write_returned, start + deadline);
        timing.process_at_go = process_at_go;
        self.late_line = None;
        let result = self.read_search(&mut timing, on_info).await;
        self.last_timing = Some(timing);
        result
    }

    async fn read_search(
        &mut self,
        timing: &mut SearchTiming,
        mut on_info: impl FnMut(&parse::InfoLine),
    ) -> Result<SearchOutput, UciError> {
        let start = timing.go_stamped;
        let until = timing.deadline;
        let mut score = None;
        let mut reported_nps = None;
        let mut nodes = None;
        let mut time_ms = None;
        let mut depth = None;

        loop {
            let line = self.read_line_until(until, UciError::MoveTimeout).await?;
            // The charged interval ends when the `bestmove` line arrived on the
            // pipe, as stamped by the reader thread. Whatever the harness did
            // between that instant and now — another game's commit, a slow
            // disk, a busy runtime — is the harness's time, not the engine's.
            let arrived = line.arrived;
            let line = line.text.trim();
            if let Some((best_move, ponder)) = parse::parse_bestmove_ponder(line) {
                timing.bestmove_arrived = Some(arrived);
                timing.consumed = Some(self.last_consumed);
                timing.process_at_answer = self.process_sample();
                let elapsed = charged_elapsed(start, arrived);
                // Some engines report a literal `nps 0` on every info line
                // (Fruit 2.1 does) — treat that as unreported and derive the
                // real speed from nodes over wall-clock time instead.
                let nps = reported_nps.or_else(|| {
                    nodes.map(|n: u64| (n as f64 / elapsed.as_secs_f64().max(0.001)).round() as u64)
                });
                return Ok(SearchOutput {
                    best_move,
                    score,
                    nps,
                    reported_nodes: nodes,
                    reported_time_ms: time_ms,
                    reported_nps,
                    depth,
                    ponder,
                    elapsed,
                });
            }
            if line.starts_with("info ")
                && let Some(info) = parse::parse_info_line(line)
            {
                timing.info(arrived, info.time_ms);
                if info.score.is_some() {
                    score = info.score;
                }
                if let Some(n) = info.nps
                    && n > 0
                {
                    reported_nps = Some(n);
                }
                if info.nodes.is_some() {
                    nodes = info.nodes;
                }
                if info.time_ms.is_some() {
                    time_ms = info.time_ms;
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
    ///
    /// The deadline is judged by when a line arrived, not by when it is
    /// looked at: a `bestmove` the engine sent in time is in time even if the
    /// harness only reaches it after the deadline, and a line that arrived
    /// after the deadline is late however early the harness asks.
    async fn read_line_until(
        &mut self,
        until: Instant,
        timeout_err: UciError,
    ) -> Result<ArrivedLine, UciError> {
        let event = match self.lines.try_recv() {
            Ok(event) => event,
            Err(mpsc::error::TryRecvError::Disconnected) => return Err(UciError::Terminated),
            Err(mpsc::error::TryRecvError::Empty) => {
                // On the `None` path we diverge by returning, so `timeout_err`
                // is only ever moved once and needs no `Clone`.
                let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                    return Err(timeout_err);
                };
                match timeout(remaining, self.lines.recv()).await {
                    Err(_elapsed) => return Err(timeout_err),
                    Ok(None) => return Err(UciError::Terminated),
                    Ok(Some(event)) => event,
                }
            }
        };
        self.last_consumed = Instant::now();
        match event {
            PipeEvent::Line(line) => {
                if line.arrived > until {
                    self.record("<", &line.text);
                    self.late_line = Some(line);
                    return Err(timeout_err);
                }
                tracing::trace!(target: "uci", direction = "recv", "{}", line.text);
                self.record("<", &line.text);
                Ok(line)
            }
            PipeEvent::Overlong { bytes } => {
                // One bad line fails the read that met it. The reader thread
                // has already skipped past it and keeps draining, so the
                // session still answers `stop`, `isready` and `quit`.
                self.record("<", &format!("[{bytes}-byte line discarded]"));
                Err(overlong_error())
            }
            PipeEvent::Eof => Err(UciError::Terminated),
            PipeEvent::Failed(error) => Err(error),
        }
    }
}

fn overlong_error() -> UciError {
    UciError::Protocol(format!(
        "protocol line exceeds {MAX_PROTOCOL_LINE_BYTES} bytes"
    ))
}

fn charged_elapsed(start: Instant, arrived: Instant) -> Duration {
    arrived.saturating_duration_since(start)
}

/// One protocol line and the instant its newline arrived on the pipe.
#[derive(Debug)]
struct ArrivedLine {
    text: String,
    arrived: Instant,
}

/// What the reader thread reports.
#[derive(Debug)]
enum PipeEvent {
    Line(ArrivedLine),
    /// A line longer than [`MAX_PROTOCOL_LINE_BYTES`], skipped to its
    /// newline. The pipe goes on being read.
    Overlong {
        bytes: usize,
    },
    Eof,
    /// The pipe could not be read. Nothing more will come.
    Failed(UciError),
}

/// Move the engine's stdout onto a thread that does nothing but read it.
///
/// A thread blocked in `read` wakes when the engine writes, whatever the async
/// runtime is doing, so the instant it records is the instant the line
/// arrived. It exits when the pipe closes, which the engine's containment
/// guarantees when the process ends.
fn spawn_pipe_reader(stdout: ChildStdout) -> Result<mpsc::UnboundedReceiver<PipeEvent>, UciError> {
    #[cfg(unix)]
    let file = std::fs::File::from(stdout.into_owned_fd()?);
    #[cfg(windows)]
    let file = std::fs::File::from(stdout.into_owned_handle()?);
    let (sender, receiver) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("uci-stdout".into())
        .spawn(move || read_pipe(std::io::BufReader::new(file), &sender))?;
    Ok(receiver)
}

fn read_pipe(mut reader: impl std::io::BufRead, sender: &mpsc::UnboundedSender<PipeEvent>) {
    loop {
        let event = match read_bounded_line(&mut reader) {
            Ok(Some(RawLine::Text(text))) => PipeEvent::Line(ArrivedLine {
                text,
                arrived: Instant::now(),
            }),
            Ok(Some(RawLine::Overlong(bytes))) => PipeEvent::Overlong { bytes },
            Ok(None) => PipeEvent::Eof,
            Err(error) => PipeEvent::Failed(error.into()),
        };
        let last = matches!(event, PipeEvent::Eof | PipeEvent::Failed(_));
        // A closed receiver means the engine handle is gone; nobody is
        // listening, so the thread has nothing left to do.
        if sender.send(event).is_err() || last {
            return;
        }
    }
}

/// One line as the reader took it off the pipe.
#[derive(Debug)]
enum RawLine {
    Text(String),
    /// Longer than the protocol allows; this many bytes were skipped.
    Overlong(usize),
}

/// Read one line of at most [`MAX_PROTOCOL_LINE_BYTES`], without its newline.
/// A longer line is consumed to its newline without being kept, so the next
/// read starts on the next line. `None` is end of stream.
fn read_bounded_line(reader: &mut impl std::io::BufRead) -> std::io::Result<Option<RawLine>> {
    let mut bytes = Vec::new();
    // Once the line is too long, its bytes are counted, not kept.
    let mut skipped: Option<usize> = None;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(match skipped {
                Some(count) => Some(RawLine::Overlong(count)),
                None if bytes.is_empty() => None,
                None => Some(RawLine::Text(String::from_utf8_lossy(&bytes).into_owned())),
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content = if newline.is_some() { take - 1 } else { take };
        if skipped.is_none() && bytes.len().saturating_add(content) > MAX_PROTOCOL_LINE_BYTES {
            skipped = Some(bytes.len());
            bytes = Vec::new();
        }
        match &mut skipped {
            Some(count) => *count = count.saturating_add(content),
            None => bytes.extend_from_slice(&available[..content]),
        }
        reader.consume(take);
        if newline.is_some() {
            if let Some(count) = skipped {
                return Ok(Some(RawLine::Overlong(count)));
            }
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            return Ok(Some(RawLine::Text(
                String::from_utf8_lossy(&bytes).into_owned(),
            )));
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

#[cfg(windows)]
fn install_process_tree_guard_platform() -> Result<(), UciError> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: the job is configured before assignment. On success its handle is
    // deliberately process-lifetime: the OS closes it during process teardown,
    // which is exactly what triggers descendant termination after abrupt exit.
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
            || AssignProcessToJobObject(job, GetCurrentProcess()) == 0
        {
            windows_sys::Win32::Foundation::CloseHandle(job);
            return Err(UciError::Io(std::io::Error::last_os_error()));
        }
        // Do not CloseHandle on the success path. Closing a kill-on-close job
        // containing this process would terminate the CLI itself. The kernel
        // owns cleanup when the process exits normally or is killed.
        Ok(())
    }
}

#[cfg(not(windows))]
fn install_process_tree_guard_platform() -> Result<(), UciError> {
    Ok(())
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_line_is_stamped_when_it_arrives_not_when_it_is_read() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let before = Instant::now();
        read_pipe(
            std::io::Cursor::new(b"info depth 1\nbestmove e2e4\r\n".to_vec()),
            &sender,
        );
        let after = Instant::now();
        // The consumer only looks now, long after the reader stamped both.
        std::thread::sleep(Duration::from_millis(50));
        let mut lines = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            lines.push(event);
        }
        let PipeEvent::Line(bestmove) = &lines[1] else {
            panic!("{lines:?}");
        };
        assert_eq!(bestmove.text, "bestmove e2e4");
        assert!(bestmove.arrived >= before && bestmove.arrived <= after);
        assert!(matches!(lines[2], PipeEvent::Eof));
    }

    #[test]
    fn an_overlong_line_is_a_protocol_error_not_a_hang() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        read_pipe(
            std::io::Cursor::new(vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 1]),
            &sender,
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(PipeEvent::Overlong { bytes }) if bytes == MAX_PROTOCOL_LINE_BYTES + 1
        ));
        assert!(matches!(receiver.try_recv(), Ok(PipeEvent::Eof)));
    }

    #[test]
    fn the_reader_skips_an_overlong_line_and_keeps_delivering_the_next_ones() {
        // Small reads, so the long line spans many buffer refills.
        let mut input = b"info depth 1\n".to_vec();
        input.extend(vec![b'x'; MAX_PROTOCOL_LINE_BYTES * 2]);
        input.extend(b"\r\nbestmove e2e4\n");
        let (sender, mut receiver) = mpsc::unbounded_channel();
        read_pipe(
            std::io::BufReader::with_capacity(4096, std::io::Cursor::new(input)),
            &sender,
        );
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        assert!(
            matches!(&events[..], [
                PipeEvent::Line(first),
                PipeEvent::Overlong { .. },
                PipeEvent::Line(last),
                PipeEvent::Eof,
            ] if first.text == "info depth 1" && last.text == "bestmove e2e4"),
            "{events:?}"
        );
        assert!(overlong_error().to_string().contains("exceeds"));
    }

    #[test]
    fn charged_search_time_is_independent_of_a_mid_search_wall_clock_jump() {
        let start = Instant::now();
        let read_finished = start + Duration::from_millis(42);
        let wall_before = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let wall_after = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        assert!(
            wall_after < wall_before,
            "fixture represents a backward jump"
        );
        assert_eq!(
            charged_elapsed(start, read_finished),
            Duration::from_millis(42)
        );
    }
}
