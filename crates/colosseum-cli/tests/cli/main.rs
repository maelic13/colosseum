//! The CLI's integration tests: the `colosseum-cli` binary driven as a user
//! drives it, against the repository's own UCI fixture engines.
//!
//! One test binary with a module per behaviour, so the suite links once.
//! Logic that needs no process is unit-tested beside its code in `src/`.

mod architecture;
mod calibration;
mod chess960;
mod command_line;
mod cpu_allocation;
mod engine_check;
mod engine_processes;
mod engine_setup_faults;
mod game_phases;
mod graceful_stop;
mod journal_recovery;
mod match_schedule;
mod nps;
mod pgn_annotations;
mod pgn_replay;
mod progress;
mod search_timing;
mod slot_pool;
mod spsa;
mod status;
mod stop_boundaries;
mod tournament;
mod unscorable_games;
