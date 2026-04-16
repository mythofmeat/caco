//! Debounced filter query state.
//!
//! Splits the pure debounce decision from the side effects (reload, repaint)
//! so the timing logic can be unit tested without egui or a DB connection.

use std::time::{Duration, Instant};

/// Default debounce window (matches prior behaviour in `AppState`).
pub const DEFAULT_DEBOUNCE_MS: u64 = 150;

/// Input/applied/debounce state for the library filter bar.
#[derive(Debug, Clone)]
pub struct FilterQuery {
    /// Current user input (what's typed into the text box).
    pub input: String,
    /// Last value that was pushed through to the query engine.
    pub applied: String,
    /// When the input last changed; `None` when settled.
    pub changed_at: Option<Instant>,
    /// Debounce window.
    pub debounce: Duration,
}

/// Result of polling [`FilterQuery::poll`].
#[derive(Debug, PartialEq)]
pub enum FilterCheck {
    /// No pending change; caller does nothing.
    Idle,
    /// Debounce still active; caller should schedule a repaint after the returned duration.
    Pending { remaining: Duration },
    /// Debounce elapsed and the input differs from `applied` — caller should run the query.
    /// `applied` has already been updated.
    Apply,
}

impl Default for FilterQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterQuery {
    pub fn new() -> Self {
        Self::with_debounce_ms(DEFAULT_DEBOUNCE_MS)
    }

    pub fn with_debounce_ms(ms: u64) -> Self {
        Self {
            input: String::new(),
            applied: String::new(),
            changed_at: None,
            debounce: Duration::from_millis(ms),
        }
    }

    /// Mark the input as changed at `now` (typical caller passes `Instant::now()`).
    pub fn mark_changed(&mut self, now: Instant) {
        self.changed_at = Some(now);
    }

    /// Clear the input and mark as changed.
    pub fn clear(&mut self, now: Instant) {
        self.input.clear();
        self.changed_at = Some(now);
    }

    /// Force-set input and applied in one step (used when restoring a saved collection).
    pub fn set_both(&mut self, text: String) {
        self.input = text.clone();
        self.applied = text;
        self.changed_at = None;
    }

    /// Poll the debounce state. When [`FilterCheck::Apply`] is returned the
    /// struct has already promoted `input` → `applied`.
    pub fn poll(&mut self, now: Instant) -> FilterCheck {
        let Some(changed_at) = self.changed_at else {
            return FilterCheck::Idle;
        };
        let elapsed = now.saturating_duration_since(changed_at);
        if elapsed >= self.debounce {
            self.changed_at = None;
            if self.applied != self.input {
                self.applied = self.input.clone();
                FilterCheck::Apply
            } else {
                FilterCheck::Idle
            }
        } else {
            FilterCheck::Pending {
                remaining: self.debounce - elapsed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_when_never_changed() {
        let mut q = FilterQuery::new();
        assert_eq!(q.poll(Instant::now()), FilterCheck::Idle);
    }

    #[test]
    fn pending_before_debounce_elapses() {
        let mut q = FilterQuery::with_debounce_ms(150);
        let t0 = Instant::now();
        q.input = "doom".to_string();
        q.mark_changed(t0);
        let result = q.poll(t0 + Duration::from_millis(50));
        match result {
            FilterCheck::Pending { remaining } => {
                assert!(remaining >= Duration::from_millis(90));
                assert!(remaining <= Duration::from_millis(110));
            }
            other => panic!("expected Pending, got {other:?}"),
        }
        // applied should not have moved
        assert_eq!(q.applied, "");
    }

    #[test]
    fn applies_after_debounce_elapses() {
        let mut q = FilterQuery::with_debounce_ms(150);
        let t0 = Instant::now();
        q.input = "doom".to_string();
        q.mark_changed(t0);
        let result = q.poll(t0 + Duration::from_millis(200));
        assert_eq!(result, FilterCheck::Apply);
        assert_eq!(q.applied, "doom");
        assert!(q.changed_at.is_none());
    }

    #[test]
    fn idle_when_input_matches_applied_after_debounce() {
        let mut q = FilterQuery::with_debounce_ms(150);
        q.set_both("doom".to_string());
        let t0 = Instant::now();
        q.mark_changed(t0);
        let result = q.poll(t0 + Duration::from_millis(200));
        assert_eq!(result, FilterCheck::Idle);
        assert_eq!(q.applied, "doom");
    }

    #[test]
    fn typing_again_resets_debounce() {
        let mut q = FilterQuery::with_debounce_ms(150);
        let t0 = Instant::now();
        q.input = "do".to_string();
        q.mark_changed(t0);
        assert!(matches!(
            q.poll(t0 + Duration::from_millis(100)),
            FilterCheck::Pending { .. }
        ));
        // User types more; mark_changed resets the timer.
        q.input = "doom".to_string();
        q.mark_changed(t0 + Duration::from_millis(100));
        // Now, 100ms after the second change (200ms after t0) we should still be pending.
        assert!(matches!(
            q.poll(t0 + Duration::from_millis(200)),
            FilterCheck::Pending { .. }
        ));
        // 150ms after the second change the apply fires.
        assert_eq!(q.poll(t0 + Duration::from_millis(260)), FilterCheck::Apply);
        assert_eq!(q.applied, "doom");
    }

    #[test]
    fn clear_marks_changed() {
        let mut q = FilterQuery::with_debounce_ms(150);
        q.set_both("doom".to_string());
        let t0 = Instant::now();
        q.clear(t0);
        assert_eq!(q.input, "");
        assert!(q.changed_at.is_some());
        assert_eq!(q.poll(t0 + Duration::from_millis(200)), FilterCheck::Apply);
        assert_eq!(q.applied, "");
    }
}
