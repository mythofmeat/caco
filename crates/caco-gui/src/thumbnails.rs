use std::collections::{HashMap, HashSet};
use std::path::Path;

use egui::TextureHandle;

use crate::message::AppMessage;
use crate::workers::BackgroundSender;

/// Metadata needed for wiki scraping fallback.
pub struct ThumbnailHint {
    pub source_type: String,
    pub source_url: Option<String>,
    pub title: String,
}

/// Manages thumbnail textures for WAD cards.
///
/// Loading priority:
/// 1. Filesystem cache (`~/.cache/caco/thumbnails/{wad_id}.png`)
/// 2. TITLEPIC extraction from WAD file
/// 3. Doom Wiki image scraping (direct URL for doomwiki source, title search for others)
/// 4. Placeholder (rendered by grid view)
pub struct ThumbnailManager {
    textures: HashMap<i64, TextureHandle>,
    pending: HashSet<i64>,
    failed: HashSet<i64>,
}

impl Default for ThumbnailManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThumbnailManager {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
        }
    }

    /// Get the texture for a WAD, if already loaded.
    pub fn get(&self, wad_id: i64) -> Option<&TextureHandle> {
        self.textures.get(&wad_id)
    }

    /// Check if a WAD needs a thumbnail request (not loaded, pending, or failed).
    pub fn needs_request(&self, wad_id: i64) -> bool {
        !self.textures.contains_key(&wad_id)
            && !self.pending.contains(&wad_id)
            && !self.failed.contains(&wad_id)
    }

    /// Request thumbnail loading for a WAD.
    ///
    /// Tries in order: FS cache → TITLEPIC from WAD → wiki scrape → fail.
    /// Does nothing if already loaded, pending, or failed.
    pub fn request(
        &mut self,
        wad_id: i64,
        cached_path: Option<&Path>,
        hint: &ThumbnailHint,
        sender: &BackgroundSender,
    ) {
        if !self.needs_request(wad_id) {
            return;
        }

        self.pending.insert(wad_id);

        let path = cached_path.map(|p| p.to_path_buf());
        let sender = sender.clone();
        let source_type = hint.source_type.clone();
        let source_url = hint.source_url.clone();
        let title = hint.title.clone();

        std::thread::spawn(move || {
            let cache_dir = caco_core::config::thumbnail_cache_dir();
            let cache_path = cache_dir.join(format!("{wad_id}.png"));

            // 1. Filesystem cache
            if let Some((w, h, pixels)) = load_cached_thumbnail(&cache_path) {
                sender.send(AppMessage::ThumbnailReady {
                    wad_id,
                    width: w,
                    height: h,
                    pixels,
                });
                return;
            }

            // 2. TITLEPIC from WAD file
            if let Some(ref p) = path
                && let Some(pic) = caco_core::titlepic::extract_titlepic(p)
            {
                save_thumbnail_cache(&cache_path, pic.width, pic.height, &pic.pixels);
                sender.send(AppMessage::ThumbnailReady {
                    wad_id,
                    width: pic.width,
                    height: pic.height,
                    pixels: pic.pixels,
                });
                return;
            }

            // 3. Wiki scrape
            let wiki_bytes = if source_type == "doomwiki" {
                source_url
                    .as_deref()
                    .and_then(crate::wiki_scraper::fetch_wiki_image)
                    .or_else(|| crate::wiki_scraper::search_wiki_image(&title))
            } else {
                crate::wiki_scraper::search_wiki_image(&title)
            };

            if let Some(bytes) = wiki_bytes
                && let Ok(img) = image::load_from_memory(&bytes)
            {
                let rgba = img.to_rgba8();
                let w = rgba.width();
                let h = rgba.height();
                let pixels = rgba.into_raw();
                save_thumbnail_cache(&cache_path, w, h, &pixels);
                sender.send(AppMessage::ThumbnailReady {
                    wad_id,
                    width: w,
                    height: h,
                    pixels,
                });
                return;
            }

            // 4. Nothing found — mark as failed so we don't retry.
            sender.send(AppMessage::ThumbnailFailed { wad_id });
        });
    }

    /// Handle a ThumbnailReady message from the background thread.
    pub fn on_ready(
        &mut self,
        ctx: &egui::Context,
        wad_id: i64,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) {
        self.pending.remove(&wad_id);

        let image =
            egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], pixels);
        let texture = ctx.load_texture(
            format!("thumb_{wad_id}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(wad_id, texture);
    }

    /// Mark a WAD as failed (no thumbnail available). Prevents retry loops.
    pub fn mark_failed(&mut self, wad_id: i64) {
        self.pending.remove(&wad_id);
        self.failed.insert(wad_id);
    }

    /// Clear all cached textures (e.g., on library reload).
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.textures.clear();
        self.pending.clear();
        self.failed.clear();
    }
}

/// Load a PNG thumbnail from the filesystem cache, returning (width, height, rgba_pixels).
fn load_cached_thumbnail(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let data = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&data).ok()?;
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Save RGBA pixels as a PNG to the filesystem cache.
fn save_thumbnail_cache(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Some(img) = image::RgbaImage::from_raw(width, height, pixels.to_vec()) {
        let _ = img.save(path);
    }
}
