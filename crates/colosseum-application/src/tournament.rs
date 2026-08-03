use std::collections::{HashMap, HashSet};

use colosseum_core::{
    EngineId, ExportRow, Format, GameOutcome, GameResult, ParticipantId, Standings, Termination,
    crosstable_csv, gauntlet, ml_ratings, ml_ratings_anchored, rating_error, round_robin,
    standings_csv,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::RuntimeParticipant;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentParticipant {
    pub participant: RuntimeParticipant,
    pub initial_rating: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentDesign {
    pub format: Format,
    pub games_per_pair: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentScheduleGame {
    pub number: u32,
    pub encounter: u32,
    pub game_in_encounter: u32,
    pub round: u32,
    pub white: ParticipantId,
    pub black: ParticipantId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentPlan {
    pub design: TournamentDesign,
    pub participants: Vec<TournamentParticipant>,
    pub schedule: Vec<TournamentScheduleGame>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentCompletedGame {
    pub number: u32,
    pub white: ParticipantId,
    pub black: ParticipantId,
    pub result: GameResult,
    pub scorable: bool,
    pub termination: Termination,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentStanding {
    pub rank: usize,
    pub participant: ParticipantId,
    pub name: String,
    pub initial_rating: f64,
    pub rating: f64,
    pub error_95: Option<f64>,
    pub anchored: bool,
    pub points: f64,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentResults {
    pub games_scheduled: usize,
    pub games_attempted: usize,
    pub games_scored: usize,
    pub anchor: Option<ParticipantId>,
    pub standings: Vec<TournamentStanding>,
    pub standings_csv: String,
    pub crosstable_csv: String,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TournamentPlanError {
    #[error("a tournament requires at least two distinct participants")]
    TooFewParticipants,
    #[error("tournament participant IDs must be unique")]
    DuplicateParticipant,
    #[error("cycles and games per pair must be positive")]
    EmptySchedule,
    #[error("gauntlet seeds must be positive and leave at least one opponent")]
    InvalidGauntletSeeds,
    #[error("initial ratings must be finite")]
    InvalidInitialRating,
    #[error("tournament schedule is too large")]
    ScheduleTooLarge,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TournamentResultError {
    #[error("rating anchor is not a tournament participant")]
    UnknownAnchor,
    #[error("completed game {0} is not in the tournament schedule")]
    UnknownGame(u32),
    #[error("completed game {0} has different participants from the schedule")]
    GameIdentityMismatch(u32),
    #[error("completed game {0} occurs more than once")]
    DuplicateGame(u32),
}

pub struct PlanTournament;

impl PlanTournament {
    pub fn execute(
        participants: Vec<TournamentParticipant>,
        design: TournamentDesign,
    ) -> Result<TournamentPlan, TournamentPlanError> {
        validate(&participants, design)?;
        let engine_ids = participants
            .iter()
            .map(|item| EngineId::from_uuid(item.participant.id.as_uuid()))
            .collect::<Vec<_>>();
        let by_engine = participants
            .iter()
            .map(|item| {
                (
                    EngineId::from_uuid(item.participant.id.as_uuid()),
                    item.participant.id,
                )
            })
            .collect::<HashMap<_, _>>();
        let pairings = match design.format {
            Format::RoundRobin { cycles } => {
                round_robin(&engine_ids, cycles, design.games_per_pair)
            }
            Format::Gauntlet { seeds, cycles } => {
                gauntlet(&engine_ids, seeds, cycles, design.games_per_pair)
            }
        };
        let mut schedule = Vec::with_capacity(pairings.len());
        for (index, pairing) in pairings.into_iter().enumerate() {
            let number =
                u32::try_from(index + 1).map_err(|_| TournamentPlanError::ScheduleTooLarge)?;
            let encounter = u32::try_from(index / design.games_per_pair as usize + 1)
                .map_err(|_| TournamentPlanError::ScheduleTooLarge)?;
            schedule.push(TournamentScheduleGame {
                number,
                encounter,
                game_in_encounter: index as u32 % design.games_per_pair + 1,
                round: pairing.round,
                white: by_engine[&pairing.white],
                black: by_engine[&pairing.black],
            });
        }
        if schedule.is_empty() {
            return Err(TournamentPlanError::EmptySchedule);
        }
        Ok(TournamentPlan {
            design,
            participants,
            schedule,
        })
    }
}

pub struct RateTournament;

impl RateTournament {
    pub fn execute(
        plan: &TournamentPlan,
        games: &[TournamentCompletedGame],
        anchor: Option<ParticipantId>,
    ) -> Result<TournamentResults, TournamentResultError> {
        if anchor.is_some_and(|id| {
            !plan
                .participants
                .iter()
                .any(|participant| participant.participant.id == id)
        }) {
            return Err(TournamentResultError::UnknownAnchor);
        }
        let schedule = plan
            .schedule
            .iter()
            .map(|game| (game.number, game))
            .collect::<HashMap<_, _>>();
        let mut seen = HashSet::new();
        for game in games {
            let Some(planned) = schedule.get(&game.number) else {
                return Err(TournamentResultError::UnknownGame(game.number));
            };
            if !seen.insert(game.number) {
                return Err(TournamentResultError::DuplicateGame(game.number));
            }
            if planned.white != game.white || planned.black != game.black {
                return Err(TournamentResultError::GameIdentityMismatch(game.number));
            }
        }

        let ids = plan
            .participants
            .iter()
            .map(|participant| engine_id(participant.participant.id))
            .collect::<Vec<_>>();
        let mut aggregate = Standings::with_engines(&ids);
        for game in games.iter().filter(|game| game.scorable) {
            aggregate.record(GameOutcome {
                white: engine_id(game.white),
                black: engine_id(game.black),
                result: game.result,
                termination: game.termination,
                white_nps: None,
                black_nps: None,
                white_depth: None,
                black_depth: None,
                white_move_ms: None,
                black_move_ms: None,
            });
        }
        let priors = plan
            .participants
            .iter()
            .map(|participant| {
                (
                    engine_id(participant.participant.id),
                    participant.initial_rating,
                )
            })
            .collect::<Vec<_>>();
        let ratings = if let Some(anchor) = anchor {
            let updatable = ids
                .iter()
                .copied()
                .filter(|id| *id != engine_id(anchor))
                .collect::<Vec<_>>();
            ml_ratings_anchored(&aggregate, &priors, &updatable)
        } else {
            ml_ratings(&aggregate, &priors)
        };
        let by_id = plan
            .participants
            .iter()
            .map(|participant| (participant.participant.id, participant))
            .collect::<HashMap<_, _>>();
        let mut order = ids.clone();
        order.sort_by(|left, right| {
            aggregate
                .standing(*right)
                .points()
                .total_cmp(&aggregate.standing(*left).points())
                .then_with(|| left.as_uuid().cmp(&right.as_uuid()))
        });
        let standings = order
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let participant_id = ParticipantId::from_uuid(id.as_uuid());
                let participant = by_id[&participant_id];
                let standing = aggregate.standing(*id);
                TournamentStanding {
                    rank: index + 1,
                    participant: participant_id,
                    name: participant_name(&participant.participant.launch),
                    initial_rating: participant.initial_rating,
                    rating: ratings[id],
                    error_95: rating_error(&aggregate, &ratings, *id),
                    anchored: anchor == Some(participant_id),
                    points: standing.points(),
                    games: standing.games(),
                    wins: standing.wins,
                    draws: standing.draws,
                    losses: standing.losses,
                }
            })
            .collect::<Vec<_>>();
        let export_rows = standings
            .iter()
            .map(|row| ExportRow {
                rank: row.rank,
                name: row.name.clone(),
                version: String::new(),
                elo: row.rating,
                elo_delta: (!row.anchored).then_some(row.rating - row.initial_rating),
                points: row.points,
                games: row.games,
                wins: row.wins,
                draws: row.draws,
                losses: row.losses,
                nps: None,
            })
            .collect::<Vec<_>>();
        let cross_order = order
            .iter()
            .map(|id| {
                let participant = by_id[&ParticipantId::from_uuid(id.as_uuid())];
                (*id, participant_name(&participant.participant.launch))
            })
            .collect::<Vec<_>>();
        Ok(TournamentResults {
            games_scheduled: plan.schedule.len(),
            games_attempted: games.len(),
            games_scored: games.iter().filter(|game| game.scorable).count(),
            anchor,
            standings,
            standings_csv: standings_csv(&export_rows),
            crosstable_csv: crosstable_csv(&cross_order, &aggregate),
        })
    }
}

fn engine_id(participant: ParticipantId) -> EngineId {
    EngineId::from_uuid(participant.as_uuid())
}

fn participant_name(launch: &crate::EngineLaunchSpec) -> String {
    launch.label.clone().unwrap_or_else(|| {
        launch
            .executable
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("engine")
            .to_owned()
    })
}

fn validate(
    participants: &[TournamentParticipant],
    design: TournamentDesign,
) -> Result<(), TournamentPlanError> {
    if participants.len() < 2 {
        return Err(TournamentPlanError::TooFewParticipants);
    }
    if participants
        .iter()
        .map(|item| item.participant.id)
        .collect::<HashSet<_>>()
        .len()
        != participants.len()
    {
        return Err(TournamentPlanError::DuplicateParticipant);
    }
    if participants
        .iter()
        .any(|participant| !participant.initial_rating.is_finite())
    {
        return Err(TournamentPlanError::InvalidInitialRating);
    }
    if design.games_per_pair == 0 {
        return Err(TournamentPlanError::EmptySchedule);
    }
    match design.format {
        Format::RoundRobin { cycles: 0 } => Err(TournamentPlanError::EmptySchedule),
        Format::Gauntlet { seeds, cycles }
            if cycles == 0 || seeds == 0 || seeds as usize >= participants.len() =>
        {
            Err(TournamentPlanError::InvalidGauntletSeeds)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineLaunchSpec;

    fn participants(count: usize) -> Vec<TournamentParticipant> {
        (0..count)
            .map(|index| TournamentParticipant {
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128(index as u128 + 1),
                    launch: EngineLaunchSpec::path_only(format!("engine-{index}").into()),
                },
                initial_rating: 1_500.0,
            })
            .collect()
    }

    #[test]
    fn round_robin_is_the_shared_core_circle_schedule_with_stable_identity() {
        let participants = participants(3);
        let plan = PlanTournament::execute(
            participants.clone(),
            TournamentDesign {
                format: Format::RoundRobin { cycles: 2 },
                games_per_pair: 2,
            },
        )
        .unwrap();
        assert_eq!(plan.schedule.len(), 12);
        assert_eq!(
            plan.schedule.iter().map(|game| game.encounter).max(),
            Some(6)
        );
        assert!(
            plan.schedule.chunks_exact(2).all(|games| {
                games[0].white == games[1].black && games[0].black == games[1].white
            })
        );
        let core = round_robin(
            &participants
                .iter()
                .map(|item| EngineId::from_uuid(item.participant.id.as_uuid()))
                .collect::<Vec<_>>(),
            2,
            2,
        );
        assert!(plan.schedule.iter().zip(core).all(|(planned, core)| {
            planned.round == core.round
                && planned.white.as_uuid() == core.white.as_uuid()
                && planned.black.as_uuid() == core.black.as_uuid()
        }));
    }

    #[test]
    fn multi_seed_gauntlet_never_pairs_seeds_or_opponents_together() {
        let plan = PlanTournament::execute(
            participants(5),
            TournamentDesign {
                format: Format::Gauntlet {
                    seeds: 2,
                    cycles: 2,
                },
                games_per_pair: 2,
            },
        )
        .unwrap();
        assert_eq!(plan.schedule.len(), 24);
        assert!(plan.schedule.iter().all(|game| {
            let white_seed = game.white == ParticipantId::from_u128(1)
                || game.white == ParticipantId::from_u128(2);
            let black_seed = game.black == ParticipantId::from_u128(1)
                || game.black == ParticipantId::from_u128(2);
            white_seed ^ black_seed
        }));
    }

    #[test]
    fn invalid_designs_fail_before_scheduling() {
        assert_eq!(
            PlanTournament::execute(
                participants(2),
                TournamentDesign {
                    format: Format::Gauntlet {
                        seeds: 2,
                        cycles: 1,
                    },
                    games_per_pair: 2,
                },
            ),
            Err(TournamentPlanError::InvalidGauntletSeeds)
        );
    }

    #[test]
    fn ratings_and_exports_are_joint_order_independent_results() {
        let plan = PlanTournament::execute(
            participants(3),
            TournamentDesign {
                format: Format::RoundRobin { cycles: 1 },
                games_per_pair: 2,
            },
        )
        .unwrap();
        let games = plan
            .schedule
            .iter()
            .map(|scheduled| TournamentCompletedGame {
                number: scheduled.number,
                white: scheduled.white,
                black: scheduled.black,
                result: if scheduled.white == ParticipantId::from_u128(1) {
                    GameResult::WhiteWin
                } else if scheduled.black == ParticipantId::from_u128(1) {
                    GameResult::BlackWin
                } else {
                    GameResult::Draw
                },
                scorable: true,
                termination: Termination::Checkmate,
            })
            .collect::<Vec<_>>();
        let mut reversed = games.clone();
        reversed.reverse();
        let first = RateTournament::execute(&plan, &games, None).unwrap();
        let second = RateTournament::execute(&plan, &reversed, None).unwrap();
        assert_eq!(first.standings, second.standings);
        assert_eq!(first.standings[0].participant, ParticipantId::from_u128(1));
        assert!(first.standings[0].rating > first.standings[1].rating);
        assert!(first.standings.iter().all(|row| row.error_95.is_some()));
        assert!(first.standings_csv.starts_with("Rank,Engine,Version,Elo"));
        assert!(first.crosstable_csv.contains("engine-0"));
    }

    #[test]
    fn optional_anchor_stays_fixed_and_checkpoint_identity_is_validated() {
        let plan = PlanTournament::execute(
            participants(2),
            TournamentDesign {
                format: Format::RoundRobin { cycles: 1 },
                games_per_pair: 2,
            },
        )
        .unwrap();
        let game = TournamentCompletedGame {
            number: 1,
            white: plan.schedule[0].white,
            black: plan.schedule[0].black,
            result: GameResult::WhiteWin,
            scorable: true,
            termination: Termination::Checkmate,
        };
        let anchor = ParticipantId::from_u128(2);
        let report =
            RateTournament::execute(&plan, std::slice::from_ref(&game), Some(anchor)).unwrap();
        let anchored = report
            .standings
            .iter()
            .find(|row| row.participant == anchor)
            .unwrap();
        assert_eq!(anchored.rating, 1_500.0);
        assert!(anchored.anchored);

        let mut invalid = game;
        invalid.white = ParticipantId::from_u128(99);
        assert_eq!(
            RateTournament::execute(&plan, &[invalid], None),
            Err(TournamentResultError::GameIdentityMismatch(1))
        );
    }
}
