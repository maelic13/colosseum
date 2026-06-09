//! The bridge between the egui front-end and the async tournament backend.
//!
//! [`Backend`] owns the process-wide resources the GUI needs: the tokio runtime
//! that drives games, a SQLite [`Store`] connection for the engine library and
//! tournament history, the loaded engine library, persisted [`AppConfig`], and
//! the slot for the currently-active tournament.
//!
//! The front-end is immediate-mode and must never block on engine I/O. The
//! backend instead publishes an `Arc<Mutex<TournamentSnapshot>>` that the GUI
//! reads each frame, and a lightweight [`TournamentEvent`] channel that merely
//! signals "something changed" so the UI can repaint promptly. While a
//! tournament is live the UI also repaints on a ~30 Hz timer (see
//! [`repaint_interval`]) so the results table animates smoothly even if events
//! are briefly quiet.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Receiver;

use colosseum_core::{EngineConfig, TournamentEvent};
use colosseum_engine::{
    AppConfig, AppDirs, EngineLibrary, Store, Tournament, TournamentSnapshot, TournamentStatus,
};

/// Target redraw cadence while a tournament is running (~30 Hz).
pub const LIVE_REPAINT: Duration = Duration::from_millis(33);

/// The currently-active tournament: its control handle, the live snapshot the
/// GUI renders, and the event channel that nudges repaints.
///
/// Constructed when a tournament is launched from the Tournament tab (Step 9).
pub struct ActiveTournament {
    pub handle: Tournament,
    pub snapshot: Arc<Mutex<TournamentSnapshot>>,
    pub events: Receiver<TournamentEvent>,
}

/// All backend resources owned by the GUI.
pub struct Backend {
    pub dirs: AppDirs,
    pub config: AppConfig,
    pub engines: Vec<EngineConfig>,
    /// SQLite connection for tournament history / resume queries.
    #[expect(
        dead_code,
        reason = "queried by the Tournament/History flows in Step 9"
    )]
    pub store: Store,
    /// Runtime that drives engine detection (Step 8) and tournament games (Step 9).
    pub runtime: tokio::runtime::Runtime,
    pub active: Option<ActiveTournament>,
}

impl Backend {
    /// Initialise the backend: resolve storage directories (falling back to a
    /// portable layout if the OS dirs can't be determined), load config and the
    /// engine library, open the database, and start a tokio runtime.
    pub fn new(portable: bool) -> anyhow::Result<Self> {
        let dirs = AppDirs::new(portable)
            .or_else(|| AppDirs::new(true))
            .ok_or_else(|| anyhow::anyhow!("could not determine application directories"))?;
        dirs.ensure_dirs()?;

        let config = AppConfig::load(&dirs.config_file()).unwrap_or_else(|e| {
            tracing::warn!("failed to load config, using defaults: {e}");
            AppConfig::default()
        });

        let engines = EngineLibrary::load(&dirs.engines_file()).unwrap_or_else(|e| {
            tracing::warn!("failed to load engine library, starting empty: {e}");
            Vec::new()
        });

        let store = Store::open(dirs.database_path())?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        tracing::info!(
            "backend ready: config_dir={}, data_dir={}, engines={}",
            dirs.config_dir.display(),
            dirs.data_dir.display(),
            engines.len()
        );

        Ok(Self {
            dirs,
            config,
            engines,
            store,
            runtime,
            active: None,
        })
    }

    /// Drain pending tournament events and return the repaint interval the UI
    /// should request this frame (`Some` while live, `None` when idle).
    ///
    /// Events only signal change; the authoritative state is read from the
    /// snapshot. Draining keeps the channel from growing unbounded.
    pub fn poll(&self) -> Option<Duration> {
        let active = self.active.as_ref()?;
        while active.events.try_recv().is_ok() {}
        repaint_interval(self.status()?)
    }

    /// The active tournament's status, if any.
    #[must_use]
    pub fn status(&self) -> Option<TournamentStatus> {
        self.active
            .as_ref()
            .and_then(|a| a.snapshot.lock().ok().map(|s| s.status))
    }

    /// Whether a tournament is currently running or draining (blocks a clean exit).
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.status().is_some_and(is_busy)
    }

    /// Persist the current [`AppConfig`] to disk, logging any failure.
    pub fn save_config(&self) {
        if let Err(e) = self.config.save(&self.dirs.config_file()) {
            tracing::warn!("failed to save config: {e}");
        }
    }

    /// Persist the engine library to disk, logging any failure.
    pub fn save_engines(&self) {
        if let Err(e) = EngineLibrary::save(&self.engines, &self.dirs.engines_file()) {
            tracing::warn!("failed to save engines: {e}");
        }
    }
}

/// The repaint interval for a given tournament status: a steady ~30 Hz while
/// games are in flight, and `None` (event-driven only) once idle/stopped/done.
#[must_use]
pub fn repaint_interval(status: TournamentStatus) -> Option<Duration> {
    if is_busy(status) {
        Some(LIVE_REPAINT)
    } else {
        None
    }
}

/// Whether a status represents in-flight work (running or draining after Stop).
#[must_use]
pub fn is_busy(status: TournamentStatus) -> bool {
    matches!(
        status,
        TournamentStatus::Running | TournamentStatus::Stopping
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_only_while_running_or_stopping() {
        assert!(is_busy(TournamentStatus::Running));
        assert!(is_busy(TournamentStatus::Stopping));
        assert!(!is_busy(TournamentStatus::Idle));
        assert!(!is_busy(TournamentStatus::Stopped));
        assert!(!is_busy(TournamentStatus::Finished));
    }

    #[test]
    fn repaint_interval_matches_busy_states() {
        assert_eq!(
            repaint_interval(TournamentStatus::Running),
            Some(LIVE_REPAINT)
        );
        assert_eq!(
            repaint_interval(TournamentStatus::Stopping),
            Some(LIVE_REPAINT)
        );
        assert_eq!(repaint_interval(TournamentStatus::Idle), None);
        assert_eq!(repaint_interval(TournamentStatus::Stopped), None);
        assert_eq!(repaint_interval(TournamentStatus::Finished), None);
    }
}
