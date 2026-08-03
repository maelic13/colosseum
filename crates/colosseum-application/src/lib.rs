//! Runtime-neutral Colosseum application boundary.
//!
//! This package owns use-case inputs, outputs and ports. It deliberately has no
//! process, filesystem, database, GUI, channel or async-runtime implementation.

pub mod calibration;
pub mod check;
pub mod commit;
pub mod inspect;
pub mod model;
pub mod pair_commit;
pub mod ports;
pub mod sprt;
pub mod spsa;

pub use calibration::{
    CalibrationBinaries, CalibrationDesign, CalibrationError, CalibrationInterval,
    CalibrationStatus, DEFAULT_CALIBRATION_CONFIDENCE, DEFAULT_CALIBRATION_GAMES,
    DEFAULT_CALIBRATION_TOLERANCE_NELO, classify_calibration,
};
pub use check::{CheckEngine, ComplianceCheck, ComplianceReport, ComplianceStatus};
pub use commit::{CommitUnit, CommitUnitDependencies};
pub use inspect::InspectEngine;
pub use model::*;
pub use pair_commit::{CompletePair, PairCommitError, PairCommitQueue};
pub use ports::*;
pub use sprt::{SprtBundle, SprtDesign, SprtDesignError, SprtParameters};
pub use spsa::{SpsaPreflightError, VerifiedSpsaSchedule};
