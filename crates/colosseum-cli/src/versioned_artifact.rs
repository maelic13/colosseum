//! Reading a stored artifact's schema version before reading its fields.
//!
//! Every versioned artifact this CLI stores does two things: it refuses a
//! schema version it cannot read, and it denies unknown fields so a stale file
//! can never be half-understood. Deserializing first makes the second rule
//! answer for the first. A field that moved between versions comes back as an
//! unknown field, the refusal that names the version is never reached, and the
//! user is told about a field instead of the version that renamed it.
//!
//! Asking for the version on its own costs one extra parse of a small file and
//! keeps the versioned refusal reachable.

use serde_json::Value;

/// The schema version a document declares, if it declares one.
///
/// `None` means the field is absent, which the full parse then reports as the
/// missing field it is.
#[must_use]
pub fn declared_schema_version(document: &Value) -> Option<u32> {
    document
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
}

/// Refuse a document that declares a schema version this build cannot read,
/// returning the version it declared.
pub fn require_schema_version(document: &Value, supported: u32) -> Result<(), u32> {
    match declared_schema_version(document) {
        Some(version) if version != supported => Err(version),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_version_is_read_without_any_other_field() {
        // Version 1 of the SPSA schedule artifact called the random-stream
        // version `stats_version`, a field this build denies. The version is
        // still readable, which is the whole point.
        let document: Value =
            serde_json::from_str(r#"{"schema_version":1,"stats_version":1,"nonsense":[1,2,3]}"#)
                .unwrap();
        assert_eq!(declared_schema_version(&document), Some(1));
        assert_eq!(require_schema_version(&document, 2), Err(1));
        assert_eq!(require_schema_version(&document, 1), Ok(()));
    }

    #[test]
    fn a_document_without_a_declared_version_is_left_to_the_full_parse() {
        let document: Value = serde_json::from_str(r#"{"schedule":{}}"#).unwrap();
        assert_eq!(declared_schema_version(&document), None);
        assert_eq!(require_schema_version(&document, 2), Ok(()));
    }
}
