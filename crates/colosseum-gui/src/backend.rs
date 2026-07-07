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

use colosseum_core::{
    EngineConfig, EngineId, RatingWriteback, TournamentConfig, TournamentEvent, TournamentId,
};
use colosseum_engine::{
    AppConfig, AppDirs, EngineLibrary, Store, Tournament, TournamentResults, TournamentRow,
    TournamentSnapshot, TournamentStatus, create_tournament, load_tournament_results,
    resume_tournament,
};

/// Target redraw cadence while a tournament is running (~30 Hz).
pub const LIVE_REPAINT: Duration = Duration::from_millis(33);

/// A participant's display identity, captured at tournament start so the live
/// results table is independent of later edits to the engine library.
#[derive(Debug, Clone)]
pub struct ParticipantInfo {
    pub id: EngineId,
    pub name: String,
    pub version: String,
}

/// The currently-active tournament: its control handle, the live snapshot the
/// GUI renders, the event channel that nudges repaints, plus display metadata.
pub struct ActiveTournament {
    pub handle: Tournament,
    pub snapshot: Arc<Mutex<TournamentSnapshot>>,
    pub events: Receiver<TournamentEvent>,
    pub name: String,
    pub participants: Vec<ParticipantInfo>,
    /// The tournament's full configuration (format, time control, …), kept
    /// for the live view (gauntlet layout, ETA fallback, writeback mode).
    pub config: TournamentConfig,
    /// Ratings at *tournament start* (the DB's `start_elo` seeds; 1500 for
    /// unrated engines) — the fixed baseline for the Elo Δ column. Pinned at
    /// tournament creation, not at load, so later library edits and the
    /// per-game writeback never shift the Δ column.
    pub priors: Vec<(EngineId, f64)>,
    /// Which engines' library ratings follow this tournament, applied after
    /// every finished game.
    pub rating_writeback: RatingWriteback,
    /// `games_finished` when the writeback last ran (apply once per new game,
    /// and never on mere load).
    pub writeback_at: usize,
}

/// All backend resources owned by the GUI.
pub struct Backend {
    pub dirs: AppDirs,
    pub config: AppConfig,
    pub engines: Vec<EngineConfig>,
    /// SQLite connection for tournament history / resume queries.
    pub store: Store,
    /// Runtime that drives engine detection (Step 8) and tournament games (Step 9).
    pub runtime: tokio::runtime::Runtime,
    /// Loaded tournaments (started or resumed this session). Several can be
    /// loaded — and even running — at once; each owns its driver + snapshot.
    pub actives: Vec<ActiveTournament>,
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
            actives: Vec::new(),
        })
    }

    /// The loaded tournament with this id, if any.
    #[must_use]
    pub fn active(&self, id: TournamentId) -> Option<&ActiveTournament> {
        self.actives.iter().find(|a| a.handle.id == id)
    }


    /// Drain pending tournament events and return the repaint interval the UI
    /// should request this frame (`Some` while live, `None` when idle).
    ///
    /// Events only signal change; the authoritative state is read from the
    /// snapshot. Draining keeps the channel from growing unbounded. Also the
    /// hook that applies the configured rating writeback after each game.
    pub fn poll(&mut self) -> Option<Duration> {
        if self.actives.is_empty() {
            return None;
        }
        for active in &self.actives {
            while active.events.try_recv().is_ok() {}
        }
        self.apply_rating_writebacks();
        self.actives
            .iter()
            .filter_map(|a| a.snapshot.lock().ok().map(|s| s.status))
            .filter_map(repaint_interval)
            .min()
    }

    /// Apply the configured rating writebacks: whenever a tournament has new
    /// finished games, its live ML ratings are written to the library for the
    /// engines the mode covers — the library tracks the tournament game by
    /// game instead of jumping once at the end.
    fn apply_rating_writebacks(&mut self) {
        let due: Vec<TournamentId> = self
            .actives
            .iter_mut()
            .filter_map(|a| {
                if matches!(a.rating_writeback, RatingWriteback::None) {
                    return None;
                }
                let finished = a.snapshot.lock().ok().map(|s| s.games_finished)?;
                if finished == a.writeback_at {
                    return None;
                }
                a.writeback_at = finished;
                Some(a.handle.id)
            })
            .collect();
        for id in due {
            self.apply_ratings_to_library(id);
        }
    }

    /// Write tournament `id`'s current ML ratings into the library for every
    /// engine its writeback mode covers (and that has played a game). Returns
    /// how many library entries changed.
    pub fn apply_ratings_to_library(&mut self, id: TournamentId) -> usize {
        let Some(active) = self.active(id) else {
            return 0;
        };
        let mode = active.rating_writeback.clone();
        let Ok(snap) = active.snapshot.lock() else {
            return 0;
        };
        let updates: Vec<(EngineId, i32)> = snap
            .elo
            .iter()
            .filter(|(eid, _)| snap.standings.standing(**eid).games() > 0)
            .filter(|(eid, _)| {
                // The manual button applies for every played engine; the
                // automatic path respects the configured mode.
                matches!(mode, RatingWriteback::None) || mode.applies_to(**eid)
            })
            .map(|(eid, entry)| (*eid, entry.current.round() as i32))
            .collect();
        drop(snap);

        let mut changed = 0;
        for (eid, elo) in updates {
            if let Some(engine) = self.engines.iter_mut().find(|e| e.id == eid)
                && engine.meta.elo != Some(elo)
            {
                engine.meta.elo = Some(elo);
                changed += 1;
            }
        }
        if changed > 0 {
            self.save_engines();
        }
        changed
    }

    /// The liveliest status across all loaded tournaments (drives the header
    /// pill): Running beats Stopping beats everything else.
    #[must_use]
    pub fn status(&self) -> Option<TournamentStatus> {
        let statuses: Vec<TournamentStatus> = self
            .actives
            .iter()
            .filter_map(|a| a.snapshot.lock().ok().map(|s| s.status))
            .collect();
        if statuses.is_empty() {
            return None;
        }
        for pick in [
            TournamentStatus::Running,
            TournamentStatus::Stopping,
            TournamentStatus::Stopped,
            TournamentStatus::Idle,
        ] {
            if statuses.contains(&pick) {
                return Some(pick);
            }
        }
        Some(TournamentStatus::Finished)
    }

    /// Whether any tournament is running or draining (blocks a clean exit).
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.actives
            .iter()
            .filter_map(|a| a.snapshot.lock().ok().map(|s| s.status))
            .any(is_busy)
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

    /// Create a fresh tournament from `engines` + `config`, spawn its driver on
    /// the runtime, and immediately begin play. Other loaded tournaments are
    /// unaffected — several can run at once. The dedicated `Store` connection
    /// handed to the driver is separate from `self.store` so the background
    /// thread owns its own handle.
    pub fn start_tournament(
        &mut self,
        name: &str,
        config: TournamentConfig,
        engines: Vec<EngineConfig>,
    ) -> anyhow::Result<()> {
        let participants: Vec<ParticipantInfo> = engines
            .iter()
            .map(|e| ParticipantInfo {
                id: e.id,
                name: if e.meta.name.is_empty() {
                    e.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string()
                } else {
                    e.meta.name.clone()
                },
                version: e.meta.version.clone(),
            })
            .collect();

        let driver_store = Store::open(self.dirs.database_path())?;
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let rating_writeback = config.rating_writeback.clone();
        let config_copy = config.clone();
        // Priors = library ratings at this moment: the tournament's start.
        let priors = engines
            .iter()
            .map(|e| (e.id, e.meta.elo.map_or(1500.0, f64::from)))
            .collect();
        let (handle, driver) = create_tournament(name, config, engines, driver_store, events_tx)?;
        let snapshot = handle.snapshot_handle();

        self.runtime.spawn(driver);
        handle.go(); // begin play immediately

        self.actives.push(ActiveTournament {
            handle,
            snapshot,
            events: events_rx,
            name: name.to_string(),
            participants,
            config: config_copy,
            priors,
            rating_writeback,
            writeback_at: 0,
        });
        Ok(())
    }

    /// Unload a tournament (after it has stopped or finished). Does not delete
    /// persisted data — it stays in the Arena list.
    pub fn close_tournament(&mut self, id: TournamentId) {
        self.actives.retain(|a| a.handle.id != id);
    }

    /// List all stored tournaments, most recent first, for the History tab.
    pub fn list_tournaments(&self) -> Vec<TournamentRow> {
        self.store.list_tournaments().unwrap_or_else(|e| {
            tracing::warn!("failed to list tournaments: {e}");
            Vec::new()
        })
    }

    /// Reconstruct a stored tournament's results read-only (no engines spawned).
    pub fn tournament_results(&self, row: &TournamentRow) -> anyhow::Result<TournamentResults> {
        Ok(load_tournament_results(&self.store, row)?)
    }

    /// Delete a tournament and all of its games from the database.
    pub fn delete_tournament(&self, id: TournamentId) -> anyhow::Result<()> {
        self.store.delete_tournament(id)?;
        Ok(())
    }

    /// Concatenate the PGN of every finished game in a tournament, in play
    /// order, separated by blank lines. Empty if no games have finished yet.
    pub fn collect_pgn(&self, id: TournamentId) -> anyhow::Result<String> {
        let games = self.store.list_games(id)?;
        let mut out = String::new();
        for g in games {
            if let Some(pgn) = g.pgn.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(pgn);
            }
        }
        Ok(out)
    }

    /// Wire a previously-persisted tournament back up as a loaded tournament
    /// (in Stopped/Idle state — the user must press Go to begin play). A no-op
    /// if it is already loaded.
    pub fn try_resume(&mut self, row: TournamentRow) -> anyhow::Result<()> {
        let name = row.name.clone();
        let id = row.id;
        if self.active(id).is_some() {
            return Ok(());
        }

        let driver_store = Store::open(self.dirs.database_path())?;
        let participants_raw = driver_store.list_tournament_engines(id)?;

        let participants: Vec<ParticipantInfo> = participants_raw
            .iter()
            .filter_map(|p| {
                let cfg = p.config.as_ref()?;
                let name = if cfg.meta.name.is_empty() {
                    cfg.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string()
                } else {
                    cfg.meta.name.clone()
                };
                Some(ParticipantInfo {
                    id: p.engine,
                    name,
                    version: cfg.meta.version.clone(),
                })
            })
            .collect();

        // Priors come from the tournament's stored start ratings — NOT the
        // current library — so the Δ column always measures against the
        // tournament's beginning, even across resumes and after writebacks.
        let priors: Vec<(EngineId, f64)> = participants_raw
            .iter()
            .map(|p| (p.engine, p.start_elo))
            .collect();

        let resume_store = Store::open(self.dirs.database_path())?;
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let rating_writeback = row.config.rating_writeback.clone();
        let config_copy = row.config.clone();
        let (handle, driver) = resume_tournament(row, resume_store, events_tx)?;
        let snapshot = handle.snapshot_handle();
        let finished_now = snapshot.lock().map_or(0, |s| s.games_finished);

        self.runtime.spawn(driver);
        // Do NOT call handle.go() — leave in Stopped state; user presses Go.

        self.actives.push(ActiveTournament {
            handle,
            snapshot,
            events: events_rx,
            name,
            participants,
            config: config_copy,
            priors,
            rating_writeback,
            // Loading must not write anything — only new games do.
            writeback_at: finished_now,
        });
        Ok(())
    }

    /// Change a loaded tournament's parallel-games limit (running or
    /// stopped). In-flight games always finish; only the launch rate changes.
    pub fn set_active_concurrency(&self, id: TournamentId, limit: usize) {
        if let Some(active) = self.active(id) {
            active.handle.set_concurrency(limit);
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
