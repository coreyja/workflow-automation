use axum::routing::get;

use crate::AppState;

pub fn routes(app_state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/", get(home))
        .with_state(app_state)
}

pub async fn home() -> &'static str {
    "Hello, world!"
}
