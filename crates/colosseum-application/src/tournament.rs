use std::collections::{HashMap, HashSet};

use colosseum_core::{EngineId, Format, ParticipantId, gauntlet, round_robin};
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
}
