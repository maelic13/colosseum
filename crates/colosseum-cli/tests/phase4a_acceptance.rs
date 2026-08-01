use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn base_match(run: &Path, games: u32) -> Command {
    let mut command = cli();
    command
        .args(["match", "--games", &games.to_string()])
        .arg(fixture())
        .arg(fixture())
        .arg("--dir")
        .arg(run)
        .args(["--seed", "424242", "--max-engine-faults", "100", "--json"]);
    command
}

fn successful_json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn wait_for_checkpoint(child: &mut Child, checkpoint: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "match ended before kill fixture"
        );
        if checkpoint.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("match did not create a checkpoint within ten seconds");
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn wait_for_new_attached_engines(
    child: &mut Child,
    a_pid_file: &Path,
    b_pid_file: &Path,
    old_a: u32,
    old_b: u32,
) -> (u32, u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "match ended before second game"
        );
        if let (Some(a), Some(b)) = (read_pid(a_pid_file), read_pid(b_pid_file))
            && a != old_a
            && b != old_b
        {
            return (a, b);
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("second game's attached engines did not start within ten seconds");
}

fn assert_processes_reaped(pids: [u32; 2]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if pids
            .iter()
            .all(|pid| !colosseum_uci::process_is_alive(*pid))
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("engine processes remained after the match owner was killed: {pids:?}");
}

#[test]
fn killed_match_resumes_missing_games_in_deterministic_schedule_order() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("resumable");
    let a_pid_file = root.path().join("a.pid");
    let b_pid_file = root.path().join("b.pid");
    let mut first = base_match(&run, 20);
    first
        .arg("--a-engine-arg=--sleep-ms=100")
        .arg(format!(
            "--a-engine-arg=--pid-file={}",
            a_pid_file.display()
        ))
        .arg(format!(
            "--b-engine-arg=--pid-file={}",
            b_pid_file.display()
        ));
    first.stdout(std::process::Stdio::null());
    let mut child = first.spawn().unwrap();
    while read_pid(&a_pid_file).is_none() || read_pid(&b_pid_file).is_none() {
        assert!(child.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(10));
    }
    let first_a = read_pid(&a_pid_file).unwrap();
    let first_b = read_pid(&b_pid_file).unwrap();
    wait_for_checkpoint(&mut child, &run.join("checkpoint.json"));
    let active =
        wait_for_new_attached_engines(&mut child, &a_pid_file, &b_pid_file, first_a, first_b);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_processes_reaped([active.0, active.1]);

    let mut resumed = base_match(&run, 20);
    resumed
        .arg("--a-engine-arg=--sleep-ms=100")
        .arg(format!(
            "--a-engine-arg=--pid-file={}",
            a_pid_file.display()
        ))
        .arg(format!(
            "--b-engine-arg=--pid-file={}",
            b_pid_file.display()
        ));
    let value = successful_json(resumed.output().unwrap());
    let games = value["report"]["games"].as_array().unwrap();
    let numbers = games
        .iter()
        .map(|game| game["number"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(numbers, (1..=20).collect::<Vec<_>>());
    assert_eq!(numbers.iter().copied().collect::<BTreeSet<_>>().len(), 20);
    assert_eq!(value["report"]["games_attempted"], 20);
    assert_eq!(value["report"]["status"], "completed");
    assert!(run.join("checkpoint.previous.json").is_file());
    assert!(
        std::fs::read_to_string(run.join("games.pgn"))
            .unwrap()
            .matches("[Event \"Colosseum CLI fixed match\"]")
            .count()
            == 20
    );
}

fn write_book(root: &Path) -> PathBuf {
    let book = root.join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -\n\
rnbqkb1r/pppppppp/5n2/8/8/5N2/PPPPPPPP/RNBQKB1R w KQkq -\n\
rnbqkbnr/pppp1ppp/8/4p3/8/8/PPPPPPPP/RNBQKBNR w KQkq -\n",
    )
    .unwrap();
    book
}

#[test]
fn concurrency_cannot_change_the_fixed_match_schedule() {
    let root = tempfile::tempdir().unwrap();
    let book = write_book(root.path());
    let mut reports = Vec::new();
    for concurrency in [1, 3] {
        let run = root.path().join(format!("run-{concurrency}"));
        let mut command = base_match(&run, 6);
        command
            .args(["--concurrency", &concurrency.to_string(), "--book"])
            .arg(&book)
            .args(["--book-order", "random"]);
        let value = successful_json(command.output().unwrap());
        reports.push(
            value["report"]["games"]
                .as_array()
                .unwrap()
                .iter()
                .map(|game| {
                    (
                        game["number"].as_u64().unwrap(),
                        game["white"].as_str().unwrap().to_owned(),
                        game["opening"]["book_index"].as_u64().unwrap(),
                        game["result"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(reports[0], reports[1]);
}

fn clock_match(root: &Path, sleep_ms: u64, budget_ms: u64, margin_ms: u64) -> Output {
    let mut command = base_match(root, 1);
    command
        .arg(format!("--a-engine-arg=--sleep-ms={sleep_ms}"))
        .args([
            "--a-movetime-ms",
            &budget_ms.to_string(),
            "--a-margin-ms",
            &margin_ms.to_string(),
        ]);
    command.output().unwrap()
}

#[test]
fn sleeping_fixture_is_charged_and_margin_outcomes_are_attributed() {
    let root = tempfile::tempdir().unwrap();
    let accepted = successful_json(clock_match(&root.path().join("accepted"), 80, 50, 100));
    let charged_ns =
        accepted["report"]["games"][0]["clock_accounting"]["white_charged_elapsed"]["min_ns"]
            .as_u64()
            .unwrap();
    assert!(
        (50_000_000..=500_000_000).contains(&charged_ns),
        "80 ms fixture was charged {charged_ns} ns"
    );
    assert_eq!(accepted["report"]["faults"]["time_losses_a"], 0);

    let forfeited = clock_match(&root.path().join("forfeited"), 200, 50, 20);
    assert_eq!(forfeited.status.code(), Some(1));
    let forfeited: serde_json::Value = serde_json::from_slice(&forfeited.stdout).unwrap();
    assert_eq!(
        forfeited["report"]["games"][0]["termination"],
        "TimeForfeit"
    );
    assert_eq!(forfeited["report"]["games"][0]["fault"]["side"], "white");
    assert_eq!(forfeited["report"]["faults"]["time_losses_a"], 1);
    assert_eq!(forfeited["report"]["faults"]["time_losses_b"], 0);
}
