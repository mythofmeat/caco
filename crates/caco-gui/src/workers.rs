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
