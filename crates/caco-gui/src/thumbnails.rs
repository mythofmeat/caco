use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use egui::TextureHandle;

use crate::message::AppMessage;
use crate::workers::BackgroundSender;

type ThumbJob = Box<dyn FnOnce() + Send + 'static>;

/// Bounded worker pool for thumbnail jobs.
///
/// Scrolling a large grid can otherwise spawn hundreds of concurrent threads.
/// Cap at min(available_parallelism, 4) — grid renders don't benefit from more.
static POOL: OnceLock<mpsc::Sender<ThumbJob>> = OnceLock::new();

fn pool_submit(job: ThumbJob) {
    let tx = POOL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ThumbJob>();
        let rx = Arc::new(Mutex::new(rx));
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(4);
        for _ in 0..workers {
            let rx = Arc::clone(&rx);
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let guard = match rx.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        guard.recv()
                    };
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                }
            });
        }
        tx
    });
    let _ = tx.send(job);
}

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

        pool_submit(Box::new(move || {
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
        }));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_request_defaults_true() {
        let tm = ThumbnailManager::new();
        assert!(tm.needs_request(1));
    }

    #[test]
    fn needs_request_false_when_pending() {
        let mut tm = ThumbnailManager::new();
        tm.pending.insert(7);
        assert!(!tm.needs_request(7));
        assert!(tm.needs_request(8));
    }

    #[test]
    fn needs_request_false_when_failed() {
        let mut tm = ThumbnailManager::new();
        tm.failed.insert(9);
        assert!(!tm.needs_request(9));
    }

    #[test]
    fn mark_failed_transitions_pending_to_failed() {
        let mut tm = ThumbnailManager::new();
        tm.pending.insert(3);
        tm.mark_failed(3);
        assert!(!tm.pending.contains(&3));
        assert!(tm.failed.contains(&3));
        assert!(!tm.needs_request(3));
    }

    #[test]
    fn clear_resets_all_state() {
        let mut tm = ThumbnailManager::new();
        tm.pending.insert(1);
        tm.failed.insert(2);
        tm.clear();
        assert!(tm.pending.is_empty());
        assert!(tm.failed.is_empty());
        assert!(tm.needs_request(1));
        assert!(tm.needs_request(2));
    }
}
