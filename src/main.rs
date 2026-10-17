use opencodetest::{config::Config, db, routes::app_router, seed, seed_demo, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "opencodetest=debug,tower_http=debug,sqlx=warn".into()),
        )
        .init();

    let config = Config::from_env();
    security_preflight(&config);
    let pool = db::connect(&config.database_url).await?;
    db::run_migrations(&pool).await?;
    if seed::ensure_admin(&pool, &config).await? {
        tracing::info!("seeded initial admin {}", config.admin_email);
    }
    if seed_demo::seed_demo(&pool).await? {
        tracing::info!("seeded demo dataset");
    }

    let state = AppState {
        pool,
        config: config.clone(),
    };
    let app = app_router(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
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
