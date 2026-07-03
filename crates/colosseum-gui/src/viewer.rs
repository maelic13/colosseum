//! A minimal built-in PGN/board viewer. Loads a stored game's PGN, replays the
//! SAN movetext with `shakmaty` to reconstruct the board after every ply, and
//! renders a board + move list in a floating window with step controls.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use shakmaty::{
    Board, CastlingMode, Chess, Color as ChessColor, File, Position, Rank, Role, Square,
    san::San,
};

use colosseum_engine::GameRow;

use crate::theme;

// Board palette (independent of the app theme so pieces stay legible).
const SQ_LIGHT: Color32 = Color32::from_rgb(0xC9, 0xB1, 0x8B);
const SQ_DARK: Color32 = Color32::from_rgb(0x6E, 0x57, 0x3D);
const PIECE_WHITE: Color32 = Color32::from_rgb(0xF7, 0xF2, 0xE7);
const PIECE_BLACK: Color32 = Color32::from_rgb(0x16, 0x14, 0x10);
const LAST_MOVE: Color32 = Color32::from_rgb(0xE0, 0xA9, 0x3b);

/// State for the floating game viewer. Closed and empty by default.
#[derive(Default)]
pub struct GameViewer {
    open: bool,
    header: String,
    result: String,
    sans: Vec<String>,
    /// Board after each ply; `boards[0]` is the start position.
    boards: Vec<Board>,
    /// (from, to) square for each played ply, aligned with `sans`.
    moves: Vec<(Option<Square>, Square)>,
    ply: usize,
    /// Set when the PGN could not be (fully) parsed.
    note: Option<String>,
}

impl GameViewer {
    /// Load a game and open the viewer. `white`/`black` are display names.
    pub fn open_game(&mut self, game: &GameRow, white: &str, black: &str) {
        let pgn = game.pgn.as_deref().unwrap_or("");
        let sans = extract_sans(pgn);

        let mut pos = initial_position(game.start_fen.as_deref());
        let mut boards = vec![pos.board().clone()];
        let mut moves = Vec::new();
        let mut played = Vec::new();
        let mut note = None;

        for s in &sans {
            let Ok(san) = s.parse::<San>() else {
                note = Some(format!("Stopped at unparsable move '{s}'."));
                break;
            };
            let Ok(m) = san.to_move(&pos) else {
                note = Some(format!("Stopped at illegal move '{s}'."));
                break;
            };
            moves.push((m.from(), m.to()));
            pos.play_unchecked(m);
            boards.push(pos.board().clone());
            played.push(s.clone());
        }

        let result = game
            .result
            .map(|r| r.pgn().to_string())
            .unwrap_or_else(|| "*".to_string());

        self.header = format!("{white} vs {black}  ·  round {}", game.round);
        self.result = result;
        self.ply = played.len(); // start at the final position
        self.sans = played;
        self.boards = boards;
        self.moves = moves;
        self.note = note;
        self.open = true;
    }

    /// Draw the viewer window. Call every frame from the app shell.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Game viewer")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 520.0])
            .show(ctx, |ui| self.body(ui));
        self.open = open;
    }

    fn body(&mut self, ui: &mut Ui) {
        ui.label(
            RichText::new(&self.header)
                .color(theme::TEXT)
                .font(theme::semibold(14.0)),
        );
        ui.label(
            RichText::new(format!("Result: {}", self.result))
                .color(theme::TEXT_WEAK)
                .size(12.5),
        );
        if let Some(note) = &self.note {
            ui.label(RichText::new(format!("⚠ {note}")).color(theme::WARN).size(12.0));
        }
        ui.add_space(6.0);

        // Keyboard navigation.
        let last = self.boards.len().saturating_sub(1);
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.ply = self.ply.saturating_sub(1);
            }
            if i.key_pressed(egui::Key::ArrowRight) && self.ply < last {
                self.ply += 1;
            }
            if i.key_pressed(egui::Key::Home) {
                self.ply = 0;
            }
            if i.key_pressed(egui::Key::End) {
                self.ply = last;
            }
        });

        ui.horizontal_top(|ui| {
            // Board on the left.
            self.draw_board(ui);

            ui.add_space(12.0);

            // Move list on the right.
            ui.vertical(|ui| {
                ui.set_min_width(150.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(360.0)
                    .show(ui, |ui| {
                        self.move_list(ui);
                    });
            });
        });

        ui.add_space(8.0);
        self.nav_bar(ui, last);
    }

    fn move_list(&mut self, ui: &mut Ui) {
        let mut target: Option<usize> = None;
        egui::Grid::new("viewer_moves")
            .num_columns(3)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                let mut i = 0;
                while i < self.sans.len() {
                    let move_no = i / 2 + 1;
                    ui.label(
                        RichText::new(format!("{move_no}."))
                            .color(theme::TEXT_FAINT)
                            .monospace(),
                    );
                    // White move (ply i+1 selects board after it).
                    if move_button(ui, &self.sans[i], self.ply == i + 1) {
                        target = Some(i + 1);
                    }
                    // Black move, if present.
                    if i + 1 < self.sans.len() {
                        if move_button(ui, &self.sans[i + 1], self.ply == i + 2) {
                            target = Some(i + 2);
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                    i += 2;
                }
            });
        if let Some(t) = target {
            self.ply = t;
        }
    }

    fn nav_bar(&mut self, ui: &mut Ui, last: usize) {
        ui.horizontal(|ui| {
            if ui.button(RichText::new("«").size(15.0)).on_hover_text("Start (Home)").clicked() {
                self.ply = 0;
            }
            if ui.button(RichText::new("‹").size(15.0)).on_hover_text("Previous (←)").clicked() {
                self.ply = self.ply.saturating_sub(1);
            }
            if ui.button(RichText::new("›").size(15.0)).on_hover_text("Next (→)").clicked()
                && self.ply < last
            {
                self.ply += 1;
            }
            if ui.button(RichText::new("»").size(15.0)).on_hover_text("End (End)").clicked() {
                self.ply = last;
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("ply {} / {}", self.ply, last))
                    .color(theme::TEXT_WEAK)
                    .size(12.5),
            );
        });
    }

    fn draw_board(&self, ui: &mut Ui) {
        let board = &self.boards[self.ply.min(self.boards.len() - 1)];
        let size = 320.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        let sq = size / 8.0;
        let painter = ui.painter_at(rect);

        // Squares highlighted by the move that led to the current position.
        let highlight = if self.ply >= 1 {
            self.moves.get(self.ply - 1).copied()
        } else {
            None
        };

        for row in 0..8u32 {
            let rank = 7 - row; // rank 8 (index 7) on top
            for col in 0..8u32 {
                let square = Square::from_coords(File::new(col), Rank::new(rank));
                let x = rect.left() + col as f32 * sq;
                let y = rect.top() + row as f32 * sq;
                let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(sq));

                let light = (col + rank) % 2 == 1;
                let mut fill = if light { SQ_LIGHT } else { SQ_DARK };
                if let Some((from, to)) = highlight
                    && (Some(square) == from || square == to)
                {
                    fill = fill.lerp_to_gamma(LAST_MOVE, 0.45);
                }
                painter.rect_filled(cell, 0.0, fill);

                if let Some(piece) = board.piece_at(square) {
                    let color = if piece.color == ChessColor::White {
                        PIECE_WHITE
                    } else {
                        PIECE_BLACK
                    };
                    painter.text(
                        cell.center(),
                        Align2::CENTER_CENTER,
                        piece_glyph(piece.role),
                        FontId::proportional(sq * 0.78),
                        color,
                    );
                }
            }
        }

        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, theme::STROKE),
            egui::StrokeKind::Inside,
        );
    }
}

/// A selectable move token; returns true when clicked.
fn move_button(ui: &mut Ui, san: &str, selected: bool) -> bool {
    let text = RichText::new(san)
        .monospace()
        .color(if selected { theme::BG_DARKEST } else { theme::TEXT });
    let btn = egui::Button::new(text)
        .fill(if selected {
            theme::ACCENT
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE);
    ui.add(btn).clicked()
}

/// Solid Unicode chess glyph for a role (tinted by piece color at draw time).
fn piece_glyph(role: Role) -> &'static str {
    match role {
        Role::King => "\u{265A}",
        Role::Queen => "\u{265B}",
        Role::Rook => "\u{265C}",
        Role::Bishop => "\u{265D}",
        Role::Knight => "\u{265E}",
        Role::Pawn => "\u{265F}",
    }
}

fn initial_position(start_fen: Option<&str>) -> Chess {
    match start_fen {
        None => Chess::default(),
        Some(fen) => fen
            .parse::<shakmaty::fen::Fen>()
            .ok()
            .and_then(|f| f.into_position(CastlingMode::Standard).ok())
            .unwrap_or_default(),
    }
}

/// Extract SAN move tokens from a PGN string: drop header lines, `{…}` comments,
/// `(…)` variations, NAGs, move numbers, and the result token.
fn extract_sans(pgn: &str) -> Vec<String> {
    let movetext: String = pgn
        .lines()
        .filter(|l| !l.trim_start().starts_with('['))
        .collect::<Vec<_>>()
        .join(" ");

    // Strip comments and variations (which may nest).
    let mut clean = String::with_capacity(movetext.len());
    let mut brace = 0u32;
    let mut paren = 0u32;
    for ch in movetext.chars() {
        match ch {
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            _ if brace > 0 || paren > 0 => {}
            _ => clean.push(ch),
        }
    }

    let mut moves = Vec::new();
    for tok in clean.split_whitespace() {
        if matches!(tok, "1-0" | "0-1" | "1/2-1/2" | "*") || tok.starts_with('$') {
            continue;
        }
        let san = strip_move_number(tok);
        if !san.is_empty() {
            moves.push(san.to_string());
        }
    }
    moves
}

/// Remove a leading move-number prefix (`12.` / `12...`). A bare number with no
/// following dot is not valid SAN, so it is dropped (returns "").
fn strip_move_number(tok: &str) -> &str {
    let bytes = tok.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return tok; // starts with a letter / 'O' — a real SAN move
    }
    let mut j = i;
    while j < bytes.len() && bytes[j] == b'.' {
        j += 1;
    }
    if j > i { &tok[j..] } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_moves_dropping_numbers_and_result() {
        let pgn = "[Event \"x\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n";
        assert_eq!(extract_sans(pgn), vec!["e4", "e5", "Nf3", "Nc6"]);
    }

    #[test]
    fn drops_comments_and_variations() {
        let pgn = "1. e4 {best by test} e5 (1... c5 2. Nf3) 2. Nf3 *";
        assert_eq!(extract_sans(pgn), vec!["e4", "e5", "Nf3"]);
    }

    #[test]
    fn replays_a_short_game() {
        let mut v = GameViewer::default();
        let game = GameRow {
            id: colosseum_core::GameId::new(),
            round: 1,
            white: colosseum_core::EngineId::new(),
            black: colosseum_core::EngineId::new(),
            result: None,
            termination: None,
            white_nps: None,
            black_nps: None,
            plies: None,
            pgn: Some("1. e4 e5 2. Nf3 Nc6 *".to_string()),
            status: "finished".to_string(),
            start_fen: None,
            opening_moves: Vec::new(),
        };
        v.open_game(&game, "A", "B");
        assert_eq!(v.sans.len(), 4);
        assert_eq!(v.boards.len(), 5); // start + 4 plies
        assert!(v.note.is_none());
        assert!(v.open);
    }
}
