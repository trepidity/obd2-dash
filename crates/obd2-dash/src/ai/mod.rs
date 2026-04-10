//! LLM-powered recording analysis pipeline.
//!
//! Sends recorded session data to a configurable LLM endpoint and parses
//! structured insights with severity levels back into the UI.

mod client;
mod config;
mod insights;
mod prompt;
mod summary;

pub use client::AiClient;
pub use config::AiConfig;
pub use insights::{AiInsights, Severity};
pub use summary::SessionSummary;
