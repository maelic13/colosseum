use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use colosseum_application::{
    EngineLaunchSpec, PlanTournament, RateTournament, RuntimeParticipant, TournamentCompletedGame,
    TournamentDesign, TournamentParticipant,
};
use colosseum_core::{Format, GameResult, ParticipantId, Termination};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Acceptance {
    schema_version: u32,
    phase: String,
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct GuiParity {
    schema_version: u32,
    source: String,
    participants: Vec<FixtureParticipant>,
    round_robin: RoundRobinFixture,
    multi_seed_gauntlet: GauntletFixture,
}

#[derive(Debug, Deserialize)]
struct FixtureParticipant {
    name: String,
    prior: f64,
}

#[derive(Debug, Deserialize)]
struct RoundRobinFixture {
    schedule: Vec<String>,
    results: Vec<String>,
    ratings: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct GauntletFixture {
    seeds: u32,
    schedule: Vec<String>,
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn participants(fixture: &GuiParity) -> Vec<TournamentParticipant> {
    fixture
        .participants
        .iter()
        .enumerate()
        .map(|(index, fixture)| TournamentParticipant {
            participant: RuntimeParticipant {
                id: ParticipantId::from_u128(index as u128 + 1),
                launch: EngineLaunchSpec {
                    label: Some(fixture.name.clone()),
                    ..EngineLaunchSpec::path_only(fixture.name.clone().into())
                },
            },
            initial_rating: fixture.prior,
        })
        .collect()
}

fn schedule_rows(plan: &colosseum_application::TournamentPlan) -> Vec<String> {
    plan.schedule
        .iter()
        .map(|game| {
            format!(
                "{}:{}:{}:{}",
                game.number,
                game.round,
                game.white.as_uuid().as_u128(),
                game.black.as_uuid().as_u128()
            )
        })
        .collect()
}

#[test]
fn stored_gui_fixture_matches_both_cli_schedules_and_ratings() {
    let fixture: GuiParity = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase7/gui-parity.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert!(fixture.source.contains("GUI"));
    let round_robin = PlanTournament::execute(
        participants(&fixture),
        TournamentDesign {
            format: Format::RoundRobin { cycles: 1 },
            games_per_pair: 2,
        },
    )
    .unwrap();
    assert_eq!(schedule_rows(&round_robin), fixture.round_robin.schedule);
    let games = round_robin
        .schedule
        .iter()
        .zip(&fixture.round_robin.results)
        .map(|(game, result)| TournamentCompletedGame {
            number: game.number,
            white: game.white,
            black: game.black,
            result: match result.as_str() {
                "white-win" => GameResult::WhiteWin,
                "black-win" => GameResult::BlackWin,
                "draw" => GameResult::Draw,
                other => panic!("unknown fixture result {other}"),
            },
            scorable: true,
            termination: Termination::Checkmate,
        })
        .collect::<Vec<_>>();
    let report = RateTournament::execute(&round_robin, &games, None).unwrap();
    let actual = fixture
        .participants
        .iter()
        .enumerate()
        .map(|(index, _)| {
            report
                .standings
                .iter()
                .find(|row| row.participant == ParticipantId::from_u128(index as u128 + 1))
                .unwrap()
                .rating
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), fixture.round_robin.ratings.len());
    assert!(
        actual
            .iter()
            .zip(&fixture.round_robin.ratings)
            .all(|(actual, expected)| (actual - expected).abs() <= 0.01),
        "actual GUI-parity ratings: {actual:?}"
    );

    let gauntlet = PlanTournament::execute(
        participants(&fixture),
        TournamentDesign {
            format: Format::Gauntlet {
                seeds: fixture.multi_seed_gauntlet.seeds,
                cycles: 1,
            },
            games_per_pair: 2,
        },
    )
    .unwrap();
    assert_eq!(
        schedule_rows(&gauntlet),
        fixture.multi_seed_gauntlet.schedule
    );
}

fn tournament_command(run: &Path, format: &str, sleep_ms: u64) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let mut command = cli();
    command.args(["tournament", "run", "--format", format]);
    for label in ["Alpha", "Beta", "Gamma", "Delta"] {
        command
            .arg("--engine")
            .arg(binary)
            .arg("--label")
            .arg(label);
    }
    command
        .args(["--engine-arg=__uci-stub"])
        .arg(format!("--engine-arg=--sleep-ms={sleep_ms}"))
        .args([
            "--games-per-pair",
            "2",
            "--max-moves",
            "1",
            "--placement",
            "off",
            "--concurrency",
            "2",
            "--seed",
            "9",
            "--dir",
        ])
        .arg(run)
        .arg("--json");
    if format == "gauntlet" {
        command.arg("--seeds").arg("2");
    }
    command
}

fn checkpoint_games(run: &Path) -> Option<usize> {
    let bytes = std::fs::read(run.join("checkpoint.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["payload"]["games"].as_array().map(Vec::len)
}

fn run_json(run: &Path, format: &str, sleep_ms: u64) -> serde_json::Value {
    let output = tournament_command(run, format, sleep_ms).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_resume_matches_uninterrupted(root: &Path, format: &str, total_games: usize) {
    let uninterrupted = run_json(&root.join(format!("{format}-full")), format, 30);
    let resumed_dir = root.join(format!("{format}-resume"));
    let mut child = tournament_command(&resumed_dir, format, 30)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if checkpoint_games(&resumed_dir).is_some_and(|games| (1..total_games).contains(&games)) {
            break;
        }
        assert!(Instant::now() < deadline, "no partial {format} checkpoint");
        thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let resumed = run_json(&resumed_dir, format, 30);
    assert_eq!(
        resumed["report"]["plan"]["schedule"],
        uninterrupted["report"]["plan"]["schedule"]
    );
    assert_eq!(
        resumed["report"]["results"]["standings"],
        uninterrupted["report"]["results"]["standings"]
    );
    assert_eq!(
        resumed["report"]["results"]["crosstable_csv"],
        uninterrupted["report"]["results"]["crosstable_csv"]
    );
    let numbers = resumed["report"]["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|game| game["number"].as_u64().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(numbers.len(), total_games);
}

#[test]
fn both_formats_resume_to_uninterrupted_standings() {
    let root = tempfile::tempdir().unwrap();
    assert_resume_matches_uninterrupted(root.path(), "round-robin", 12);
    assert_resume_matches_uninterrupted(root.path(), "gauntlet", 8);
}

#[test]
fn acceptance_manifest_names_every_phase_seven_exit_gate_and_owner() {
    let acceptance: Acceptance = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase7/acceptance.json"
    ))
    .unwrap();
    assert_eq!(acceptance.schema_version, 1);
    assert_eq!(acceptance.phase, "7");
    let expected = BTreeSet::from([
        "gui-schedule-parity",
        "gui-rating-parity-0.01-elo",
        "round-robin-kill-resume",
        "multi-seed-gauntlet-kill-resume",
        "workspace-regression",
    ]);
    let actual = acceptance
        .gates
        .iter()
        .map(|gate| gate.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), acceptance.gates.len());
    assert!(
        acceptance
            .gates
            .iter()
            .all(|gate| !gate.evidence.is_empty())
    );
}
