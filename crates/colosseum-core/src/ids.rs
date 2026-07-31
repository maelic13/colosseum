//! Stable, opaque identifiers for engines, games and tournaments.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(test)]
static TEST_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Construct an opaque identifier from bytes supplied by an outer
            /// identity adapter.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Deterministic constructor used by persisted input and tests.
            #[must_use]
            pub const fn from_u128(value: u128) -> Self {
                Self(Uuid::from_u128(value))
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }

            /// Deterministic convenience constructor available only to this
            /// crate's unit tests. Production identity comes from an adapter.
            #[cfg(test)]
            #[must_use]
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                use std::sync::atomic::Ordering;
                Self::from_u128(u128::from(TEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed)))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifies an engine configuration in the engine library.
    EngineId
);
id_type!(
    /// Identifies one run-local participant independently of any GUI library.
    ParticipantId
);
id_type!(
    /// Identifies a single game within a tournament.
    GameId
);
id_type!(
    /// Identifies a tournament.
    TournamentId
);
id_type!(
    /// Identifies a durable experiment run.
    RunId
);
id_type!(
    /// Identifies one immutable scheduled execution unit.
    UnitId
);
id_type!(
    /// Identifies one colour-reversed opening pair.
    PairId
);
