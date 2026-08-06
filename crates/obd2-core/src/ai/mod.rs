#[cfg(feature = "ai")]
mod client;
mod config;
mod insights;
mod prompt;
mod summary;

#[cfg(feature = "ai")]
pub use client::AiClient;
pub use config::{AiConfig, AiProvider};
pub use insights::{AiInsights, InsightSection, Severity};
pub use summary::SessionSummary;
