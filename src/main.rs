use cja::{app_state::AppState as AS, color_eyre::eyre::Context as _, server::run_server};
use db::setup_db_pool;
use tracing::info;

mod cron;
mod db;
mod github;
mod jobs;
mod routes;

#[derive(Clone, Debug)]
pub struct GithubApp {
    pub client_id: String,
    pub client_secret: String,
    pub private_key: String,
    pub installation_id: String,
    pub app_id: String,
}

impl GithubApp {
    pub fn from_env() -> cja::Result<Self> {
        let client_id = std::env::var("GITHUB_APP_CLIENT_ID")?;
        let client_secret = std::env::var("GITHUB_APP_CLIENT_SECRET")?;
        let private_key = std::env::var("GITHUB_APP_PRIVATE_KEY")?;
        let installation_id = std::env::var("GITHUB_APP_INSTALLATION_ID")?;
        let app_id = std::env::var("GITHUB_APP_ID")?;

        Ok(Self {
            client_id,
            client_secret,
            private_key,
            installation_id,
            app_id,
        })
    }
}

#[derive(Clone, Debug)]
struct AppState {
    db: sqlx::PgPool,
    cookie_key: cja::server::cookies::CookieKey,
    github_app: GithubApp,
}

impl AS for AppState {
    fn db(&self) -> &sqlx::PgPool {
        &self.db
    }

    fn version(&self) -> &str {
        "dev"
    }

    fn cookie_key(&self) -> &cja::server::cookies::CookieKey {
        &self.cookie_key
    }
}

fn main() -> cja::Result<()> {
    let _sentry_guard = cja::setup::setup_sentry();

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?
        .block_on(_main())
}

async fn _main() -> cja::Result<()> {
    cja::setup::setup_tracing("knowledge")?;

    let db_pool = setup_db_pool().await.context("Failed to setup DB Pool")?;

    let cookie_key = cja::server::cookies::CookieKey::from_env_or_generate()?;

    let app_state = AppState {
        db: db_pool,
        cookie_key,
        github_app: GithubApp::from_env()?,
    };

    let app = routes::routes(app_state.clone());

    info!("Spawning Tasks");
    let mut futures = vec![
        tokio::spawn(run_server(app)),
        tokio::spawn(cja::jobs::worker::job_worker(app_state.clone(), jobs::Jobs)),
    ];
    if std::env::var("CRON_DISABLED").unwrap_or_else(|_| "false".to_string()) != "true" {
        info!("Cron Enabled");
        futures.push(tokio::spawn(cron::run_cron(app_state.clone())));
    }
    info!("Tasks Spawned");

    futures::future::try_join_all(futures).await?;

    Ok(())
}
