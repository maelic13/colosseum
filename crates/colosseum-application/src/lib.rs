//! Runtime-neutral Colosseum application boundary.
//!
//! This package owns use-case inputs, outputs and ports. It deliberately has no
//! process, filesystem, database, GUI, channel or async-runtime implementation.

pub mod commit;
pub mod inspect;
pub mod model;
pub mod ports;

pub use commit::{CommitUnit, CommitUnitDependencies};
pub use inspect::InspectEngine;
pub use model::*;
pub use ports::*;
