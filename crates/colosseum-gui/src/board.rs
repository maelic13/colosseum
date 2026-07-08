//! Chess board rendering with the bundled cburnett SVG pieces
//! (CC BY-SA 3.0, see `assets/pieces/cburnett/LICENSE.md`).
//!
//! The board palette is wood-toned in both app themes — deep wood on the dark
//! theme, a brighter maple/oak pairing on the light theme so the board doesn't
//! sit like a dark slab on a light page. SVGs are rasterized by egui's image
//! loader at the exact display size, so the board is crisp at any scale —
//! `egui_extras::install_image_loaders` must have been called once.

use eframe::egui::{self, Color32, Rect, Ui, pos2, vec2};
use shakmaty::{Board, Color as ChessColor, File, Rank, Role, Square};

use crate::theme;

/// Light squares (dark theme).
const SQ_LIGHT_DARK_THEME: Color32 = Color32::from_rgb(0xC9, 0xB1, 0x8B);
/// Dark squares (dark theme).
const SQ_DARK_DARK_THEME: Color32 = Color32::from_rgb(0x6E, 0x57, 0x3D);
/// Light squares (light theme): bright maple.
const SQ_LIGHT_LIGHT_THEME: Color32 = Color32::from_rgb(0xF0, 0xDA, 0xB5);
/// Dark squares (light theme): warm oak.
const SQ_DARK_LIGHT_THEME: Color32 = Color32::from_rgb(0xB5, 0x88, 0x63);

/// The square pair for the active theme.
fn square_colors() -> (Color32, Color32) {
    if theme::is_dark() {
        (SQ_LIGHT_DARK_THEME, SQ_DARK_DARK_THEME)
    } else {
        (SQ_LIGHT_LIGHT_THEME, SQ_DARK_LIGHT_THEME)
    }
}

/// The piece image for one side/role (also used by the material row).
pub fn piece_source(color: ChessColor, role: Role) -> egui::ImageSource<'static> {
    use egui::include_image as img;
    match (color, role) {
        (ChessColor::White, Role::King) => img!("../assets/pieces/cburnett/wK.svg"),
        (ChessColor::White, Role::Queen) => img!("../assets/pieces/cburnett/wQ.svg"),
        (ChessColor::White, Role::Rook) => img!("../assets/pieces/cburnett/wR.svg"),
        (ChessColor::White, Role::Bishop) => img!("../assets/pieces/cburnett/wB.svg"),
        (ChessColor::White, Role::Knight) => img!("../assets/pieces/cburnett/wN.svg"),
        (ChessColor::White, Role::Pawn) => img!("../assets/pieces/cburnett/wP.svg"),
        (ChessColor::Black, Role::King) => img!("../assets/pieces/cburnett/bK.svg"),
        (ChessColor::Black, Role::Queen) => img!("../assets/pieces/cburnett/bQ.svg"),
        (ChessColor::Black, Role::Rook) => img!("../assets/pieces/cburnett/bR.svg"),
        (ChessColor::Black, Role::Bishop) => img!("../assets/pieces/cburnett/bB.svg"),
        (ChessColor::Black, Role::Knight) => img!("../assets/pieces/cburnett/bN.svg"),
        (ChessColor::Black, Role::Pawn) => img!("../assets/pieces/cburnett/bP.svg"),
    }
}

/// Paint a board (white at the bottom) into `rect`: squares, last-move
/// highlight, pieces, and file/rank labels when squares are large enough.
pub fn draw(ui: &mut Ui, rect: Rect, board: &Board, last_move: Option<(Option<Square>, Square)>) {
    let sq = rect.width().min(rect.height()) / 8.0;
    let origin = rect.min;
    let painter = ui.painter().clone();
    let (sq_light, sq_dark) = square_colors();

    for rank in 0..8u32 {
        for file in 0..8u32 {
            // rank 0 at the bottom (white's side).
            let square = Square::from_coords(File::new(file), Rank::new(rank));
            let x = origin.x + file as f32 * sq;
            let y = origin.y + (7 - rank) as f32 * sq;
            let cell = Rect::from_min_size(pos2(x, y), vec2(sq, sq));
            let dark = (rank + file) % 2 == 0;
            painter.rect_filled(cell, 0.0, if dark { sq_dark } else { sq_light });

            let highlighted =
                last_move.is_some_and(|(from, to)| from == Some(square) || to == square);
            if highlighted {
                painter.rect_filled(cell, 0.0, theme::accent().gamma_multiply(0.35));
            }

            if let Some(piece) = board.piece_at(square) {
                let inset = sq * 0.04;
                egui::Image::new(piece_source(piece.color, piece.role))
                    .paint_at(ui, cell.shrink(inset));
            }
        }
    }

    // Coordinate labels in the square corners, lichess-style, only when
    // there's room for them to be unobtrusive.
    if sq >= 34.0 {
        let font = egui::FontId::proportional((sq * 0.22).min(12.0));
        for file in 0..8u32 {
            let dark = file % 2 == 0; // a1 square color parity on rank 0
            let color = if dark { sq_light } else { sq_dark };
            painter.text(
                pos2(
                    origin.x + file as f32 * sq + sq * 0.08,
                    origin.y + 8.0 * sq - sq * 0.06,
                ),
                egui::Align2::LEFT_BOTTOM,
                char::from(b'a' + file as u8),
                font.clone(),
                color,
            );
        }
        for rank in 0..8u32 {
            let dark = rank % 2 == 0; // h-file square color parity
            let color = if dark { sq_light } else { sq_dark };
            painter.text(
                pos2(
                    origin.x + 8.0 * sq - sq * 0.08,
                    origin.y + (7 - rank) as f32 * sq + sq * 0.06,
                ),
                egui::Align2::RIGHT_TOP,
                char::from(b'1' + rank as u8),
                font.clone(),
                color,
            );
        }
    }
}
