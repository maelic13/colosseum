//! Shared "save results to a file" helpers used by both the live Tournament
//! view and the History tab. Each opens a native save dialog (via `rfd`) and
//! writes the supplied text, returning a short status note to surface inline.

use colosseum_core::{EngineId, ExportRow, Standings, crosstable_csv, standings_csv};

/// Write `contents` to a user-chosen file seeded with `default_name`/`ext`.
/// Returns `None` if the dialog was cancelled, else a status note (ok or error).
fn save_text(default_name: &str, ext: &str, contents: &str) -> Option<String> {
    let path = rfd::FileDialog::new()
        .set_title("Export")
        .set_file_name(default_name)
        .add_filter(ext.to_uppercase(), &[ext])
        .save_file()?;
    match std::fs::write(&path, contents) {
        Ok(()) => {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            Some(format!("Saved {name}"))
        }
        Err(e) => Some(format!("Save failed: {e}")),
    }
}

/// Sanitize a tournament name into a safe filename stem.
fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "tournament".to_string() } else { s }
}

/// Export the standings table as CSV. Returns a status note unless cancelled.
pub fn export_standings_csv(tournament_name: &str, rows: &[ExportRow]) -> Option<String> {
    let csv = standings_csv(rows);
    save_text(&format!("{}_standings.csv", slug(tournament_name)), "csv", &csv)
}

/// Export the head-to-head crosstable as CSV. Returns a status note unless cancelled.
pub fn export_crosstable_csv(
    tournament_name: &str,
    order: &[(EngineId, String)],
    standings: &Standings,
) -> Option<String> {
    let csv = crosstable_csv(order, standings);
    save_text(&format!("{}_crosstable.csv", slug(tournament_name)), "csv", &csv)
}

/// Export the concatenated game PGN. Returns a status note unless cancelled.
/// An empty `pgn` yields a note without opening a dialog.
pub fn export_pgn(tournament_name: &str, pgn: &str) -> Option<String> {
    if pgn.trim().is_empty() {
        return Some("No finished games to export yet.".to_string());
    }
    save_text(&format!("{}.pgn", slug(tournament_name)), "pgn", pgn)
}
