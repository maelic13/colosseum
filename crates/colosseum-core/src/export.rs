//! Tabular export of tournament results as RFC-4180 CSV.
//!
//! Pure string builders with no I/O so they are trivially testable and reusable
//! from both the live Tournament view and the History tab. The GUI fills in
//! [`ExportRow`]s (already joined with engine names / Elo) and picks the file.

use crate::ids::EngineId;
use crate::standings::Standings;

/// A fully-resolved standings row, mirroring the GUI results table.
#[derive(Debug, Clone)]
pub struct ExportRow {
    pub rank: usize,
    pub name: String,
    pub version: String,
    pub elo: f64,
    pub elo_delta: f64,
    pub points: f64,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub nps: Option<u64>,
}

/// Quote a CSV field if it contains a comma, quote, CR, or LF (RFC 4180).
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Join already-escaped-or-plain fields into one CSV line (CRLF-terminated).
fn csv_line<I: IntoIterator<Item = String>>(fields: I) -> String {
    let mut line = String::new();
    for (i, f) in fields.into_iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        line.push_str(&f);
    }
    line.push_str("\r\n");
    line
}

/// Final standings as CSV: one row per engine, ordered as given.
#[must_use]
pub fn standings_csv(rows: &[ExportRow]) -> String {
    let mut out = csv_line(
        [
            "Rank", "Engine", "Version", "Elo", "EloDelta", "Points", "Games", "Wins", "Draws",
            "Losses", "AvgNps",
        ]
        .into_iter()
        .map(str::to_string),
    );
    for r in rows {
        out.push_str(&csv_line([
            r.rank.to_string(),
            csv_field(&r.name),
            csv_field(&r.version),
            format!("{:.1}", r.elo),
            format!("{:+.1}", r.elo_delta),
            format!("{:.1}", r.points),
            r.games.to_string(),
            r.wins.to_string(),
            r.draws.to_string(),
            r.losses.to_string(),
            r.nps.map_or(String::new(), |n| n.to_string()),
        ]));
    }
    out
}

/// Head-to-head crosstable as CSV. `order` lists the participants (id + display
/// name) row/column order; each cell is `wins-draws-losses` from the row
/// engine's perspective, blank on the diagonal.
#[must_use]
pub fn crosstable_csv(order: &[(EngineId, String)], standings: &Standings) -> String {
    // Header: empty corner, then each opponent's name.
    let mut header = vec![String::new()];
    header.extend(order.iter().map(|(_, name)| csv_field(name)));
    let mut out = csv_line(header);

    for (row_id, row_name) in order {
        let mut fields = vec![csv_field(row_name)];
        for (col_id, _) in order {
            if row_id == col_id {
                fields.push(String::new());
            } else {
                let h = standings.head_to_head(*row_id, *col_id);
                fields.push(format!("{}-{}-{}", h.wins, h.draws, h.losses));
            }
        }
        out.push_str(&csv_line(fields));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameResult;
    use crate::standings::GameOutcome;

    fn row(rank: usize, name: &str, pts: f64, w: u32, d: u32, l: u32) -> ExportRow {
        ExportRow {
            rank,
            name: name.to_string(),
            version: String::new(),
            elo: 1500.0,
            elo_delta: 0.0,
            points: pts,
            games: w + d + l,
            wins: w,
            draws: d,
            losses: l,
            nps: None,
        }
    }

    #[test]
    fn standings_csv_has_header_and_rows() {
        let csv = standings_csv(&[row(1, "Alpha", 1.5, 1, 1, 0)]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Rank,Engine,Version,Elo,EloDelta,Points,Games,Wins,Draws,Losses,AvgNps");
        assert_eq!(lines[1], "1,Alpha,,1500.0,+0.0,1.5,2,1,1,0,");
        assert!(csv.ends_with("\r\n"));
    }

    #[test]
    fn fields_with_commas_are_quoted() {
        let csv = standings_csv(&[row(1, "Engine, v2", 0.0, 0, 0, 0)]);
        assert!(csv.contains("\"Engine, v2\""));
    }

    #[test]
    fn embedded_quotes_are_doubled() {
        let csv = standings_csv(&[row(1, "the \"best\"", 0.0, 0, 0, 0)]);
        assert!(csv.contains("\"the \"\"best\"\"\""));
    }

    #[test]
    fn crosstable_is_square_with_blank_diagonal() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        s.record(GameOutcome {
            white: a,
            black: b,
            result: GameResult::WhiteWin,
            white_nps: None,
            black_nps: None,
        });
        let order = vec![(a, "A".to_string()), (b, "B".to_string())];
        let csv = crosstable_csv(&order, &s);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], ",A,B");
        assert_eq!(lines[1], "A,,1-0-0"); // A beat B once
        assert_eq!(lines[2], "B,0-0-1,"); // B lost to A once
    }
}
