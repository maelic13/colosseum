//! Tournament scheduler: the Go / Stop / Force-Stop / resume state machine.
//!
//! The driver runs on a tokio runtime. It launches up to `concurrency` games at once
//! (each game owns two engine processes), collects their reports, persists results,
//! updates standings + Elo, appends PGN, and publishes a [`TournamentSnapshot`] that
//! the GUI reads each frame. Control commands arrive over an mpsc channel; lightweight
//! [`TournamentEvent`]s are pushed to a crossbeam channel to nudge the GUI to refresh.
//!
//! Control semantics:
//! - **Go**: start, or resume after Stop, or top up running games to the limit.
//! - **Stop**: launch no new games; let in-flight games finish and count as results.
//! - **Force-Stop**: abort in-flight games (kill engines via drop), discard them.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;

use colosseum_core::{
    CommonEngineOptions, EloPolicy, EngineConfig, EngineId, GameId, GameResult, IncrementalElo,
    Pairing, Rating, Standings, TournamentConfig, TournamentEvent, TournamentId, generate_schedule,
    standings::GameOutcome,
};
use colosseum_uci::SpawnOptions;

use crate::error::EngineError;
use crate::runner::{EngineGameSpec, GameSpec, run_game};
use crate::store::{self, Store};

/// Elo assigned to an engine that has no configured rating.
const DEFAULT_ELO: f64 = 1500.0;
/// Extra time beyond the move time before a move is judged a timeout.
const TIMEOUT_TOLERANCE: Duration = Duration::from_secs(2);
/// Time allowed for handshake / isready.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Cap on retained recent engine-error messages.
const MAX_RECENT_ERRORS: usize = 50;

/// A control command from the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Go,
    Stop,
    ForceStop,
}

/// High-level tournament status for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TournamentStatus {
    Idle,
    Running,
    /// Stop pressed; draining in-flight games.
    Stopping,
    Stopped,
    Finished,
}

/// An engine's current rating and change since the tournament started.
#[derive(Debug, Clone, Copy, Default)]
pub struct EloEntry {
    pub current: f64,
    pub delta: f64,
}

/// A consistent snapshot of tournament state for the GUI to render.
#[derive(Debug, Clone)]
pub struct TournamentSnapshot {
    pub status: TournamentStatus,
    pub standings: Standings,
    pub elo: HashMap<EngineId, EloEntry>,
    pub games_finished: usize,
    pub games_total: usize,
    pub recent_errors: Vec<String>,
}

impl TournamentSnapshot {
    fn new(total: usize) -> Self {
        Self {
            status: TournamentStatus::Idle,
            standings: Standings::new(),
            elo: HashMap::new(),
            games_finished: 0,
            games_total: total,
            recent_errors: Vec::new(),
        }
    }
}

/// A handle to a running tournament: send commands, read the live snapshot.
pub struct Tournament {
    pub id: TournamentId,
    commands: UnboundedSender<Command>,
    snapshot: Arc<Mutex<TournamentSnapshot>>,
}

impl Tournament {
    /// Start, resume, or top up to the concurrency limit.
    pub fn go(&self) {
        let _ = self.commands.send(Command::Go);
    }

    /// Stop launching new games; let in-flight games finish.
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    /// Abort in-flight games (kill engines) and discard them.
    pub fn force_stop(&self) {
        let _ = self.commands.send(Command::ForceStop);
    }

    /// A cheap clone of the shared snapshot handle for the GUI to read each frame.
    #[must_use]
    pub fn snapshot_handle(&self) -> Arc<Mutex<TournamentSnapshot>> {
        Arc::clone(&self.snapshot)
    }
}

/// Per-engine launch template, resolved once.
#[derive(Clone)]
struct EngineTemplate {
    id: EngineId,
    name: String,
    spawn: SpawnOptions,
    options: Vec<(String, Option<String>)>,
    start_elo: f64,
}

/// A scheduled game (id + pairing).
#[derive(Clone, Copy)]
struct ScheduledGame {
    game_id: GameId,
    pairing: Pairing,
}

/// Create a fresh tournament: persist it, its participants and pending games, and
/// return a control [`Tournament`] handle plus the driver future to spawn on a runtime.
///
/// Requires at least two engines.
pub fn create_tournament(
    name: &str,
    config: TournamentConfig,
    engines: Vec<EngineConfig>,
    store: Store,
    events: Sender<TournamentEvent>,
) -> Result<(Tournament, impl Future<Output = ()>), EngineError> {
    if engines.len() < 2 {
        return Err(EngineError::Corrupt(
            "a tournament needs at least two engines".into(),
        ));
    }

    let id = TournamentId::new();
    store.create_tournament(id, name, &config)?;

    // Resolve per-engine templates and register participants.
    let mut templates: HashMap<EngineId, EngineTemplate> = HashMap::new();
    let mut ids: Vec<EngineId> = Vec::with_capacity(engines.len());
    for (seed, engine) in engines.iter().enumerate() {
        let start_elo = engine.meta.elo.map_or(DEFAULT_ELO, f64::from);
        store.add_tournament_engine(id, engine.id, seed as u32, start_elo)?;
        templates.insert(
            engine.id,
            EngineTemplate {
                id: engine.id,
                name: display_name(engine),
                spawn: SpawnOptions {
                    path: engine.path.clone(),
                    args: engine.args.clone(),
                    working_dir: engine.working_dir.clone(),
                    env: engine.env.clone().into_iter().collect(),
                },
                options: resolve_options(engine, &config.common),
                start_elo,
            },
        );
        ids.push(engine.id);
    }

    // Build and persist the schedule.
    let pairings = generate_schedule(&ids, &config);
    let mut schedule = Vec::with_capacity(pairings.len());
    for pairing in pairings {
        let game_id = GameId::new();
        store.insert_pending_game(game_id, id, pairing.round, pairing.white, pairing.black)?;
        schedule.push(ScheduledGame { game_id, pairing });
    }

    let snapshot = Arc::new(Mutex::new(TournamentSnapshot::new(schedule.len())));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let driver = drive(Driver {
        id,
        event_name: name.to_string(),
        config,
        templates,
        ids,
        schedule,
        store,
        events,
        snapshot: Arc::clone(&snapshot),
        commands: cmd_rx,
    });

    Ok((
        Tournament {
            id,
            commands: cmd_tx,
            snapshot,
        },
        driver,
    ))
}

/// Everything the driver loop owns.
struct Driver {
    id: TournamentId,
    event_name: String,
    config: TournamentConfig,
    templates: HashMap<EngineId, EngineTemplate>,
    ids: Vec<EngineId>,
    schedule: Vec<ScheduledGame>,
    store: Store,
    events: Sender<TournamentEvent>,
    snapshot: Arc<Mutex<TournamentSnapshot>>,
    commands: UnboundedReceiver<Command>,
}

async fn drive(mut driver: Driver) {
    let total = driver.schedule.len();
    let concurrency = driver.config.concurrency.max(1);

    let seeds: Vec<(EngineId, f64)> = driver
        .ids
        .iter()
        .map(|id| (*id, driver.templates[id].start_elo))
        .collect();
    let mut elo = IncrementalElo::with_seed(driver.config.k_factor, seeds.iter().copied());
    let mut standings = Standings::with_engines(&driver.ids);
    let mut finished_results: Vec<(EngineId, EngineId, GameResult)> = Vec::new();
    let mut recent_errors: Vec<String> = Vec::new();

    let mut games: JoinSet<crate::runner::GameReport> = JoinSet::new();
    let mut in_flight: HashSet<GameId> = HashSet::new();
    let mut next_index = 0usize;
    let mut running = false;
    let mut finished = false;

    publish(
        &driver.snapshot,
        &standings,
        &elo,
        &driver.ids,
        finished_results.len(),
        total,
        TournamentStatus::Idle,
        &recent_errors,
    );

    loop {
        // Launch while running, with spare capacity and pending games.
        while running && games.len() < concurrency && next_index < driver.schedule.len() {
            let scheduled = driver.schedule[next_index];
            next_index += 1;
            let _ = driver.store.mark_game_running(scheduled.game_id);
            let _ = driver.events.send(TournamentEvent::GameStarted {
                game_id: scheduled.game_id,
                white: scheduled.pairing.white,
                black: scheduled.pairing.black,
                round: scheduled.pairing.round,
            });
            in_flight.insert(scheduled.game_id);
            let spec = build_game_spec(&driver, &scheduled);
            games.spawn(run_game(spec));
        }

        // All games launched and drained: the tournament is finished.
        if running && games.is_empty() && next_index >= driver.schedule.len() {
            running = false;
            finished = true;
            if driver.config.elo_policy == EloPolicy::EndOfTournament {
                let mut recomputed =
                    IncrementalElo::with_seed(driver.config.k_factor, seeds.iter().copied());
                for (white, black, result) in &finished_results {
                    recomputed.update(*white, *black, *result);
                }
                elo = recomputed;
            }
            let _ = driver
                .store
                .set_tournament_status(driver.id, store::STATUS_FINISHED);
            publish(
                &driver.snapshot,
                &standings,
                &elo,
                &driver.ids,
                finished_results.len(),
                total,
                TournamentStatus::Finished,
                &recent_errors,
            );
            let _ = driver.events.send(TournamentEvent::StandingsUpdated);
            let _ = driver.events.send(TournamentEvent::TournamentFinished);
        }

        tokio::select! {
            command = driver.commands.recv() => match command {
                Some(Command::Go) => {
                    if !finished {
                        running = true;
                        let _ = driver.store.set_tournament_status(driver.id, store::STATUS_RUNNING);
                        publish_status(&driver.snapshot, TournamentStatus::Running);
                    }
                }
                Some(Command::Stop) => {
                    running = false;
                    let _ = driver.store.set_tournament_status(driver.id, store::STATUS_STOPPED);
                    let status = if in_flight.is_empty() {
                        TournamentStatus::Stopped
                    } else {
                        TournamentStatus::Stopping
                    };
                    publish_status(&driver.snapshot, status);
                }
                Some(Command::ForceStop) => {
                    games.shutdown().await; // abort tasks -> drop engines -> kill_on_drop
                    for game_id in in_flight.drain() {
                        let _ = driver.store.discard_game(game_id);
                    }
                    running = false;
                    let _ = driver.store.set_tournament_status(driver.id, store::STATUS_STOPPED);
                    publish_status(&driver.snapshot, TournamentStatus::Stopped);
                }
                None => break, // handle dropped
            },

            Some(joined) = games.join_next(), if !games.is_empty() => {
                match joined {
                    Ok(report) => {
                        in_flight.remove(&report.game_id);
                        let _ = driver.store.finish_game(
                            report.game_id,
                            report.result,
                            report.termination,
                            report.stats.white_nps,
                            report.stats.black_nps,
                            report.stats.plies,
                            &report.pgn,
                        );
                        append_pgn(&driver.config, &report.pgn);

                        standings.record(GameOutcome {
                            white: report.white,
                            black: report.black,
                            result: report.result,
                            white_nps: report.stats.white_nps,
                            black_nps: report.stats.black_nps,
                        });
                        finished_results.push((report.white, report.black, report.result));
                        if driver.config.elo_policy == EloPolicy::PerGame {
                            elo.update(report.white, report.black, report.result);
                        }

                        if let Some(message) = &report.error {
                            push_error(&mut recent_errors, format!("game {}: {message}", report.game_id));
                            let _ = driver.events.send(TournamentEvent::EngineError {
                                engine: report.white,
                                message: message.clone(),
                            });
                        }

                        let _ = driver.events.send(TournamentEvent::GameFinished {
                            game_id: report.game_id,
                            result: report.result,
                            termination: report.termination,
                            stats: report.stats,
                        });

                        let status = if running {
                            TournamentStatus::Running
                        } else if in_flight.is_empty() {
                            TournamentStatus::Stopped
                        } else {
                            TournamentStatus::Stopping
                        };
                        publish(
                            &driver.snapshot,
                            &standings,
                            &elo,
                            &driver.ids,
                            finished_results.len(),
                            total,
                            status,
                            &recent_errors,
                        );
                        let _ = driver.events.send(TournamentEvent::StandingsUpdated);
                    }
                    Err(join_error) => {
                        if join_error.is_panic() {
                            tracing::error!(target: "scheduler", "game task panicked: {join_error}");
                        }
                        // Cancelled tasks (Force-Stop) are expected; ignore.
                    }
                }
            }
        }
    }
}

/// Build the full [`GameSpec`] for a scheduled game.
fn build_game_spec(driver: &Driver, scheduled: &ScheduledGame) -> GameSpec {
    let white = &driver.templates[&scheduled.pairing.white];
    let black = &driver.templates[&scheduled.pairing.black];
    GameSpec {
        game_id: scheduled.game_id,
        event: driver.event_name.clone(),
        site: "Colosseum".into(),
        date: today_pgn_date(),
        round: scheduled.pairing.round,
        white: to_game_spec(white),
        black: to_game_spec(black),
        start_fen: None,
        time_control: driver.config.time_control,
        time_control_label: time_control_label(&driver.config),
        adjudication: driver.config.adjudication,
        timeout_tolerance: TIMEOUT_TOLERANCE,
        handshake_timeout: HANDSHAKE_TIMEOUT,
    }
}

fn to_game_spec(template: &EngineTemplate) -> EngineGameSpec {
    EngineGameSpec {
        id: template.id,
        name: template.name.clone(),
        spawn: template.spawn.clone(),
        options: template.options.clone(),
    }
}

/// Merge an engine's own option values with the tournament's common options.
fn resolve_options(
    engine: &EngineConfig,
    common: &CommonEngineOptions,
) -> Vec<(String, Option<String>)> {
    let mut values: std::collections::BTreeMap<String, String> = engine
        .options
        .iter()
        .map(|(name, value)| (name.clone(), value.as_uci_string()))
        .collect();

    if let Some(threads) = common.threads {
        values.insert("Threads".into(), threads.to_string());
    }
    if let Some(hash) = common.hash_mb {
        values.insert("Hash".into(), hash.to_string());
    }
    if let Some(path) = &common.syzygy_path
        && !path.is_empty()
    {
        values.insert("SyzygyPath".into(), path.clone());
    }
    if let Some(rule) = common.syzygy_50_move_rule {
        values.insert("Syzygy50MoveRule".into(), rule.to_string());
    }
    // Ponder is always forwarded (default off keeps fast games fair).
    values.insert("Ponder".into(), common.ponder.to_string());

    values
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect()
}

fn display_name(engine: &EngineConfig) -> String {
    if engine.meta.name.is_empty() {
        engine
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| engine.id.to_string())
    } else {
        engine.meta.name.clone()
    }
}

fn time_control_label(config: &TournamentConfig) -> String {
    match config.time_control {
        colosseum_core::TimeControl::PerMove { ms } => format!("movetime/{ms}ms"),
    }
}

fn append_pgn(config: &TournamentConfig, pgn: &str) {
    let Some(path) = &config.pgn_output else {
        return;
    };
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut file) => {
            if let Err(err) = writeln!(file, "{pgn}") {
                tracing::warn!(target: "scheduler", "failed to append PGN: {err}");
            }
        }
        Err(err) => tracing::warn!(target: "scheduler", "failed to open PGN file: {err}"),
    }
}

fn push_error(errors: &mut Vec<String>, message: String) {
    errors.push(message);
    if errors.len() > MAX_RECENT_ERRORS {
        let overflow = errors.len() - MAX_RECENT_ERRORS;
        errors.drain(0..overflow);
    }
}

fn today_pgn_date() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}.{:02}.{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

#[allow(clippy::too_many_arguments)]
fn publish(
    snapshot: &Arc<Mutex<TournamentSnapshot>>,
    standings: &Standings,
    elo: &IncrementalElo,
    ids: &[EngineId],
    finished: usize,
    total: usize,
    status: TournamentStatus,
    recent_errors: &[String],
) {
    let elo_map = ids
        .iter()
        .map(|id| {
            (
                *id,
                EloEntry {
                    current: elo.current(*id),
                    delta: elo.delta_since_start(*id),
                },
            )
        })
        .collect();
    if let Ok(mut snap) = snapshot.lock() {
        snap.status = status;
        snap.standings = standings.clone();
        snap.elo = elo_map;
        snap.games_finished = finished;
        snap.games_total = total;
        snap.recent_errors = recent_errors.to_vec();
    }
}

fn publish_status(snapshot: &Arc<Mutex<TournamentSnapshot>>, status: TournamentStatus) {
    if let Ok(mut snap) = snapshot.lock() {
        snap.status = status;
    }
}
