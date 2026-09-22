//! The `nps` command: fixed-node speed, A/B comparison and thread scaling.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct NpsCommand {
    #[command(flatten)]
    pub(crate) engine: EngineArgs,
    /// Fixed number of nodes requested from the engine.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) nodes: u64,
    /// Primary executable for arm B; enables A/B comparison.
    #[arg(long)]
    pub(crate) against: Option<PathBuf>,
    /// Additional executable in arm A; repeat for pooled builds.
    #[arg(long = "a-build")]
    pub(crate) a_builds: Vec<PathBuf>,
    /// Additional executable in arm B; repeat for pooled builds.
    #[arg(long = "b-build")]
    pub(crate) b_builds: Vec<PathBuf>,
    /// Compare the primary executable with itself instead of using --against.
    #[arg(long, conflicts_with = "against")]
    pub(crate) self_pair: bool,
    /// UCI FEN to search; repeat for a suite. Omit to use startpos.
    #[arg(long)]
    pub(crate) positions: Vec<String>,
    /// Move following the selected position; repeat to provide a move list.
    #[arg(long = "move")]
    pub(crate) moves: Vec<String>,
    /// Measured repetitions of the complete position/build schedule.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) repetitions: u32,
    /// Unreported repetitions run before measurement.
    #[arg(long, default_value_t = 1)]
    pub(crate) warmup: u32,
    /// Restart per sample (cold) or retain each engine process (warm).
    #[arg(long, value_enum, default_value_t = NpsStateArg::Warm)]
    pub(crate) state: NpsStateArg,
    /// Master seed for position, pair and warm-up scheduling.
    #[arg(long, default_value_t = 0)]
    pub(crate) seed: u64,
    /// Bootstrap resamples used for each arm's median confidence interval.
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) bootstrap_samples: u32,
    /// Maximum absolute self-pair median difference before warning.
    #[arg(long, default_value_t = 0.5)]
    pub(crate) self_tolerance_percent: f64,
    /// Comma-separated search-thread counts; enables a pinned scaling sweep.
    #[arg(long, value_name = "1,2,4,8")]
    pub(crate) scale_threads: Option<String>,
    /// Advertised UCI spin option controlling engine search threads.
    #[arg(long, requires = "scale_threads")]
    pub(crate) threads_option: Option<String>,
    /// Hash allocation rule used by a scaling sweep.
    #[arg(long, value_enum, default_value_t = NpsHashPolicyArg::FixedTotal)]
    pub(crate) hash_policy: NpsHashPolicyArg,
    /// Hash MiB: total under fixed-total, or per thread under per-thread.
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) hash_mb: u64,
    /// Advertised UCI spin option controlling hash size.
    #[arg(long, default_value = "Hash")]
    pub(crate) hash_option: String,
    /// Maximum wall time allowed for the fixed-node search.
    #[arg(long, default_value_t = 60_000, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) deadline_ms: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum NpsStateArg {
    Cold,
    Warm,
}

impl From<NpsStateArg> for NpsStatePolicy {
    fn from(value: NpsStateArg) -> Self {
        match value {
            NpsStateArg::Cold => Self::Cold,
            NpsStateArg::Warm => Self::Warm,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum NpsHashPolicyArg {
    FixedTotal,
    PerThread,
}

impl From<NpsHashPolicyArg> for NpsHashPolicy {
    fn from(value: NpsHashPolicyArg) -> Self {
        match value {
            NpsHashPolicyArg::FixedTotal => Self::FixedTotal,
            NpsHashPolicyArg::PerThread => Self::PerThread,
        }
    }
}

pub(crate) async fn run_nps(command: NpsCommand, machine: bool, dry_run: bool) -> ExitCode {
    let base_launch = match command.engine.resolve() {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if !command.self_tolerance_percent.is_finite() || command.self_tolerance_percent < 0.0 {
        eprintln!("configuration error: --self-tolerance-percent must be finite and non-negative");
        return ExitCode::from(2);
    }
    if command.scale_threads.is_some() {
        return run_nps_scaling(command, base_launch, machine, dry_run).await;
    }
    let comparison = command.against.is_some()
        || command.self_pair
        || !command.a_builds.is_empty()
        || !command.b_builds.is_empty();
    if comparison && command.against.is_none() && !command.self_pair {
        eprintln!("configuration error: NPS comparison requires --against or --self-pair");
        return ExitCode::from(2);
    }
    if comparison && !command.moves.is_empty() {
        eprintln!("configuration error: --move is currently limited to single-sample nps");
        return ExitCode::from(2);
    }
    let positions = if command.positions.is_empty() {
        vec!["startpos".to_owned()]
    } else {
        command.positions.clone()
    };
    let design = NpsExperimentDesign {
        nodes: command.nodes,
        positions: positions.clone(),
        repetitions: command.repetitions,
        warmup_repetitions: command.warmup,
        deadline_ms: command.deadline_ms,
        state_policy: command.state.into(),
        seed: command.seed,
        bootstrap_samples: command.bootstrap_samples,
    };
    let participants = if comparison {
        nps_participants(&base_launch, &command)
    } else {
        vec![NpsExperimentParticipant {
            arm: "A".into(),
            build: nps_build_label(&base_launch.executable, "A", 1),
            participant: RuntimeParticipant {
                id: ParticipantId::from_u128(1),
                launch: base_launch,
            },
        }]
    };
    let mut path_pointers = Vec::new();
    for (index, item) in participants.iter().enumerate() {
        path_pointers.push(format!(
            "/participants/{index}/participant/launch/executable"
        ));
        if item.participant.launch.working_directory.is_some() {
            path_pointers.push(format!(
                "/participants/{index}/participant/launch/working_directory"
            ));
        }
    }
    let current_directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("configuration error: cannot read current directory: {error}");
            return ExitCode::from(2);
        }
    };
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "nps",
            "participants": participants,
            "design": design,
            "self_tolerance_percent": command.self_tolerance_percent
        }),
        &[],
        &current_directory,
        &path_pointers,
    ) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let participants: Vec<NpsExperimentParticipant> =
        serde_json::from_value(resolved.value()["participants"].clone())
            .expect("resolved NPS participants retain their schema");
    let design: NpsExperimentDesign = serde_json::from_value(resolved.value()["design"].clone())
        .expect("resolved NPS design retains its schema");
    if dry_run {
        let invocations = participants
            .iter()
            .map(|item| &item.participant.launch)
            .collect();
        print_output(
            &MachineOutput::DryRun {
                command: "nps",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations,
                wave_shape: None,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }
    if design.positions == ["startpos"] {
        eprintln!(
            "nps warning: startpos alone is a weak workload; use representative positions for decisions"
        );
    }
    if !comparison {
        let request = NpsRequest {
            nodes: design.nodes,
            position: design.positions[0].clone(),
            moves: command.moves,
            deadline_ms: design.deadline_ms,
        };
        return print_nps_result(
            MeasureNps::execute(
                &AffinityUciSessionFactory::new(apply_nps_affinity),
                &participants[0].participant,
                request,
            )
            .await,
            machine,
        );
    }
    match CompareNps::execute(
        &AffinityUciSessionFactory::new(apply_nps_affinity),
        &participants,
        design,
    )
    .await
    {
        Ok(report) => {
            let self_pair = participants[0].participant.launch.executable
                == participants
                    .iter()
                    .find(|item| item.arm == "B")
                    .expect("comparison has B")
                    .participant
                    .launch
                    .executable;
            if !self_pair {
                eprintln!(
                    "nps warning: no self pair was recorded; run --self-pair under matching conditions to measure harness noise"
                );
            } else if let [left, right] = report.arms.as_slice() {
                let difference = ((right.median_nps / left.median_nps) - 1.0).abs() * 100.0;
                if difference > command.self_tolerance_percent {
                    eprintln!(
                        "nps warning: self-pair median difference {difference:.3}% exceeds tolerance ±{:.3}%",
                        command.self_tolerance_percent
                    );
                }
            }
            if machine {
                print_json(&MachineOutput::NpsComparison { report });
            } else {
                for arm in &report.arms {
                    println!(
                        "arm {}: median {:.3} NPS (95% bootstrap CI {:.3}..{:.3}); best build {:.3}",
                        arm.arm,
                        arm.median_nps,
                        arm.median_ci95[0],
                        arm.median_ci95[1],
                        arm.best_of_nps
                    );
                    for build in &arm.builds {
                        println!(
                            "  {}: median {:.3} over {} samples",
                            build.build, build.median_nps, build.samples
                        );
                    }
                }
                println!(
                    "per-round ratio SD: {}",
                    report.per_round_ratio_sd.map_or_else(
                        || "unavailable (need at least two rounds)".into(),
                        |value| format!("{value:.6}")
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("nps measurement failed: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn apply_nps_affinity(
    process_id: u32,
    allocation: &colosseum_application::CpuAllocation,
) -> Result<(), String> {
    colosseum_engine::apply_process_affinity(process_id, allocation)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) async fn run_nps_scaling(
    command: NpsCommand,
    base_launch: EngineLaunchSpec,
    machine: bool,
    dry_run: bool,
) -> ExitCode {
    if command.against.is_some()
        || command.self_pair
        || !command.a_builds.is_empty()
        || !command.b_builds.is_empty()
        || !command.moves.is_empty()
    {
        eprintln!(
            "configuration error: scaling sweep cannot be combined with A/B builds or --move"
        );
        return ExitCode::from(2);
    }
    let threads = match parse_scaling_threads(command.scale_threads.as_deref().unwrap_or_default())
    {
        Ok(threads) => threads,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let Some(threads_option) = command.threads_option.as_deref() else {
        eprintln!("configuration error: --threads-option is required for a scaling sweep");
        return ExitCode::from(2);
    };
    let topology = match detect_cpu_topology() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires exact CPU topology: {error}");
            return ExitCode::from(2);
        }
    };
    let allowed = match detect_allowed_cpu_set(&topology) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires the allowed CPU set: {error}");
            return ExitCode::from(2);
        }
    };
    let characteristics = match detect_cpu_characteristics(&topology) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires CPU class/NUMA evidence: {error}");
            return ExitCode::from(2);
        }
    };
    let placement = match plan_cpu_placement(
        &topology,
        &allowed,
        &characteristics,
        &CpuPlacementPolicy::Auto {
            headroom_physical_cores: 0,
        },
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: cannot allocate scaling cores: {error}");
            return ExitCode::from(2);
        }
    };
    let CpuPlacementPlan::WholePhysicalCores { cores, .. } = placement else {
        eprintln!("configuration error: scaling requires whole physical-core placement");
        return ExitCode::from(2);
    };
    if threads.last().copied().unwrap_or(0) as usize > cores.len() {
        eprintln!(
            "configuration error: requested {} threads but only {} allowed physical cores are available",
            threads.last().copied().unwrap_or(0),
            cores.len()
        );
        return ExitCode::from(2);
    }
    let positions = if command.positions.is_empty() {
        vec!["startpos".into()]
    } else {
        command.positions.clone()
    };
    if positions == ["startpos"] {
        eprintln!(
            "nps warning: startpos alone is a weak workload; use representative positions for decisions"
        );
    }
    let hash_policy: NpsHashPolicy = command.hash_policy.into();
    let mut jobs = Vec::new();
    for (job_index, thread_count) in threads.iter().copied().enumerate() {
        let selected = &cores[..thread_count as usize];
        let cpus = selected
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .collect::<Vec<_>>();
        let point_hash = match scaling_hash_mb(hash_policy, command.hash_mb, thread_count) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        };
        let mut launch = base_launch.clone();
        launch.options.insert(
            threads_option.to_owned(),
            UciOptionValue::String(thread_count.to_string()),
        );
        launch.options.insert(
            command.hash_option.clone(),
            UciOptionValue::String(point_hash.to_string()),
        );
        launch.allocated_cpus = colosseum_application::CpuAllocation::Enforced(cpus.clone());
        let participants = ["A", "B"]
            .into_iter()
            .enumerate()
            .map(|(side, arm)| NpsExperimentParticipant {
                arm: arm.into(),
                build: format!("{}t", thread_count),
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128((job_index * 2 + side + 1) as u128),
                    launch: launch.clone(),
                },
            })
            .collect();
        let selected_characteristics = characteristics
            .cores
            .iter()
            .filter(|item| item.logical_cpus.iter().any(|cpu| cpus.contains(cpu)))
            .collect::<Vec<_>>();
        let mut core_classes = selected_characteristics
            .iter()
            .map(|item| format!("{:?}", item.core_class))
            .collect::<Vec<_>>();
        core_classes.sort();
        core_classes.dedup();
        let mut numa_nodes = selected_characteristics
            .iter()
            .filter_map(|item| item.numa_node)
            .map(|node| format!("{}:{}", node.group, node.number))
            .collect::<Vec<_>>();
        numa_nodes.sort();
        numa_nodes.dedup();
        jobs.push(NpsScalingJob {
            threads: thread_count,
            hash_mb: point_hash,
            cpus,
            core_classes,
            numa_nodes,
            participants,
            design: NpsExperimentDesign {
                nodes: command.nodes,
                positions: positions.clone(),
                repetitions: command.repetitions,
                warmup_repetitions: command.warmup,
                deadline_ms: command.deadline_ms,
                state_policy: command.state.into(),
                seed: command.seed,
                bootstrap_samples: command.bootstrap_samples,
            },
        });
    }
    let current_directory = match std::env::current_dir() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: cannot read current directory: {error}");
            return ExitCode::from(2);
        }
    };
    let mut path_pointers = Vec::new();
    for (job, scaling_job) in jobs.iter().enumerate() {
        for participant in 0..2 {
            path_pointers.push(format!(
                "/jobs/{job}/participants/{participant}/participant/launch/executable"
            ));
            if scaling_job.participants[participant]
                .participant
                .launch
                .working_directory
                .is_some()
            {
                path_pointers.push(format!(
                    "/jobs/{job}/participants/{participant}/participant/launch/working_directory"
                ));
            }
        }
    }
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "nps-scaling",
            "hash_policy": hash_policy,
            "threads_option": threads_option,
            "hash_option": command.hash_option,
            "jobs": jobs,
            "topology": topology,
            "characteristics": characteristics,
        }),
        &[],
        &current_directory,
        &path_pointers,
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let jobs: Vec<NpsScalingJob> = serde_json::from_value(resolved.value()["jobs"].clone())
        .expect("resolved scaling jobs retain their schema");
    if dry_run {
        let invocations = jobs
            .iter()
            .flat_map(|job| job.participants.iter().map(|item| &item.participant.launch))
            .collect();
        print_output(
            &MachineOutput::DryRun {
                command: "nps-scaling",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations,
                wave_shape: None,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }
    let factory = AffinityUciSessionFactory::new(apply_nps_affinity);
    let mut inputs = Vec::new();
    for job in jobs {
        let comparison = match CompareNps::execute(&factory, &job.participants, job.design).await {
            Ok(value) => value,
            Err(error) => {
                eprintln!("nps scaling failed at {} threads: {error}", job.threads);
                return ExitCode::FAILURE;
            }
        };
        let mut values = comparison
            .samples
            .iter()
            .map(|sample| sample.measurement.authoritative_nps)
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        let median_nps = if values.len().is_multiple_of(2) {
            (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
        } else {
            values[values.len() / 2]
        };
        inputs.push(NpsScalingInput {
            threads: job.threads,
            pinned_physical_cores: job.threads,
            hash_mb: job.hash_mb,
            median_nps,
            core_classes: job.core_classes,
            numa_nodes: job.numa_nodes,
        });
    }
    let report = match summarize_nps_scaling(hash_policy, inputs) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("nps scaling failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if machine {
        print_json(&MachineOutput::NpsScaling { report });
    } else {
        for point in &report.points {
            println!(
                "{} threads / {} cores / {} MiB Hash: {:.3} NPS, {:.3}x speedup, {:.2}% efficiency",
                point.input.threads,
                point.input.pinned_physical_cores,
                point.input.hash_mb,
                point.input.median_nps,
                point.speedup,
                point.parallel_efficiency * 100.0
            );
        }
    }
    ExitCode::SUCCESS
}

pub(crate) fn parse_scaling_threads(value: &str) -> Result<Vec<u32>, String> {
    let mut threads = value
        .split(',')
        .map(|item| {
            item.parse::<u32>()
                .map_err(|_| format!("invalid scaling thread count {item:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    threads.sort_unstable();
    if threads.first().copied() != Some(1)
        || threads.contains(&0)
        || threads.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err("scaling thread counts must be unique, positive, and include 1".into());
    }
    Ok(threads)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NpsScalingJob {
    pub(crate) threads: u32,
    pub(crate) hash_mb: u64,
    pub(crate) cpus: Vec<colosseum_application::LogicalCpuId>,
    pub(crate) core_classes: Vec<String>,
    pub(crate) numa_nodes: Vec<String>,
    pub(crate) participants: Vec<NpsExperimentParticipant>,
    pub(crate) design: NpsExperimentDesign,
}

pub(crate) fn print_nps_result(
    result: Result<NpsReport, colosseum_application::ApplicationError>,
    machine: bool,
) -> ExitCode {
    match result {
        Ok(report) => {
            if machine {
                print_json(&MachineOutput::Nps { report });
            } else {
                println!("authoritative NPS: {:.3}", report.authoritative_nps);
                println!(
                    "fixed work: {} requested, {} reported; harness wall time: {:.3} ms",
                    report.requested_nodes,
                    report.reported_nodes,
                    report.harness_elapsed_ns as f64 / 1_000_000.0
                );
                println!(
                    "engine diagnostics: time={} ms, nps={}",
                    report
                        .engine_reported_time_ms
                        .map_or_else(|| "unavailable".into(), |value| value.to_string()),
                    report
                        .engine_reported_nps
                        .map_or_else(|| "unavailable".into(), |value| value.to_string())
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("nps measurement failed: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn nps_participants(
    base: &EngineLaunchSpec,
    command: &NpsCommand,
) -> Vec<NpsExperimentParticipant> {
    let mut paths_a = vec![base.executable.clone()];
    paths_a.extend(command.a_builds.clone());
    let mut paths_b = vec![
        command
            .against
            .clone()
            .unwrap_or_else(|| base.executable.clone()),
    ];
    paths_b.extend(command.b_builds.clone());
    paths_a
        .into_iter()
        .enumerate()
        .map(|(index, path)| ("A", index, path))
        .chain(
            paths_b
                .into_iter()
                .enumerate()
                .map(|(index, path)| ("B", index, path)),
        )
        .enumerate()
        .map(|(identity, (arm, index, executable))| {
            let mut launch = base.clone();
            launch.executable = executable.clone();
            launch.label = Some(nps_build_label(&executable, arm, index + 1));
            NpsExperimentParticipant {
                arm: arm.into(),
                build: launch.label.clone().expect("label assigned"),
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128(identity as u128 + 1),
                    launch,
                },
            }
        })
        .collect()
}

pub(crate) fn nps_build_label(path: &Path, arm: &str, ordinal: usize) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| format!("{arm}-{ordinal}"), str::to_owned)
}
