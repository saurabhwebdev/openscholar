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
