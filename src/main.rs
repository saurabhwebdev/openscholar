use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use opencodetest::auth::ratelimit::IpLimiter;
use opencodetest::{config::Config, db, routes::app_router, seed, seed_demo, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "opencodetest=info,tower_http=info,sqlx=warn".into()),
        )
        .init();

    let config = Config::from_env();
    security_preflight(&config);
    let pool = db::connect_with(&config.database_url, config.db_max_connections).await?;
    db::run_migrations(&pool).await?;
    if seed::ensure_admin(&pool, &config).await? {
        tracing::info!("seeded initial admin");
    }
    if seed_demo::seed_demo(&pool).await? {
        tracing::info!("seeded demo dataset");
    }

    // Periodically reclaim expired session rows so the table stays bounded.
    spawn_session_sweeper(pool.clone());

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        login_limiter: Arc::new(IpLimiter::new()),
    };
    let app = app_router(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // Drain the pool so in-flight DB work finishes before exit.
    pool.close().await;
    tracing::info!("shut down cleanly");
    Ok(())
}

/// Background task: purge expired sessions on boot and hourly thereafter.
fn spawn_session_sweeper(pool: sqlx::PgPool) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match db::purge_expired_sessions(&pool).await {
                Ok(n) if n > 0 => tracing::info!("purged {n} expired session(s)"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "session purge failed"),
            }
        }
    });
}

/// Resolve on Ctrl-C or SIGTERM (docker stop) for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

/// Loud, non-fatal warnings for insecure defaults, so a production deployment
/// that forgets to set secrets is impossible to miss in the logs.
fn security_preflight(config: &Config) {
    const DEFAULT_ADMIN_PASSWORD: &str = "changeme-admin-123";
    if config.admin_password == DEFAULT_ADMIN_PASSWORD {
        tracing::warn!(
            "SECURITY: ADMIN_PASSWORD is the published default — set a strong ADMIN_PASSWORD before exposing this server"
        );
    }
    if opencodetest::auth::middleware::configured_api_token().is_none() {
        tracing::warn!(
            "SECURITY: REST API bearer auth is DISABLED (API_TOKEN unset or left at the dev default) — set a strong API_TOKEN to enable /api/* bearer access"
        );
    }
    if !config.cookie_secure {
        tracing::warn!(
            "SECURITY: COOKIE_SECURE is off — session cookies are sent over plain HTTP; use this only for local development"
        );
    }
}
