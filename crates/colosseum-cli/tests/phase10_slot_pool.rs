//! A CPU slot belongs to one game — or one colour-reversed pair — at a time.
//!
//! Games of deliberately uneven length run at concurrency 4. Every game's
//! journal record names the slot it ran on and when it held it, from before
//! its first engine was spawned to after both engines had exited. On any one
//! slot those spans must never overlap. Choosing a slot by game number, as the
//! drivers once did, puts a new game on a slot whose previous game is still
//! playing as soon as lengths differ, and fails here.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const SLOTS: u64 = 4;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command.arg("--json");
    command
}

fn engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn run(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[derive(Debug, Clone, Copy)]
struct Span {
    game: u64,
    pair: u64,
    slot: u64,
    started: u64,
    ended: u64,
}

fn spans(directory: &Path) -> Vec<Span> {
    std::fs::read_to_string(directory.join("games.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let game = &serde_json::from_str::<Value>(line).unwrap()["game"];
            let slot = &game["slot"];
            Span {
                game: game["number"].as_u64().unwrap(),
                pair: game["pair_number"].as_u64().unwrap(),
                slot: slot["index"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("no slot recorded: {game}")),
                started: slot["started_unix_us"].as_u64().unwrap(),
                ended: slot["ended_unix_us"].as_u64().unwrap(),
            }
        })
        .collect()
}

/// The invariant, checked over a whole run: no two live units ever hold the
/// same slot, every slot is used, and the PGN names the slot the journal does.
/// `pairs` makes the unit a colour-reversed pair, whose two games share one
/// slot and run one after the other.
fn assert_one_unit_per_slot(directory: &Path, pairs: bool) -> Vec<Span> {
    let spans = spans(directory);
    assert!(spans.len() >= 8, "too few games to say anything: {spans:?}");
    let mut by_slot = BTreeMap::<u64, Vec<Span>>::new();
    for span in &spans {
        assert!(span.slot < SLOTS, "slot {} of {SLOTS}", span.slot);
        assert!(span.started <= span.ended, "{span:?}");
        by_slot.entry(span.slot).or_default().push(*span);
    }
    assert_eq!(
        by_slot.len() as u64,
        SLOTS,
        "a slot stood idle for the whole run: {spans:?}"
    );
    for (slot, mut held) in by_slot {
        held.sort_by_key(|span| span.started);
        for window in held.windows(2) {
            let (before, after) = (window[0], window[1]);
            assert!(
                after.started >= before.ended,
                "slot {slot} held by game {} while game {} still ran: {before:?} {after:?}",
                after.game,
                before.game
            );
        }
    }
    if pairs {
        let mut by_pair = BTreeMap::<u64, Vec<Span>>::new();
        for span in &spans {
            by_pair.entry(span.pair).or_default().push(*span);
        }
        for (pair, games) in by_pair {
            if let [first, second] = games[..] {
                assert_eq!(first.slot, second.slot, "pair {pair} split across slots");
            }
        }
    }
    let pgn = std::fs::read_to_string(directory.join("games.pgn")).unwrap();
    for span in &spans {
        let game = pgn
            .split("[Event ")
            .find(|game| game.contains(&format!("[GameNumber \"{}\"]", span.game)))
            .unwrap_or_else(|| panic!("game {} is not in the PGN", span.game));
        assert!(
            game.contains(&format!("[GameSlot \"{}\"]", span.slot)),
            "game {} ran on slot {} but its PGN says otherwise:\n{game}",
            span.game,
            span.slot
        );
    }
    spans
}

/// Games of uneven length: each engine process draws a delay factor from its
/// process identifier.
fn uneven(command: &mut Command, prefix: &str) {
    command.args([
        &format!("--{prefix}engine-arg=__uci-stub"),
        &format!("--{prefix}engine-arg=--uneven-sleep-ms=8"),
    ]);
}

#[test]
fn a_match_never_puts_two_games_on_one_slot() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("match");
    let mut command = cli();
    command.arg("match").arg(engine()).arg(engine());
    uneven(&mut command, "a-");
    uneven(&mut command, "b-");
    let value = run(command
        .args([
            "--games",
            "32",
            "--concurrency",
            "4",
            "--a-movetime-ms",
            "5",
            "--b-movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--seed",
            "3",
            // Uneven stubs overrun their movetime on purpose; slots, not
            // clocks, are under test here.
            "--a-margin-ms",
            "2000",
            "--b-margin-ms",
            "2000",
            "--dir",
        ])
        .arg(&directory));
    assert_eq!(value["report"]["status"], "completed", "{value}");
    let spans = assert_one_unit_per_slot(&directory, false);
    assert_eq!(spans.len(), 32);
    // Every game's report carries its slot too.
    assert!(
        value["report"]["games"]
            .as_array()
            .unwrap()
            .iter()
            .all(|game| game["slot"]["index"]
                .as_u64()
                .is_some_and(|slot| slot < SLOTS))
    );
}

#[test]
fn an_sprt_pair_holds_one_slot_for_both_of_its_games() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("sprt");
    let mut command = cli();
    command.arg("sprt").arg(engine()).arg(engine());
    uneven(&mut command, "a-");
    uneven(&mut command, "b-");
    run(command
        .args([
            "--preset",
            "gainer",
            "--max-pairs",
            "16",
            "--concurrency",
            "4",
            "--a-movetime-ms",
            "5",
            "--b-movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "1000",
            "--max-time-losses",
            "1000",
            "--seed",
            "3",
            "--dir",
        ])
        .arg(&directory));
    assert_one_unit_per_slot(&directory, true);
}

/// Inside an SPSA iteration the game, not the pair, holds a slot.
///
/// Five pairs on four slots, with games of uneven length. Were the pair the
/// unit, four pairs would play their two games back to back and the fifth
/// would then play both of its games while three slots idled; no pair's games
/// could ever run at the same time. Each game taking a free slot instead
/// keeps the slots busy until the last wave, and the first wave alone puts
/// both games of pairs 1 and 2 on the machine at once.
#[test]
fn an_spsa_iteration_places_each_game_on_a_free_slot() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let directory = root.path().join("spsa");
    let mut command = cli();
    command
        .arg("spsa")
        .arg(engine())
        .arg("--engine-arg=__uci-stub")
        .arg("--engine-arg=--uneven-sleep-ms=8")
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "2",
            "--games-per-iteration",
            "10",
            "--concurrency",
            "4",
            "--depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(&directory);
    let value = run(&mut command);
    assert_eq!(value["report"]["driver"]["status"], "completed", "{value}");
    // The per-slot invariants hold for games: never two on one slot, every
    // slot used, and the PGN names the slot the journal does.
    let spans = assert_one_unit_per_slot(&directory, false);
    assert_eq!(spans.len(), 20);

    let iterations = std::fs::read_to_string(directory.join("games.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let game = &serde_json::from_str::<Value>(line).unwrap()["game"];
            (
                game["number"].as_u64().unwrap(),
                game["iteration"].as_u64().unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut concurrent_pairs = 0;
    for iteration in 0..2 {
        let games = spans
            .iter()
            .filter(|span| iterations[&span.game] == iteration)
            .collect::<Vec<_>>();
        assert_eq!(games.len(), 10);
        // Occupancy: slot-time held by games over the slot-time the iteration
        // spanned. Pair-held slots leave three of four slots idle while the
        // fifth pair plays its second game.
        let started = games.iter().map(|span| span.started).min().unwrap();
        let ended = games.iter().map(|span| span.ended).max().unwrap();
        let held = games
            .iter()
            .map(|span| span.ended - span.started)
            .sum::<u64>();
        let occupancy = held as f64 / (SLOTS as f64 * (ended - started) as f64);
        eprintln!("iteration {iteration}: occupancy {:.0}%", occupancy * 100.0);
        assert!(
            occupancy >= 0.6,
            "iteration {iteration} kept its slots {:.0}% busy",
            occupancy * 100.0
        );
        // Games launch in schedule order. Each span is stamped when its task
        // first runs, which the runtime may reorder by a few microseconds, so
        // order is asserted to within 20 ms.
        let mut by_number = games.clone();
        by_number.sort_by_key(|span| span.game);
        for window in by_number.windows(2) {
            assert!(
                window[1].started + 20_000 >= window[0].started,
                "game {} started before game {}",
                window[1].game,
                window[0].game
            );
        }
        // A pair's two games may run at once, on different slots.
        for pair in by_number.chunks(2) {
            let (first, second) = (pair[0], pair[1]);
            assert_eq!(first.pair, second.pair);
            if second.started < first.ended {
                assert_ne!(first.slot, second.slot);
                concurrent_pairs += 1;
            }
        }
    }
    assert!(
        concurrent_pairs >= 2,
        "no pair ran its two games at once: the pair is still holding the slot"
    );
    // The gradient still used whole pairs: every iteration committed its
    // five, games 10k + 1 to 10k + 10 of the journal, with a full score.
    for (index, iteration) in value["report"]["driver"]["completed_iterations"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let first = index as u64 * 10 + 1;
        assert_eq!(iteration["games"]["first"], first);
        assert_eq!(iteration["games"]["last"], first + 9);
        let score = &iteration["score"];
        assert_eq!(
            score["plus_wins"].as_u64().unwrap()
                + score["plus_losses"].as_u64().unwrap()
                + score["draws"].as_u64().unwrap(),
            10
        );
    }
}

#[test]
fn a_tournament_never_puts_two_games_on_one_slot() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("tournament");
    let mut command = cli();
    command
        .args(["tournament", "run"])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args([
            "--label",
            "a",
            "--label",
            "b",
            "--label",
            "c",
            "--engine-arg=__uci-stub",
            "--engine-arg=--uneven-sleep-ms=8",
            "--movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--games-per-pair",
            "4",
            "--concurrency",
            "4",
            "--seed",
            "5",
            "--margin-ms",
            "2000",
            "--dir",
        ])
        .arg(&directory);
    let value = run(&mut command);
    assert_eq!(value["report"]["status"], "completed", "{value}");
    assert_one_unit_per_slot(&directory, false);
}
