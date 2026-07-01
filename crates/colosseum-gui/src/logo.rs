// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine logo storage and a lazily-populated texture cache.
//!
//! A user-chosen logo image is copied into the GUI's `logos/` data directory
//! (so it survives deletion of the original), and a unique file name is stored
//! in the engine's `meta.extra["logo"]`. Textures are decoded on first use and
//! cached by file path; the natural pixel size is kept so callers can fit the
//! image into a box without distorting or cropping it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2, pos2};
use image::RgbaImage;
use image::imageops::FilterType;

use colosseum_core::EngineId;

/// Lazily-loaded cache of logo images and per-size textures.
///
/// Source logos are often large (1000+ px) while the UI draws them at 34–96 pt,
/// so GPU linear minification alone produces badly aliased results. Instead the
/// decoded image is kept once, and for every distinct *physical-pixel* target
/// size a Lanczos-downscaled texture is uploaded and cached. Targets are the
/// handful of fixed slot sizes the UI uses, so the cache stays tiny.
///
/// A stored `None` records "missing / undecodable" so we don't retry decoding
/// every frame. Unique import file names mean a replaced logo gets a fresh key
/// automatically.
#[derive(Default)]
pub struct LogoCache {
    /// Decoded source images, keyed by file path.
    sources: HashMap<String, Option<Arc<RgbaImage>>>,
    /// Uploaded textures, keyed by (file path, target pixel width, height).
    textures: HashMap<(String, u32, u32), TextureHandle>,
}

impl LogoCache {
    /// Natural pixel dimensions of the logo at `path`, if it decodes.
    /// Lets callers shape the display slot to the image's aspect ratio.
    pub fn natural_size(&mut self, path: &Path) -> Option<Vec2> {
        self.source(path)
            .map(|img| Vec2::new(img.width() as f32, img.height() as f32))
    }

    fn source(&mut self, path: &Path) -> Option<Arc<RgbaImage>> {
        let key = path.to_string_lossy().to_string();
        if let Some(entry) = self.sources.get(&key) {
            return entry.clone();
        }
        let loaded = decode_logo(path);
        self.sources.insert(key, loaded.clone());
        loaded
    }

    /// A texture of the logo resized (aspect-fit, high quality) to fill a
    /// `box_px`-sized box in physical pixels. Never upscales past native size.
    /// Returns the texture and its size in physical pixels.
    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        box_px: Vec2,
    ) -> Option<(TextureHandle, Vec2)> {
        let src = self.source(path)?;
        let (nw, nh) = (src.width() as f32, src.height() as f32);
        let scale = (box_px.x / nw).min(box_px.y / nh).min(1.0);
        let tw = ((nw * scale).round() as u32).max(1);
        let th = ((nh * scale).round() as u32).max(1);

        let key = (path.to_string_lossy().to_string(), tw, th);
        if let Some(tex) = self.textures.get(&key) {
            return Some((tex.clone(), Vec2::new(tw as f32, th as f32)));
        }

        let resized: RgbaImage = if tw == src.width() && th == src.height() {
            (*src).clone()
        } else {
            image::imageops::resize(&*src, tw, th, FilterType::Lanczos3)
        };
        let color =
            ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], resized.as_raw());
        let tex = ctx.load_texture(
            format!("logo:{}:{tw}x{th}", key.0),
            color,
            TextureOptions::LINEAR,
        );
        self.textures.insert(key, tex.clone());
        Some((tex, Vec2::new(tw as f32, th as f32)))
    }
}

fn decode_logo(path: &Path) -> Option<Arc<RgbaImage>> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    if img.width() == 0 || img.height() == 0 {
        return None;
    }
    Some(Arc::new(img))
}

/// Draw the logo at `path` fitted (aspect-preserving, never cropped or squished
/// into a square) inside `rect`, with its corners rounded by `corner_radius`
/// (so square images on opaque backgrounds don't poke square corners out from
/// under rounded frames). The texture is resized on the CPU to match the
/// physical pixel size exactly and the draw rect is snapped to the pixel grid,
/// so the result stays crisp at any DPI. Returns `true` if a logo was drawn,
/// `false` if the caller should fall back to a monogram avatar.
pub fn draw_fitted(
    ui: &mut Ui,
    cache: &mut LogoCache,
    path: &Path,
    rect: Rect,
    corner_radius: u8,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let box_px = rect.size() * ppp;
    let Some((texture, size_px)) = cache.texture_for(&ctx, path, box_px) else {
        return false;
    };

    // Fit the draw size to the slot (upscaling small sources so they fill the
    // available space; the texture itself is never upscaled past native, the
    // GPU stretches it).
    let mut size_pt = size_px / ppp;
    let fill_scale = (rect.width() / size_pt.x).min(rect.height() / size_pt.y);
    if fill_scale > 1.0 {
        size_pt *= fill_scale;
    }
    let center = rect.center();
    let min = pos2(
        ((center.x - size_pt.x / 2.0) * ppp).round() / ppp,
        ((center.y - size_pt.y / 2.0) * ppp).round() / ppp,
    );
    let img_rect = Rect::from_min_size(min, size_pt);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().add(
        egui::epaint::RectShape::filled(
            img_rect,
            egui::CornerRadius::same(corner_radius),
            Color32::WHITE,
        )
        .with_texture(texture.id(), uv),
    );
    true
}

/// Allocate a square `size`×`size` slot with the given `sense`. Returns the slot
/// rect + response; the caller paints the logo or avatar into the rect.
pub fn slot(ui: &mut Ui, size: f32, sense: Sense) -> (Rect, egui::Response) {
    ui.allocate_exact_size(Vec2::splat(size), sense)
}

/// Copy `src` into `logos_dir` as `<engine_id>-<millis>.<ext>` (a unique name so
/// a replacement reloads cleanly), removing any prior logo for that id, and
/// return the stored file name to save in `meta.extra["logo"]`.
pub fn import(logos_dir: &Path, id: EngineId, src: &Path) -> std::io::Result<String> {
    std::fs::create_dir_all(logos_dir)?;
    remove(logos_dir, id);

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("{id}-{millis}.{ext}");
    std::fs::copy(src, logos_dir.join(&file_name))?;
    Ok(file_name)
}

/// Delete the stored logo file(s) for `id`, if any.
pub fn remove(logos_dir: &Path, id: EngineId) {
    let prefix = format!("{id}-");
    if let Ok(entries) = std::fs::read_dir(logos_dir) {
        for e in entries.flatten() {
            let matches = e
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with(&prefix));
            if matches {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
