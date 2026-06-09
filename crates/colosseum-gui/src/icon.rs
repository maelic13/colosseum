//! Procedurally-drawn application icon — no image assets or extra dependencies.
//!
//! The emblem is a warm gold amphitheatre (two concentric rings, the arena seen
//! from above) on a dark rounded square, matching the app theme. Drawn with
//! antialiased coverage so it stays crisp from 16 px to 256 px.

use std::sync::Arc;

use eframe::egui::IconData;

/// Build the application icon as RGBA pixels.
#[must_use]
pub fn icon() -> Arc<IconData> {
    const SIZE: u32 = 256;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    // Theme colors (kept local to avoid coupling the icon to the theme module).
    let bg = [0x1a, 0x1e, 0x26]; // panel slate
    let gold = [0xe0, 0xa9, 0x3b];
    let gold_dim = [0xb8, 0x88, 0x2f];

    let s = SIZE as f32;
    let c = s / 2.0;

    // Rounded-square background parameters.
    let half = s / 2.0 - 6.0; // inset from edge
    let corner = 48.0;

    // Two ring radii (outer arena wall, inner arena wall) + stroke half-widths.
    let r_outer = s * 0.34;
    let r_inner = s * 0.185;
    let stroke_outer = s * 0.052;
    let stroke_inner = s * 0.040;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            // Background coverage (rounded square, antialiased).
            let bg_cov = rounded_rect_coverage(px - c, py - c, half, half, corner);
            if bg_cov <= 0.0 {
                continue; // fully transparent outside the rounded square
            }

            // Start from the background slate.
            let mut r = bg[0] as f32;
            let mut g = bg[1] as f32;
            let mut b = bg[2] as f32;

            let dist = ((px - c).powi(2) + (py - c).powi(2)).sqrt();

            // Outer ring (brighter gold).
            let outer_cov = ring_coverage(dist, r_outer, stroke_outer);
            blend(&mut r, &mut g, &mut b, gold, outer_cov);

            // Inner ring (slightly dimmer for depth).
            let inner_cov = ring_coverage(dist, r_inner, stroke_inner);
            blend(&mut r, &mut g, &mut b, gold_dim, inner_cov);

            let idx = ((y * SIZE + x) * 4) as usize;
            rgba[idx] = r.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 1] = g.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 2] = b.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 3] = (bg_cov * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }

    Arc::new(IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    })
}

/// Alpha coverage `[0,1]` for a point at offset `(dx, dy)` from a rounded
/// rectangle's center, with half-extents `(hx, hy)` and corner `radius`.
fn rounded_rect_coverage(dx: f32, dy: f32, hx: f32, hy: f32, radius: f32) -> f32 {
    // Signed distance to a rounded box (negative inside).
    let qx = dx.abs() - (hx - radius);
    let qy = dy.abs() - (hy - radius);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    let outside = (ax * ax + ay * ay).sqrt();
    let inside = qx.max(qy).min(0.0);
    let sd = outside + inside - radius;
    // Convert signed distance to ~1px antialiased coverage.
    (0.5 - sd).clamp(0.0, 1.0)
}

/// Alpha coverage `[0,1]` for a ring at `radius` with the given stroke
/// half-width, evaluated at distance `dist` from the center.
fn ring_coverage(dist: f32, radius: f32, half_width: f32) -> f32 {
    let d = (dist - radius).abs() - half_width;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Alpha-composite `color` (with coverage `a`) over the running `(r,g,b)`.
fn blend(r: &mut f32, g: &mut f32, b: &mut f32, color: [u8; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    *r = *r * (1.0 - a) + color[0] as f32 * a;
    *g = *g * (1.0 - a) + color[1] as f32 * a;
    *b = *b * (1.0 - a) + color[2] as f32 * a;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_dimensions() {
        let icon = icon();
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }

    #[test]
    fn center_is_opaque_and_corners_transparent() {
        let icon = icon();
        let at = |x: u32, y: u32| -> u8 {
            let idx = ((y * 256 + x) * 4 + 3) as usize;
            icon.rgba[idx]
        };
        // Corners are outside the rounded square -> transparent.
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(255, 0), 0);
        // Center is inside -> opaque.
        assert_eq!(at(128, 128), 255);
    }
}
