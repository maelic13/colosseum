//! An async handle to a running UCI engine process.
//!
//! Responsibilities: spawn (with args/workdir/env), the `uci`/`isready` handshake,
//! `setoption`/`ucinewgame`, running a search to `bestmove` under a deadline, and
//! clean shutdown. The child is configured with `kill_on_drop(true)` so an engine is
//! never leaked if the handle is dropped (e.g. on Force-Stop or a panic).
//!
//! Note: `kill_on_drop` reaps the engine process itself. UCI engines do not normally
//! spawn helper processes; hardening against grandchildren (Unix process groups /
//! Windows job objects) is a documented follow-up.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use colosseum_core::UciOption;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
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
    /// Wall-clock time actually spent on this search.
    pub elapsed: Duration,
}

/// A running UCI engine.
pub struct EngineProcess {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    info: HandshakeInfo,
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
            .stderr(Stdio::null());
        if let Some(dir) = &options.working_dir {
            std_cmd.current_dir(dir);
        }
        for (key, value) in &options.env {
            std_cmd.env(key, value);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: don't pop up a console for each engine.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std_cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut command = Command::from(std_cmd);
        command.kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or(UciError::Terminated)?;
        let stdout = child.stdout.take().ok_or(UciError::Terminated)?;
        let lines = BufReader::new(stdout).lines();

        Ok(Self {
            child,
            stdin,
            lines,
            info: HandshakeInfo::default(),
        })
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
    /// the last score/nps/depth. Fails with [`UciError::MoveTimeout`] if `deadline`
    /// elapses first, or [`UciError::Terminated`] if the engine exits mid-search.
    pub async fn search(
        &mut self,
        position: &UciPosition,
        limits: &GoLimits,
        deadline: Duration,
    ) -> Result<SearchOutput, UciError> {
        self.send(&position.to_command()).await?;
        self.send(&limits.to_command()).await?;

        let start = Instant::now();
        let until = start + deadline;
        let mut score = None;
        let mut nps = None;
        let mut depth = None;

        loop {
            let line = self.read_line_until(until, UciError::MoveTimeout).await?;
            let line = line.trim();
            if let Some(best_move) = parse::parse_bestmove(line) {
                return Ok(SearchOutput {
                    best_move,
                    score,
                    nps,
                    depth,
                    elapsed: start.elapsed(),
                });
            }
            if line.starts_with("info ")
                && let Some(info) = parse::parse_info_line(line)
            {
                if info.score.is_some() {
                    score = info.score;
                }
                if info.nps.is_some() {
                    nps = info.nps;
                }
                if info.depth.is_some() {
                    depth = info.depth;
                }
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
                let _ = self.child.start_kill();
                Ok(())
            }
        }
    }

    /// Immediately kill the engine (for Force-Stop). Waits for the process to be reaped.
    pub async fn kill(&mut self) -> Result<(), UciError> {
        self.child.kill().await.map_err(UciError::Io)
    }

    /// Write a command line to the engine and flush.
    async fn send(&mut self, line: &str) -> Result<(), UciError> {
        tracing::trace!(target: "uci", direction = "send", "{line}");
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
        match timeout(remaining, self.lines.next_line()).await {
            Err(_elapsed) => Err(timeout_err),
            Ok(Ok(Some(line))) => {
                tracing::trace!(target: "uci", direction = "recv", "{line}");
                Ok(line)
            }
            Ok(Ok(None)) => Err(UciError::Terminated),
            Ok(Err(err)) => Err(UciError::Io(err)),
        }
    }
}
