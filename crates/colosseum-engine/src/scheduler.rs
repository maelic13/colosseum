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
    Pairing, Rating, Standings, StartPosition, TournamentConfig, TournamentEvent, TournamentId,
    generate_schedule, standings::GameOutcome,
};
use colosseum_uci::SpawnOptions;

use crate::error::EngineError;
use crate::openings::{ResolvedOpening, load_openings};
use crate::runner::{EngineGameSpec, GameSpec, run_game};
use crate::store::{self, Store, TournamentRow};

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
    /// Wall-clock time when the first game was launched (None before any game starts).
    pub started_at: Option<std::time::Instant>,
    /// Sum of `duration_ms` across all finished games with a measured duration.
    pub total_game_ms: u64,
    /// Count of finished games that have a measured duration (denominator for avg).
    pub games_timed: usize,
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

/// A scheduled game: id, pairing, and the opening assigned to it.
#[derive(Clone)]
struct ScheduledGame {
    game_id: GameId,
    pairing: Pairing,
    /// Opening start FEN (`None` = standard start position).
    start_fen: Option<String>,
    /// Opening moves (UCI) to pre-play before the engines move.
    opening_moves: Vec<String>,
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
) -> Result<(Tournament, impl Future<Output = ()> + use<>), EngineError> {
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
        store.add_tournament_engine(id, engine.id, engine, seed as u32, start_elo)?;
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

    let seeds: Vec<(EngineId, f64)> = ids
        .iter()
        .map(|eid| (*eid, templates[eid].start_elo))
        .collect();
    let init_standings = Standings::with_engines(&ids);
    let init_elo = IncrementalElo::with_seed(config.k_factor, seeds.iter().copied());

    // Resolve the opening book once (if any). One opening is drawn per *encounter*
    // (a run of `games_per_pair` consecutive games), so both colours are played
    // from the same position; the book cycles if there are more encounters than
    // openings.
    let openings: Vec<ResolvedOpening> = match &config.start_position {
        StartPosition::Startpos => Vec::new(),
        StartPosition::Book(book) => load_openings(book)?,
    };
    let games_per_pair = config.games_per_pair.max(1) as usize;

    // Build and persist the schedule.
    let pairings = generate_schedule(&ids, &config);
    let mut schedule = Vec::with_capacity(pairings.len());
    for (i, pairing) in pairings.into_iter().enumerate() {
        let game_id = GameId::new();
        let (start_fen, opening_moves) = if openings.is_empty() {
            (None, Vec::new())
        } else {
            let opening = &openings[(i / games_per_pair) % openings.len()];
            (opening.start_fen.clone(), opening.moves.clone())
        };
        store.insert_pending_game(
            game_id,
            id,
            pairing.round,
            pairing.white,
            pairing.black,
            start_fen.as_deref(),
            &opening_moves,
        )?;
        schedule.push(ScheduledGame {
            game_id,
            pairing,
            start_fen,
            opening_moves,
        });
    }

    let total_games = schedule.len();
    let elo_snapshot: HashMap<EngineId, EloEntry> = ids
        .iter()
        .map(|eid| {
            (
                *eid,
                EloEntry {
                    current: templates[eid].start_elo,
                    delta: 0.0,
                },
            )
        })
        .collect();
    let snapshot = Arc::new(Mutex::new(TournamentSnapshot {
        status: TournamentStatus::Idle,
        standings: init_standings.clone(),
        elo: elo_snapshot,
        games_finished: 0,
        games_total: total_games,
        recent_errors: Vec::new(),
        started_at: None,
        total_game_ms: 0,
        games_timed: 0,
    }));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let driver = drive(Driver {
        id,
        event_name: name.to_string(),
        config,
        templates,
        ids,
        schedule,
        total_games,
        store,
        events,
        snapshot: Arc::clone(&snapshot),
        commands: cmd_rx,
        seeds,
        init_standings,
        init_elo,
        init_finished: Vec::new(),
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
    /// Games remaining to be played.  For fresh tournaments this is the full
    /// schedule; for resumed tournaments it contains only the unfinished games.
    schedule: Vec<ScheduledGame>,
    /// Total games in the full tournament schedule, including already-finished
    /// games on resume (used for `games_total` in snapshots).
    total_games: usize,
    store: Store,
    events: Sender<TournamentEvent>,
    snapshot: Arc<Mutex<TournamentSnapshot>>,
    commands: UnboundedReceiver<Command>,
    /// Original start Elos for `EndOfTournament` recompute.
    seeds: Vec<(EngineId, f64)>,
    /// Pre-initialized standings (empty for fresh tournaments, replayed from
    /// the database for resumed ones).
    init_standings: Standings,
    /// Pre-initialized Elo model (seeded for fresh, replayed for resumed).
    init_elo: IncrementalElo,
    /// Pre-completed game results (empty for fresh, filled for resumed).
    init_finished: Vec<(EngineId, EngineId, GameResult)>,
}

async fn drive(mut driver: Driver) {
    let total = driver.total_games;
    let concurrency = driver.config.concurrency.max(1);
    // Take the init state out of `driver` using `mem::take`/`replace` so the
    // struct remains fully initialized and `&driver` borrows remain valid later.
    let seeds = std::mem::take(&mut driver.seeds);
    let mut elo = std::mem::replace(&mut driver.init_elo, IncrementalElo::new(0.0));
    let mut standings = std::mem::replace(&mut driver.init_standings, Standings::with_engines(&[]));
    let mut finished_results = std::mem::take(&mut driver.init_finished);
    let mut recent_errors: Vec<String> = Vec::new();

    let mut games: JoinSet<crate::runner::GameReport> = JoinSet::new();
    let mut in_flight: HashSet<GameId> = HashSet::new();
    let mut next_index = 0usize;
    let mut running = false;
    let mut finished = false;
    let mut tournament_started_at: Option<std::time::Instant> = None;
    let mut total_game_ms: u64 = 0;
    let mut games_timed: usize = 0;

    loop {
        // Launch while running, with spare capacity and pending games.
        while running && games.len() < concurrency && next_index < driver.schedule.len() {
            if tournament_started_at.is_none() {
                tournament_started_at = Some(std::time::Instant::now());
            }
            let scheduled = driver.schedule[next_index].clone();
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
                tournament_started_at,
                total_game_ms,
                games_timed,
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
                        if let Some(dur_ms) = report.stats.duration_ms {
                            total_game_ms += dur_ms;
                            games_timed += 1;
                        }
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
                            tournament_started_at,
                            total_game_ms,
                            games_timed,
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

/// Resume an existing tournament from persisted database state.
///
/// Loads the engine config snapshots, replays finished games into standings and
/// Elo, resets any `running` or `discarded` games to `pending` (so they will
/// be replayed), then hands back the same `(Tournament, driver-future)` pair as
/// [`create_tournament`].  Call [`Tournament::go`] to start (re)playing.
///
/// Returns an error if the tournament has fewer than two participants in the
/// database, or if any participant is missing its stored config snapshot.
pub fn resume_tournament(
    row: TournamentRow,
    store: Store,
    events: Sender<TournamentEvent>,
) -> Result<(Tournament, impl Future<Output = ()> + use<>), EngineError> {
    let id = row.id;
    let config = row.config;

    // Load participants sorted by their original insertion seed.
    let participants = store.list_tournament_engines(id)?;
    if participants.len() < 2 {
        return Err(EngineError::Corrupt(
            "tournament has fewer than 2 engines in the database".into(),
        ));
    }

    // Rebuild engine templates from the stored config snapshots.
    let mut templates: HashMap<EngineId, EngineTemplate> = HashMap::new();
    let mut ids: Vec<EngineId> = Vec::with_capacity(participants.len());
    let mut seeds: Vec<(EngineId, f64)> = Vec::with_capacity(participants.len());

    for p in &participants {
        let engine = p.config.as_ref().ok_or_else(|| {
            EngineError::Corrupt(format!(
                "no engine config snapshot stored for participant {:?}",
                p.engine
            ))
        })?;
        seeds.push((p.engine, p.start_elo));
        templates.insert(
            p.engine,
            EngineTemplate {
                id: p.engine,
                name: display_name(engine),
                spawn: SpawnOptions {
                    path: engine.path.clone(),
                    args: engine.args.clone(),
                    working_dir: engine.working_dir.clone(),
                    env: engine.env.clone().into_iter().collect(),
                },
                options: resolve_options(engine, &config.common),
                start_elo: p.start_elo,
            },
        );
        ids.push(p.engine);
    }

    // Load all games for this tournament (ORDER BY round, rowid = original order).
    let all_games = store.list_games(id)?;
    let total_games = all_games.len();

    // Reset in-flight (running) and force-stopped (discarded) games to pending
    // so they will be replayed in this session.
    for game in &all_games {
        if game.status == store::GAME_RUNNING || game.status == store::GAME_DISCARDED {
            store.reset_game_to_pending(game.id)?;
        }
    }

    // Replay finished games to reconstruct standings and Elo.
    let mut elo = IncrementalElo::with_seed(config.k_factor, seeds.iter().copied());
    let mut standings = Standings::with_engines(&ids);
    let mut finished_results: Vec<(EngineId, EngineId, GameResult)> = Vec::new();

    // Build remaining schedule from non-finished games (preserving original DB order).
    let mut schedule: Vec<ScheduledGame> = Vec::new();

    for game in &all_games {
        if game.status == store::GAME_FINISHED {
            if let Some(result) = game.result {
                standings.record(GameOutcome {
                    white: game.white,
                    black: game.black,
                    result,
                    white_nps: game.white_nps,
                    black_nps: game.black_nps,
                });
                finished_results.push((game.white, game.black, result));
                if config.elo_policy == EloPolicy::PerGame {
                    elo.update(game.white, game.black, result);
                }
            }
        } else {
            schedule.push(ScheduledGame {
                game_id: game.id,
                pairing: Pairing {
                    round: game.round,
                    white: game.white,
                    black: game.black,
                },
                start_fen: game.start_fen.clone(),
                opening_moves: game.opening_moves.clone(),
            });
        }
    }

    // Mark the tournament as stopped; it becomes running again when Go is pressed.
    store.set_tournament_status(id, store::STATUS_STOPPED)?;

    let elo_snapshot: HashMap<EngineId, EloEntry> = ids
        .iter()
        .map(|eid| {
            (
                *eid,
                EloEntry {
                    current: elo.current(*eid),
                    delta: elo.delta_since_start(*eid),
                },
            )
        })
        .collect();

    let snapshot = Arc::new(Mutex::new(TournamentSnapshot {
        status: TournamentStatus::Stopped,
        standings: standings.clone(),
        elo: elo_snapshot,
        games_finished: finished_results.len(),
        games_total: total_games,
        recent_errors: Vec::new(),
        started_at: None,
        total_game_ms: 0,
        games_timed: 0,
    }));

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

    let driver_fut = drive(Driver {
        id,
        event_name: row.name,
        config,
        templates,
        ids,
        schedule,
        total_games,
        store,
        events,
        snapshot: Arc::clone(&snapshot),
        commands: cmd_rx,
        seeds,
        init_standings: standings,
        init_elo: elo,
        init_finished: finished_results,
    });

    Ok((
        Tournament {
            id,
            commands: cmd_tx,
            snapshot,
        },
        driver_fut,
    ))
}

/// One participant's identity within a stored tournament summary.
#[derive(Debug, Clone)]
pub struct ResultParticipant {
    pub id: EngineId,
    pub name: String,
    pub version: String,
}

/// A read-only reconstruction of a stored tournament's outcome: final standings,
/// Elo, participant identities, and game counts. Built by replaying the finished
/// games in order — no engine processes are spawned. Backs the History tab.
#[derive(Debug, Clone)]
pub struct TournamentResults {
    pub standings: Standings,
    pub elo: HashMap<EngineId, EloEntry>,
    pub participants: Vec<ResultParticipant>,
    pub games_finished: usize,
    pub games_total: usize,
    /// Finished games with a decisive (non-draw) result.
    pub decisive: usize,
    /// Finished games drawn.
    pub draws: usize,
}

/// Reconstruct a stored tournament's results without spawning any engines.
///
/// Finished games are replayed in DB order to rebuild standings and Elo; the Elo
/// reconstruction matches the live end-of-tournament state for both `PerGame` and
/// `EndOfTournament` policies (and leaves ratings at their seeds for `Never`).
pub fn load_tournament_results(
    store: &Store,
    row: &TournamentRow,
) -> Result<TournamentResults, EngineError> {
    let id = row.id;
    let config = &row.config;

    let participants_raw = store.list_tournament_engines(id)?;
    let mut ids = Vec::with_capacity(participants_raw.len());
    let mut seeds = Vec::with_capacity(participants_raw.len());
    let mut participants = Vec::with_capacity(participants_raw.len());
    for p in &participants_raw {
        ids.push(p.engine);
        seeds.push((p.engine, p.start_elo));
        let (name, version) = match &p.config {
            Some(cfg) => (display_name(cfg), cfg.meta.version.clone()),
            None => (p.engine.to_string(), String::new()),
        };
        participants.push(ResultParticipant {
            id: p.engine,
            name,
            version,
        });
    }

    let all_games = store.list_games(id)?;
    let total_games = all_games.len();

    let mut elo = IncrementalElo::with_seed(config.k_factor, seeds.iter().copied());
    let mut standings = Standings::with_engines(&ids);
    let mut finished = 0usize;
    let mut decisive = 0usize;
    let mut draws = 0usize;

    for game in &all_games {
        if game.status != store::GAME_FINISHED {
            continue;
        }
        let Some(result) = game.result else { continue };
        standings.record(GameOutcome {
            white: game.white,
            black: game.black,
            result,
            white_nps: game.white_nps,
            black_nps: game.black_nps,
        });
        if config.elo_policy != EloPolicy::Never {
            elo.update(game.white, game.black, result);
        }
        finished += 1;
        if result == GameResult::Draw {
            draws += 1;
        } else {
            decisive += 1;
        }
    }

    let elo_map = ids
        .iter()
        .map(|eid| {
            (
                *eid,
                EloEntry {
                    current: elo.current(*eid),
                    delta: elo.delta_since_start(*eid),
                },
            )
        })
        .collect();

    Ok(TournamentResults {
        standings,
        elo: elo_map,
        participants,
        games_finished: finished,
        games_total: total_games,
        decisive,
        draws,
    })
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
        start_fen: scheduled.start_fen.clone(),
        opening_moves: scheduled.opening_moves.clone(),
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
    started_at: Option<std::time::Instant>,
    total_game_ms: u64,
    games_timed: usize,
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
        snap.started_at = started_at;
        snap.total_game_ms = total_game_ms;
        snap.games_timed = games_timed;
    }
}

fn publish_status(snapshot: &Arc<Mutex<TournamentSnapshot>>, status: TournamentStatus) {
    if let Ok(mut snap) = snapshot.lock() {
        snap.status = status;
    }
}
