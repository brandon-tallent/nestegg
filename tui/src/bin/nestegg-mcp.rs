//! Persistent Streamable HTTP MCP service, deployed separately from the scraper.
use anyhow::{Context, Result};
use axum::{http::StatusCode, routing::get, Router};
use nestegg::mcp::NesteggMcp;
use rmcp::transport::streamable_http_server::{
    session::never::NeverSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let url =
        std::env::var("DATABASE_URL").context("Set DATABASE_URL to a read-only mirror role")?;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only = on")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("SET statement_timeout = '15s'")
                    .execute(&mut *c)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .context("Could not connect to the Nestegg mirror")?;
    let bind = std::env::var("NESTEGG_MCP_BIND").unwrap_or_else(|_| "127.0.0.1:8091".into());
    let hosts = std::env::var("NESTEGG_MCP_ALLOWED_HOSTS")
        .unwrap_or_else(|_| "localhost,127.0.0.1,::1".into());
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(
            hosts
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect::<Vec<_>>(),
        )
        .with_cancellation_token(ct.child_token());
    let template = NesteggMcp::new(pool.clone());
    let service = StreamableHttpService::new(
        move || Ok(template.clone()),
        NeverSessionManager::default().into(),
        config,
    );
    let app = Router::new()
        .nest_service("/mcp", service)
        // Native MCP clients send no Origin. rmcp 1.6 treats an empty origin
        // allowlist as unrestricted, so explicitly reject browser-origin calls.
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                if request.headers().contains_key(axum::http::header::ORIGIN) {
                    Err(StatusCode::FORBIDDEN)
                } else {
                    Ok(next.run(request).await)
                }
            },
        ))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route(
            "/readyz",
            get(move || {
                let pool = pool.clone();
                async move {
                    match sqlx::query("SELECT 1 FROM latest_run LIMIT 1")
                        .execute(&pool)
                        .await
                    {
                        Ok(_) => StatusCode::OK,
                        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind,"Nestegg MCP listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=term.recv()=>{} }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}
