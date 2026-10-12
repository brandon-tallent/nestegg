//! Structured logging setup for Monarch Money scraper

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize the tracing subscriber with JSON formatting for container environments
/// or pretty formatting for local development
pub fn init(json_format: bool) {
    // sqlx logs every statement over 1s in full (for the schema, the whole
    // file) and every "already exists, skipping" notice from re-applying it
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,sqlx::query=error"));

    if json_format {
        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json());
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set subscriber");
    } else {
        let subscriber = tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty());
        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set subscriber");
    }
}
