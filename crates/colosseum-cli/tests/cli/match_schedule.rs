use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
        .args(["--seed", "424242", "--max-engine-faults", "1000", "--json"]);
    command
}

fn successful_json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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
    for concurrency in [1, 2] {
        let run = root.path().join(format!("run-{concurrency}"));
        let mut command = base_match(&run, 4);
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
    // A sleep is a floor, never a ceiling: the 80 ms fixture always overruns
    // its 50 ms budget, and a margin far beyond any scheduler delay keeps the
    // overrun accepted. Exact sub/equal/super-margin boundaries are
    // deterministic runner unit tests.
    let accepted = successful_json(clock_match(&root.path().join("accepted"), 80, 50, 10_000));
    let charged_ns =
        accepted["report"]["games"][0]["clock_accounting"]["white_charged_elapsed"]["min_ns"]
            .as_u64()
            .unwrap();
    assert!(
        charged_ns > 50_000_000,
        "the 80 ms fixture overran its budget but was charged only {charged_ns} ns"
    );
    assert_eq!(accepted["report"]["faults"]["time_losses_a"], 0);

    // The 200 ms sleep can only exceed the 70 ms budget and margin. The
    // fixture runs allow 1000 engine faults, and an omitted time-loss limit
    // follows that allowance; a limit of zero makes the one forfeit
    // invalidate the match.
    let mut forfeited = base_match(&root.path().join("forfeited"), 1);
    forfeited.arg("--a-engine-arg=--sleep-ms=200").args([
        "--a-movetime-ms",
        "50",
        "--a-margin-ms",
        "20",
        "--max-time-losses",
        "0",
    ]);
    let forfeited = forfeited.output().unwrap();
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
