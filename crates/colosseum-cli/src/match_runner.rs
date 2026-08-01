//! Fixed-length direct-engine match orchestration for the CLI.
//!
//! This deliberately runs a finite number of games without sequential
//! statistics or stopping logic. Pair-atomic scheduling, configurable clocks,
//! openings, persistence and fault policy are later phase responsibilities.

use std::path::Path;
use std::time::Duration;

use colosseum_application::{CpuAllocation, EngineLaunchSpec};
use colosseum_core::{AdjudicationConfig, EngineId, GameId, GameResult, Termination, TimeControl};
use colosseum_engine::{ClockAccountingReport, EngineGameSpec, GameSpec, LiveGameState, run_game};
use colosseum_uci::SpawnOptions;
use serde::Serialize;
use thiserror::Error;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

pub const DEFAULT_BASE_MS: u64 = 3_000;
pub const DEFAULT_INCREMENT_MS: u64 = 30;
pub const DEFAULT_MARGIN_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConfiguredTimeControl {
    pub control: TimeControl,
    pub margin_ms: u64,
}

impl Default for ConfiguredTimeControl {
    fn default() -> Self {
        Self {
            control: TimeControl::Increment {
                base_ms: DEFAULT_BASE_MS,
                inc_ms: DEFAULT_INCREMENT_MS,
            },
            margin_ms: DEFAULT_MARGIN_MS,
        }
    }
}

impl ConfiguredTimeControl {
    fn label(self) -> String {
        match self.control {
            TimeControl::PerMove { ms } => format!("movetime/{ms}ms"),
            TimeControl::SuddenDeath { base_ms } => format!("{base_ms}ms"),
            TimeControl::Increment { base_ms, inc_ms } => {
                format!("{base_ms}ms+{inc_ms}ms")
            }
            TimeControl::Nodes { nodes } => format!("nodes/{nodes}"),
            TimeControl::Depth { depth } => format!("depth/{depth}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchSide {
    A,
    B,
}

impl MatchSide {
    fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchScore {
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchGame {
    pub number: u32,
    pub white: MatchSide,
    pub result: GameResult,
    pub termination: Termination,
    pub clock_accounting: ClockAccountingReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixedMatchReport {
    pub games_requested: u32,
    pub games_completed: u32,
    pub engine_a: MatchScore,
    pub engine_b: MatchScore,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub games: Vec<MatchGame>,
}

#[derive(Debug, Error)]
pub enum MatchError {
    #[error("a fixed match needs at least one game")]
    ZeroGames,
    #[error("{side} requests CPU placement, which is not available for direct matches yet")]
    CpuPlacementUnavailable { side: &'static str },
}

/// Run exactly `games` sequential games, alternating colours by game number.
/// The same executable path is valid for both sides because side identity is
/// the resolved launch specification, not the path.
pub async fn run_fixed_match(
    engine_a: EngineLaunchSpec,
    engine_b: EngineLaunchSpec,
    games: u32,
    engine_a_time_control: ConfiguredTimeControl,
    engine_b_time_control: ConfiguredTimeControl,
    adjudication: AdjudicationConfig,
) -> Result<FixedMatchReport, MatchError> {
    if games == 0 {
        return Err(MatchError::ZeroGames);
    }
    let engine_a = engine_spec(engine_a, MatchSide::A, EngineId::from_u128(1))?;
    let engine_b = engine_spec(engine_b, MatchSide::B, EngineId::from_u128(2))?;
    let mut report = FixedMatchReport {
        games_requested: games,
        games_completed: 0,
        engine_a: MatchScore {
            name: engine_a.name.clone(),
            wins: 0,
            losses: 0,
            draws: 0,
        },
        engine_b: MatchScore {
            name: engine_b.name.clone(),
            wins: 0,
            losses: 0,
            draws: 0,
        },
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        games: Vec::with_capacity(games as usize),
    };

    for number in 1..=games {
        let a_is_white = number % 2 == 1;
        let white_side = if a_is_white {
            MatchSide::A
        } else {
            MatchSide::B
        };
        let (white, black, white_time_control, black_time_control) = if a_is_white {
            (
                engine_a.clone(),
                engine_b.clone(),
                engine_a_time_control,
                engine_b_time_control,
            )
        } else {
            (
                engine_b.clone(),
                engine_a.clone(),
                engine_b_time_control,
                engine_a_time_control,
            )
        };
        let game_id = GameId::from_u128(u128::from(number) + 100);
        let spec = GameSpec {
            game_id,
            event: "Colosseum CLI fixed match".into(),
            site: "?".into(),
            date: "????.??.??".into(),
            round: number,
            white: white.clone(),
            black: black.clone(),
            start_fen: None,
            opening_moves: Vec::new(),
            white_time_control: white_time_control.control,
            black_time_control: black_time_control.control,
            time_control_label: format!(
                "white {}; black {}",
                white_time_control.label(),
                black_time_control.label()
            ),
            adjudication,
            ponder: false,
            white_time_margin: Duration::from_millis(white_time_control.margin_ms),
            black_time_margin: Duration::from_millis(black_time_control.margin_ms),
            handshake_timeout: HANDSHAKE_TIMEOUT,
        };
        let live = LiveGameState::new_handle(
            game_id,
            number,
            (white.id, white.name.clone()),
            (black.id, black.name.clone()),
            None,
            white_time_control.control,
        );
        let game = run_game(spec, live).await;
        record_score(&mut report, white_side, game.result);
        report.games.push(MatchGame {
            number,
            white: white_side,
            result: game.result,
            termination: game.termination,
            clock_accounting: game.clock_accounting,
            error: game.error,
        });
        report.games_completed += 1;
    }
    Ok(report)
}

fn engine_spec(
    launch: EngineLaunchSpec,
    side: MatchSide,
    id: EngineId,
) -> Result<EngineGameSpec, MatchError> {
    if !matches!(launch.allocated_cpus, CpuAllocation::Unrestricted) {
        return Err(MatchError::CpuPlacementUnavailable {
            side: match side {
                MatchSide::A => "engine A",
                MatchSide::B => "engine B",
            },
        });
    }
    let name = launch
        .label
        .clone()
        .unwrap_or_else(|| display_name(&launch.executable));
    Ok(EngineGameSpec {
        id,
        name,
        spawn: SpawnOptions {
            path: launch.executable,
            args: launch.arguments,
            working_dir: launch.working_directory,
            env: launch.environment,
        },
        options: launch
            .options
            .into_iter()
            .map(|(name, value)| (name, value.command_value()))
            .collect(),
    })
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("engine")
        .to_owned()
}

fn record_score(report: &mut FixedMatchReport, white: MatchSide, result: GameResult) {
    let winner = match result {
        GameResult::WhiteWin => Some(white),
        GameResult::BlackWin => Some(white.other()),
        GameResult::Draw => None,
    };
    match winner {
        Some(MatchSide::A) => {
            report.engine_a.wins += 1;
            report.engine_b.losses += 1;
        }
        Some(MatchSide::B) => {
            report.engine_b.wins += 1;
            report.engine_a.losses += 1;
        }
        None => {
            report.engine_a.draws += 1;
            report.engine_b.draws += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_are_from_the_named_side_not_the_current_colour() {
        let mut report = FixedMatchReport {
            games_requested: 2,
            games_completed: 0,
            engine_a: MatchScore {
                name: "A".into(),
                wins: 0,
                losses: 0,
                draws: 0,
            },
            engine_b: MatchScore {
                name: "B".into(),
                wins: 0,
                losses: 0,
                draws: 0,
            },
            engine_a_time_control: ConfiguredTimeControl::default(),
            engine_b_time_control: ConfiguredTimeControl::default(),
            adjudication: AdjudicationConfig::default(),
            games: Vec::new(),
        };
        record_score(&mut report, MatchSide::A, GameResult::WhiteWin);
        record_score(&mut report, MatchSide::B, GameResult::WhiteWin);
        record_score(&mut report, MatchSide::A, GameResult::Draw);
        assert_eq!((report.engine_a.wins, report.engine_a.losses), (1, 1));
        assert_eq!((report.engine_b.wins, report.engine_b.losses), (1, 1));
        assert_eq!((report.engine_a.draws, report.engine_b.draws), (1, 1));
    }

    #[test]
    fn cpu_requests_are_rejected_before_a_match_is_launched() {
        let launch = EngineLaunchSpec {
            allocated_cpus: CpuAllocation::Enforced(vec![0.into()]),
            ..EngineLaunchSpec::path_only("engine".into())
        };
        assert!(matches!(
            engine_spec(launch, MatchSide::A, EngineId::from_u128(1)),
            Err(MatchError::CpuPlacementUnavailable { side: "engine A" })
        ));
    }
}
