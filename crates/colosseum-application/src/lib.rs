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
pub mod spsa_plan;

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
pub use spsa::{
    DEFAULT_SPSA_FINAL_WINDOW_PERCENT, DEFAULT_SPSA_GAMES_PER_ITERATION, DEFAULT_SPSA_ITERATIONS,
    SPSA_TUNE_RESULT_SCHEMA_VERSION, SpsaBoundParameter, SpsaBoundTune, SpsaCenterSample,
    SpsaCommittedUpdate, SpsaDriverPolicyError, SpsaFinalWindow, SpsaGateHashStatus,
    SpsaGateIdentity, SpsaInvalidUpdate, SpsaIterationTransition, SpsaLiveSpin, SpsaMiniMatchScore,
    SpsaPreflightError, SpsaResultParameter, SpsaRunSettings, SpsaRunSettingsError, SpsaTune,
    SpsaTuneAudit, SpsaTuneAuditError, SpsaTuneError, SpsaTuneParameter, SpsaTuneResult,
    SpsaTuneResultError, SpsaTuneWarning, SpsaTuningState, VerifiedSpsaSchedule,
};
pub use spsa_plan::{
    SPSA_PLAN_SCHEMA_VERSION, SpsaHorizonComparison, SpsaHorizonKnob, SpsaKnobPlan, SpsaPlanError,
    SpsaPlanPoint, SpsaPlanReport, SpsaTimingBasis, SpsaTimingInput, SpsaWallTimeEstimate,
    plan_spsa,
};
