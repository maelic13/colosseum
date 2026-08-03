use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::collections::BTreeMap;

use colosseum_core::{NamedRng, rng::stream_names};

use crate::{
    ApplicationError, EngineSession, EngineSessionFactory, PortFuture, RuntimeParticipant,
    SearchLimit, SearchObservation, SearchRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpsRequest {
    pub nodes: u64,
    pub position: String,
    pub moves: Vec<String>,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsReport {
    pub requested_nodes: u64,
    pub reported_nodes: u64,
    pub harness_elapsed_ns: u64,
    pub authoritative_nps: f64,
    pub engine_reported_time_ms: Option<u64>,
    pub engine_reported_nps: Option<u64>,
    pub best_move: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NpsStatePolicy {
    Cold,
    Warm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpsExperimentParticipant {
    pub arm: String,
    pub build: String,
    pub participant: RuntimeParticipant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpsExperimentDesign {
    pub nodes: u64,
    pub positions: Vec<String>,
    pub repetitions: u32,
    pub warmup_repetitions: u32,
    pub deadline_ms: u64,
    pub state_policy: NpsStatePolicy,
    pub seed: u64,
    pub bootstrap_samples: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpsScheduleEntry {
    pub sequence: u64,
    pub warmup: bool,
    pub repetition: u32,
    pub position_index: usize,
    pub participant_index: usize,
    pub arm: String,
    pub build: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsMeasuredSample {
    pub schedule: NpsScheduleEntry,
    pub measurement: NpsReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsBuildSummary {
    pub build: String,
    pub samples: usize,
    pub median_nps: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsArmSummary {
    pub arm: String,
    pub samples: usize,
    pub median_nps: f64,
    pub best_of_nps: f64,
    pub median_ci95: [f64; 2],
    pub builds: Vec<NpsBuildSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsExperimentReport {
    pub design: NpsExperimentDesign,
    pub schedule: Vec<NpsScheduleEntry>,
    pub samples: Vec<NpsMeasuredSample>,
    pub arms: Vec<NpsArmSummary>,
    pub per_round_ratio_sd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NpsHashPolicy {
    FixedTotal,
    PerThread,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsScalingInput {
    pub threads: u32,
    pub pinned_physical_cores: u32,
    pub hash_mb: u64,
    pub median_nps: f64,
    pub core_classes: Vec<String>,
    pub numa_nodes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsScalingPoint {
    #[serde(flatten)]
    pub input: NpsScalingInput,
    pub speedup: f64,
    pub parallel_efficiency: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsScalingReport {
    pub hash_policy: NpsHashPolicy,
    pub points: Vec<NpsScalingPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NpsError {
    #[error("fixed-node workload must contain at least one node")]
    ZeroNodes,
    #[error("fixed-node deadline must be at least one millisecond")]
    ZeroDeadline,
    #[error("engine did not report info nodes; fixed work cannot be verified")]
    MissingReportedNodes,
    #[error("engine stopped at {reported} reported nodes before the requested {requested}")]
    IncompleteWork { requested: u64, reported: u64 },
    #[error("the monotonic harness interval had zero duration")]
    ZeroElapsed,
    #[error("NPS comparison requires exactly two non-empty arms")]
    InvalidArms,
    #[error("NPS comparison requires at least one position and one measured repetition")]
    EmptyExperiment,
    #[error("bootstrap sample count must be at least one")]
    ZeroBootstrapSamples,
    #[error("scaling sweep requires a one-thread baseline and positive unique thread counts")]
    InvalidScalingThreads,
    #[error("scaling point for {threads} threads is not pinned to {threads} physical cores")]
    ScalingCoreMismatch { threads: u32 },
    #[error("scaling NPS values must be finite and positive")]
    InvalidScalingNps,
}

pub fn summarize_nps_scaling(
    hash_policy: NpsHashPolicy,
    mut inputs: Vec<NpsScalingInput>,
) -> Result<NpsScalingReport, NpsError> {
    inputs.sort_by_key(|input| input.threads);
    if inputs.first().map(|input| input.threads) != Some(1)
        || inputs.iter().any(|input| input.threads == 0)
        || inputs
            .windows(2)
            .any(|pair| pair[0].threads == pair[1].threads)
    {
        return Err(NpsError::InvalidScalingThreads);
    }
    for input in &inputs {
        if input.pinned_physical_cores != input.threads {
            return Err(NpsError::ScalingCoreMismatch {
                threads: input.threads,
            });
        }
        if !input.median_nps.is_finite() || input.median_nps <= 0.0 {
            return Err(NpsError::InvalidScalingNps);
        }
    }
    let baseline = inputs[0].median_nps;
    let points = inputs
        .into_iter()
        .map(|input| {
            let speedup = input.median_nps / baseline;
            let parallel_efficiency = speedup / f64::from(input.threads);
            NpsScalingPoint {
                input,
                speedup,
                parallel_efficiency,
            }
        })
        .collect();
    Ok(NpsScalingReport {
        hash_policy,
        points,
    })
}

pub struct MeasureNps;

impl MeasureNps {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participant: &'a RuntimeParticipant,
        request: NpsRequest,
    ) -> PortFuture<'a, Result<NpsReport, ApplicationError>> {
        Box::pin(async move {
            validate_request(&request).map_err(domain_error)?;
            let mut session = sessions.open(participant).await?;
            let measured = async {
                session.inspect().await?;
                for (name, value) in &participant.launch.options {
                    let value = value.command_value();
                    session.set_option(name, value.as_deref()).await?;
                }
                session.is_ready().await?;
                session.new_game().await?;
                session.is_ready().await?;
                let requested_nodes = request.nodes;
                let observation = session
                    .search(SearchRequest {
                        position: request.position,
                        moves: request.moves,
                        limit: SearchLimit::Nodes(requested_nodes),
                        deadline_ms: request.deadline_ms,
                    })
                    .await?;
                report(requested_nodes, observation).map_err(domain_error)
            }
            .await;
            let shutdown = session.shutdown().await;
            match (measured, shutdown) {
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                (Ok(report), Ok(())) => Ok(report),
            }
        })
    }
}

pub struct CompareNps;

impl CompareNps {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participants: &'a [NpsExperimentParticipant],
        design: NpsExperimentDesign,
    ) -> PortFuture<'a, Result<NpsExperimentReport, ApplicationError>> {
        Box::pin(async move {
            validate_design(participants, &design).map_err(domain_error)?;
            let schedule = build_schedule(participants, &design).map_err(domain_error)?;
            let samples = match design.state_policy {
                NpsStatePolicy::Cold => {
                    run_cold(sessions, participants, &design, &schedule).await?
                }
                NpsStatePolicy::Warm => {
                    run_warm(sessions, participants, &design, &schedule).await?
                }
            };
            summarize(design, schedule, samples).map_err(domain_error)
        })
    }
}

async fn run_cold(
    factory: &dyn EngineSessionFactory,
    participants: &[NpsExperimentParticipant],
    design: &NpsExperimentDesign,
    schedule: &[NpsScheduleEntry],
) -> Result<Vec<NpsMeasuredSample>, ApplicationError> {
    let mut measured = Vec::new();
    for entry in schedule {
        let configured = &participants[entry.participant_index];
        let mut session = factory.open(&configured.participant).await?;
        let result = async {
            prepare_session(session.as_mut(), &configured.participant).await?;
            measure_session(session.as_mut(), design, entry).await
        }
        .await;
        let shutdown = session.shutdown().await;
        let measurement = match (result, shutdown) {
            (Err(error), _) | (Ok(_), Err(error)) => return Err(error),
            (Ok(measurement), Ok(())) => measurement,
        };
        if !entry.warmup {
            measured.push(NpsMeasuredSample {
                schedule: entry.clone(),
                measurement,
            });
        }
    }
    Ok(measured)
}

async fn run_warm(
    factory: &dyn EngineSessionFactory,
    participants: &[NpsExperimentParticipant],
    design: &NpsExperimentDesign,
    schedule: &[NpsScheduleEntry],
) -> Result<Vec<NpsMeasuredSample>, ApplicationError> {
    let mut open = Vec::<Box<dyn EngineSession>>::new();
    for configured in participants {
        let mut session = factory.open(&configured.participant).await?;
        if let Err(error) = prepare_session(session.as_mut(), &configured.participant).await {
            let _ = session.shutdown().await;
            shutdown_all(&mut open).await;
            return Err(error);
        }
        open.push(session);
    }
    let mut measured = Vec::new();
    let mut failure = None;
    for entry in schedule {
        match measure_session(open[entry.participant_index].as_mut(), design, entry).await {
            Ok(measurement) if !entry.warmup => measured.push(NpsMeasuredSample {
                schedule: entry.clone(),
                measurement,
            }),
            Ok(_) => {}
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    let shutdown_error = shutdown_all(&mut open).await;
    if let Some(error) = failure.or(shutdown_error) {
        Err(error)
    } else {
        Ok(measured)
    }
}

async fn shutdown_all(sessions: &mut Vec<Box<dyn EngineSession>>) -> Option<ApplicationError> {
    let mut first = None;
    while let Some(mut session) = sessions.pop() {
        if let Err(error) = session.shutdown().await
            && first.is_none()
        {
            first = Some(error);
        }
    }
    first
}

async fn prepare_session(
    session: &mut dyn EngineSession,
    participant: &RuntimeParticipant,
) -> Result<(), ApplicationError> {
    let inspection = session.inspect().await?;
    for (name, value) in &participant.launch.options {
        if !inspection
            .options
            .iter()
            .any(|schema| schema.name() == name)
        {
            return Err(ApplicationError::ConfigurationFault(format!(
                "UCI option {name:?} was not advertised by the engine"
            )));
        }
        let value = value.command_value();
        session.set_option(name, value.as_deref()).await?;
    }
    session.is_ready().await
}

async fn measure_session(
    session: &mut dyn EngineSession,
    design: &NpsExperimentDesign,
    entry: &NpsScheduleEntry,
) -> Result<NpsReport, ApplicationError> {
    session.new_game().await?;
    session.is_ready().await?;
    let observation = session
        .search(SearchRequest {
            position: design.positions[entry.position_index].clone(),
            moves: Vec::new(),
            limit: SearchLimit::Nodes(design.nodes),
            deadline_ms: design.deadline_ms,
        })
        .await?;
    report(design.nodes, observation).map_err(domain_error)
}

fn validate_design(
    participants: &[NpsExperimentParticipant],
    design: &NpsExperimentDesign,
) -> Result<(), NpsError> {
    let mut arms = participants
        .iter()
        .map(|item| item.arm.as_str())
        .collect::<Vec<_>>();
    arms.sort_unstable();
    arms.dedup();
    if arms.len() != 2
        || arms
            .iter()
            .any(|arm| !participants.iter().any(|item| item.arm == *arm))
    {
        return Err(NpsError::InvalidArms);
    }
    if design.positions.is_empty() || design.repetitions == 0 {
        return Err(NpsError::EmptyExperiment);
    }
    if design.bootstrap_samples == 0 {
        return Err(NpsError::ZeroBootstrapSamples);
    }
    validate_request(&NpsRequest {
        nodes: design.nodes,
        position: design.positions[0].clone(),
        moves: Vec::new(),
        deadline_ms: design.deadline_ms,
    })
}

fn build_schedule(
    participants: &[NpsExperimentParticipant],
    design: &NpsExperimentDesign,
) -> Result<Vec<NpsScheduleEntry>, NpsError> {
    validate_design(participants, design)?;
    let mut arms = BTreeMap::<&str, Vec<usize>>::new();
    for (index, participant) in participants.iter().enumerate() {
        arms.entry(&participant.arm).or_default().push(index);
    }
    let arm_names = arms.keys().copied().collect::<Vec<_>>();
    let mut position_rng =
        NamedRng::new(design.seed, stream_names::POSITION_ORDER).expect("stable stream name");
    let mut warmup_rng =
        NamedRng::new(design.seed, stream_names::WARMUP_SCHEDULING).expect("stable stream name");
    let mut output = Vec::new();
    let total_repetitions = design.warmup_repetitions + design.repetitions;
    for repetition in 0..total_repetitions {
        let warmup = repetition < design.warmup_repetitions;
        let measured_repetition = repetition.saturating_sub(design.warmup_repetitions);
        let mut positions = (0..design.positions.len()).collect::<Vec<_>>();
        position_rng.shuffle(&mut positions);
        for position_index in positions {
            let mut pairs = arms[arm_names[0]]
                .iter()
                .flat_map(|left| arms[arm_names[1]].iter().map(move |right| (*left, *right)))
                .collect::<Vec<_>>();
            position_rng.shuffle(&mut pairs);
            let reverse = warmup_rng.rademacher() > 0;
            for (left, right) in pairs {
                for participant_index in if reverse {
                    [right, left]
                } else {
                    [left, right]
                } {
                    let participant = &participants[participant_index];
                    output.push(NpsScheduleEntry {
                        sequence: output.len() as u64,
                        warmup,
                        repetition: measured_repetition,
                        position_index,
                        participant_index,
                        arm: participant.arm.clone(),
                        build: participant.build.clone(),
                    });
                }
            }
        }
    }
    Ok(output)
}

fn summarize(
    design: NpsExperimentDesign,
    schedule: Vec<NpsScheduleEntry>,
    samples: Vec<NpsMeasuredSample>,
) -> Result<NpsExperimentReport, NpsError> {
    let mut by_arm = BTreeMap::<String, Vec<&NpsMeasuredSample>>::new();
    for sample in &samples {
        by_arm
            .entry(sample.schedule.arm.clone())
            .or_default()
            .push(sample);
    }
    let mut bootstrap =
        NamedRng::new(design.seed, stream_names::BOOTSTRAP_RESAMPLING).expect("stable stream name");
    let mut arms = Vec::new();
    for (arm, arm_samples) in &by_arm {
        let values = arm_samples
            .iter()
            .map(|sample| sample.measurement.authoritative_nps)
            .collect::<Vec<_>>();
        let mut by_build = BTreeMap::<String, Vec<f64>>::new();
        for sample in arm_samples {
            by_build
                .entry(sample.schedule.build.clone())
                .or_default()
                .push(sample.measurement.authoritative_nps);
        }
        let builds = by_build
            .into_iter()
            .map(|(build, values)| NpsBuildSummary {
                build,
                samples: values.len(),
                median_nps: median(&values),
            })
            .collect::<Vec<_>>();
        let best_of_nps = builds
            .iter()
            .map(|build| build.median_nps)
            .max_by(f64::total_cmp)
            .expect("each arm has measured samples");
        let mut medians = Vec::with_capacity(design.bootstrap_samples as usize);
        for _ in 0..design.bootstrap_samples {
            let indices = bootstrap
                .bootstrap_indices(values.len(), values.len())
                .expect("non-empty population");
            let resample = indices
                .into_iter()
                .map(|index| values[index])
                .collect::<Vec<_>>();
            medians.push(median(&resample));
        }
        medians.sort_by(f64::total_cmp);
        arms.push(NpsArmSummary {
            arm: arm.clone(),
            samples: values.len(),
            median_nps: median(&values),
            best_of_nps,
            median_ci95: percentile_interval(&medians),
            builds,
        });
    }
    let per_round_ratio_sd = round_ratio_sd(&samples);
    Ok(NpsExperimentReport {
        design,
        schedule,
        samples,
        arms,
        per_round_ratio_sd,
    })
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    if sorted.len().is_multiple_of(2) {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    }
}

fn percentile_interval(sorted: &[f64]) -> [f64; 2] {
    let last = sorted.len() - 1;
    [sorted[last * 25 / 1000], sorted[last * 975 / 1000]]
}

fn round_ratio_sd(samples: &[NpsMeasuredSample]) -> Option<f64> {
    let mut rounds = BTreeMap::<(u32, usize), BTreeMap<&str, Vec<f64>>>::new();
    for sample in samples {
        rounds
            .entry((sample.schedule.repetition, sample.schedule.position_index))
            .or_default()
            .entry(&sample.schedule.arm)
            .or_default()
            .push(sample.measurement.authoritative_nps);
    }
    let ratios = rounds
        .values()
        .filter_map(|arms| {
            let values = arms.values().collect::<Vec<_>>();
            (values.len() == 2).then(|| median(values[1]) / median(values[0]))
        })
        .collect::<Vec<_>>();
    if ratios.len() < 2 {
        return None;
    }
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    Some(
        (ratios
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / (ratios.len() - 1) as f64)
            .sqrt(),
    )
}

fn validate_request(request: &NpsRequest) -> Result<(), NpsError> {
    if request.nodes == 0 {
        return Err(NpsError::ZeroNodes);
    }
    if request.deadline_ms == 0 {
        return Err(NpsError::ZeroDeadline);
    }
    Ok(())
}

fn report(requested_nodes: u64, observation: SearchObservation) -> Result<NpsReport, NpsError> {
    let reported_nodes = observation
        .reported_nodes
        .ok_or(NpsError::MissingReportedNodes)?;
    if reported_nodes < requested_nodes {
        return Err(NpsError::IncompleteWork {
            requested: requested_nodes,
            reported: reported_nodes,
        });
    }
    if observation.harness_elapsed_ns == 0 {
        return Err(NpsError::ZeroElapsed);
    }
    let authoritative_nps =
        requested_nodes as f64 * 1_000_000_000.0 / observation.harness_elapsed_ns as f64;
    Ok(NpsReport {
        requested_nodes,
        reported_nodes,
        harness_elapsed_ns: observation.harness_elapsed_ns,
        authoritative_nps,
        engine_reported_time_ms: observation.reported_time_ms,
        engine_reported_nps: observation.reported_nps,
        best_move: observation.best_move,
    })
}

fn domain_error(error: NpsError) -> ApplicationError {
    ApplicationError::DomainError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpuAllocation, EngineLaunchSpec};
    use colosseum_core::ParticipantId;
    use std::path::PathBuf;

    fn observation(reported_nps: u64) -> SearchObservation {
        SearchObservation {
            best_move: "e2e4".into(),
            ponder: None,
            reported_nodes: Some(1_000_010),
            reported_time_ms: Some(1),
            reported_nps: Some(reported_nps),
            harness_elapsed_ns: 500_000_000,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn authoritative_speed_uses_requested_work_and_harness_time_only() {
        let low_claim = report(1_000_000, observation(1)).unwrap();
        let high_claim = report(1_000_000, observation(u64::MAX)).unwrap();
        assert_eq!(low_claim.authoritative_nps, 2_000_000.0);
        assert_eq!(low_claim.authoritative_nps, high_claim.authoritative_nps);
        assert_ne!(
            low_claim.engine_reported_nps,
            high_claim.engine_reported_nps
        );
    }

    #[test]
    fn reported_nodes_must_prove_the_fixed_work_completed() {
        let mut missing = observation(10);
        missing.reported_nodes = None;
        assert_eq!(
            report(1_000_000, missing),
            Err(NpsError::MissingReportedNodes)
        );

        let mut short = observation(10);
        short.reported_nodes = Some(999_999);
        assert_eq!(
            report(1_000_000, short),
            Err(NpsError::IncompleteWork {
                requested: 1_000_000,
                reported: 999_999,
            })
        );
    }

    fn experiment_participant(arm: &str, build: &str, id: u128) -> NpsExperimentParticipant {
        NpsExperimentParticipant {
            arm: arm.into(),
            build: build.into(),
            participant: RuntimeParticipant {
                id: ParticipantId::from_u128(id),
                launch: EngineLaunchSpec {
                    executable: PathBuf::from(build),
                    arguments: Vec::new(),
                    working_directory: None,
                    environment: BTreeMap::new(),
                    label: Some(build.into()),
                    options: BTreeMap::new(),
                    allocated_cpus: CpuAllocation::Unrestricted,
                },
            },
        }
    }

    fn design() -> NpsExperimentDesign {
        NpsExperimentDesign {
            nodes: 1_000,
            positions: vec!["startpos".into(), "8/8/8/8/8/8/K6k/8 w - - 0 1".into()],
            repetitions: 2,
            warmup_repetitions: 1,
            deadline_ms: 1_000,
            state_policy: NpsStatePolicy::Warm,
            seed: 42,
            bootstrap_samples: 100,
        }
    }

    #[test]
    fn comparison_schedule_is_seeded_strictly_alternating_and_build_balanced() {
        let participants = vec![
            experiment_participant("A", "a1", 1),
            experiment_participant("A", "a2", 2),
            experiment_participant("B", "b1", 3),
        ];
        let first = build_schedule(&participants, &design()).unwrap();
        let second = build_schedule(&participants, &design()).unwrap();
        assert_eq!(first, second);
        assert!(first.windows(2).all(|pair| {
            pair[0].arm != pair[1].arm
                || pair[0].position_index != pair[1].position_index
                || pair[0].repetition != pair[1].repetition
                || pair[0].warmup != pair[1].warmup
        }));
        assert_eq!(first.iter().filter(|entry| entry.warmup).count(), 8);
        assert_eq!(first.iter().filter(|entry| !entry.warmup).count(), 16);
    }

    #[test]
    fn summaries_use_medians_best_build_bootstrap_and_round_ratios() {
        let mut experiment = design();
        experiment.positions.truncate(1);
        experiment.warmup_repetitions = 0;
        let participants = vec![
            experiment_participant("A", "a", 1),
            experiment_participant("B", "b", 2),
        ];
        let schedule = build_schedule(&participants, &experiment).unwrap();
        let samples = schedule
            .iter()
            .cloned()
            .map(|schedule| {
                let authoritative_nps = match (schedule.arm.as_str(), schedule.repetition) {
                    ("A", 0) => 100.0,
                    ("A", 1) => 110.0,
                    ("B", 0) => 200.0,
                    ("B", 1) => 220.0,
                    _ => unreachable!(),
                };
                NpsMeasuredSample {
                    schedule,
                    measurement: NpsReport {
                        requested_nodes: 1_000,
                        reported_nodes: 1_000,
                        harness_elapsed_ns: 1,
                        authoritative_nps,
                        engine_reported_time_ms: None,
                        engine_reported_nps: None,
                        best_move: "e2e4".into(),
                    },
                }
            })
            .collect();
        let report = summarize(experiment, schedule, samples).unwrap();
        assert_eq!(report.arms[0].median_nps, 105.0);
        assert_eq!(report.arms[1].median_nps, 210.0);
        assert_eq!(report.arms[0].best_of_nps, 105.0);
        assert_eq!(report.per_round_ratio_sd, Some(0.0));
    }

    #[test]
    fn scaling_speedup_and_efficiency_match_hand_arithmetic() {
        let input = |threads, median_nps| NpsScalingInput {
            threads,
            pinned_physical_cores: threads,
            hash_mb: 128,
            median_nps,
            core_classes: vec!["performance".into()],
            numa_nodes: vec!["0:0".into()],
        };
        let report = summarize_nps_scaling(
            NpsHashPolicy::FixedTotal,
            vec![input(4, 300.0), input(1, 100.0), input(2, 180.0)],
        )
        .unwrap();
        assert_eq!(report.points[0].speedup, 1.0);
        assert_eq!(report.points[1].speedup, 1.8);
        assert_eq!(report.points[1].parallel_efficiency, 0.9);
        assert_eq!(report.points[2].speedup, 3.0);
        assert_eq!(report.points[2].parallel_efficiency, 0.75);
    }
}
