// Nested `if let` chains are common in TUI event/render code and are more
// readable than collapsed `if let .. && let ..` chains at deep indentation.
#![allow(clippy::collapsible_if, clippy::collapsible_else_if)]

pub mod app;
pub mod event;
pub mod input;
pub mod message;
pub mod screens;
pub mod theme;
pub mod widgets;
