use std::path::PathBuf;
use std::sync::mpsc;

use crate::message::AppMessage;

/// Channel for background threads to send messages back to the UI.
///
/// Wraps `mpsc` + `egui::Context` so workers can trigger a repaint after
/// sending a message.
pub struct BackgroundChannel {
    pub tx: mpsc::Sender<AppMessage>,
    pub rx: mpsc::Receiver<AppMessage>,
    ctx: Option<egui::Context>,
}

impl Default for BackgroundChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundChannel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, ctx: None }
    }

    /// Store the egui context for triggering repaints from background threads.
    pub fn set_ctx(&mut self, ctx: egui::Context) {
        self.ctx = Some(ctx);
    }

    /// Create a sender that can be moved into a background thread.
    pub fn sender(&self) -> BackgroundSender {
        BackgroundSender {
            tx: self.tx.clone(),
            ctx: self.ctx.clone(),
        }
    }

    /// Drain all pending messages from background threads.
    pub fn drain(&self) -> Vec<AppMessage> {
        let mut msgs = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            msgs.push(msg);
        }
        msgs
    }
}

/// A clonable sender that can be moved into background threads.
#[derive(Clone)]
pub struct BackgroundSender {
    tx: mpsc::Sender<AppMessage>,
    ctx: Option<egui::Context>,
}

impl BackgroundSender {
    pub fn send(&self, msg: AppMessage) {
        let _ = self.tx.send(msg);
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }
}

// ---------------------------------------------------------------------------
// File dialog off-thread support
//
// `rfd::FileDialog::pick_file()` / `save_file()` blocks until the user
// responds, which freezes the egui update loop. The helpers below spawn a
// worker thread that owns the dialog and post the result back through a
// per-call-site `mpsc` channel; callers poll via `try_recv` each frame.
// ---------------------------------------------------------------------------

/// Receiver returned by [`spawn_file_dialog`]. A single message will arrive
/// once the user confirms or cancels the picker.
pub type FileDialogReceiver = mpsc::Receiver<Option<PathBuf>>;

/// Open vs. save picker.
#[derive(Clone, Debug)]
pub enum FileDialogKind {
    /// Pick an existing file.
    Open,
    /// Save-as picker; `default_filename` is pre-filled.
    Save,
}

/// Description of a file-picker invocation. Built up and passed to
/// [`spawn_file_dialog`].
#[derive(Clone, Debug)]
pub struct FileDialogRequest {
    /// Filter entries: `(label, [ext, ext, ...])` without the leading dot.
    pub filters: Vec<(String, Vec<String>)>,
    /// Initial directory shown in the picker.
    pub start_dir: Option<PathBuf>,
    /// Pre-filled filename (only honoured for save dialogs).
    pub default_filename: Option<String>,
    /// Open or save picker.
    pub kind: FileDialogKind,
}

impl FileDialogRequest {
    /// Build a new open-picker request with no filters or defaults.
    pub fn open() -> Self {
        Self {
            filters: Vec::new(),
            start_dir: None,
            default_filename: None,
            kind: FileDialogKind::Open,
        }
    }

    /// Build a new save-picker request with no filters or defaults.
    pub fn save() -> Self {
        Self {
            filters: Vec::new(),
            start_dir: None,
            default_filename: None,
            kind: FileDialogKind::Save,
        }
    }

    pub fn add_filter(mut self, label: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push((
            label.into(),
            extensions.iter().map(|s| (*s).to_string()).collect(),
        ));
        self
    }

    pub fn set_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.start_dir = Some(dir.into());
        self
    }

    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.default_filename = Some(name.into());
        self
    }
}

/// Spawn the file dialog on a worker thread. Returns a receiver that yields
/// exactly one message: `Some(path)` if the user chose a file, `None` if they
/// cancelled. `ctx` is used to request a repaint so the polling loop notices
/// the result promptly.
pub fn spawn_file_dialog(ctx: Option<egui::Context>, req: FileDialogRequest) -> FileDialogReceiver {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new();
        for (label, exts) in &req.filters {
            let ext_refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
            dialog = dialog.add_filter(label, &ext_refs);
        }
        if let Some(dir) = &req.start_dir {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = &req.default_filename {
            dialog = dialog.set_file_name(name);
        }
        let result = match req.kind {
            FileDialogKind::Open => dialog.pick_file(),
            FileDialogKind::Save => dialog.save_file(),
        };
        let _ = tx.send(result);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    rx
}
