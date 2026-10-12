use anyhow::{Context, Result};
use clap::Parser;
use monarch_scraper::{
    api::{router, ApiState},
    Config, Database,
};
use std::sync::Arc;
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();
    let token = std::env::var("NESTEGG_API_TOKEN").context("Set NESTEGG_API_TOKEN")?;
    anyhow::ensure!(
        token.len() >= 32,
        "NESTEGG_API_TOKEN must be at least 32 characters"
    );
    let db = Database::connect(config.database.as_deref().context("Set DATABASE_URL")?).await?;
    let state = Arc::new(ApiState {
        config,
        db,
        token,
        writes: tokio::sync::Mutex::new(()),
    });
    let bind = std::env::var("NESTEGG_API_BIND").unwrap_or_else(|_| "127.0.0.1:30803".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("Nestegg write API listening on {bind}");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
