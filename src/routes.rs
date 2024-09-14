use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    github::{get_access_token, validate_github_oidc_jwt, GithubPr},
    AppState,
};

pub fn routes(app_state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/", get(home))
        .route("/create-pr", post(create_pr))
        .with_state(app_state)
}

pub async fn home() -> &'static str {
    "Hello, world!"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreatePrPayload {
    pub github_oidc_jwt: String,
    pub owner: String,
    pub repo: String,
    pub base_branch: String,
    pub head_branch: String,
    pub title: String,
    pub body: String,
}

#[axum_macros::debug_handler]
pub async fn create_pr(
    State(app_state): State<AppState>,
    Json(payload): Json<CreatePrPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if let Err(e) = validate_github_oidc_jwt(&payload.github_oidc_jwt).await {
        error!("Invalid GitHub OIDC JWT: {e:?}");

        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid GitHub OIDC JWT".to_string(),
        ));
    }

    let access_token = match get_access_token(&app_state).await {
        Ok(access_token) => access_token,
        Err(e) => {
            error!("Failed to get access token: {e}");

            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get access token: {}", e),
            ));
        }
    };

    let pr_to_create = GithubPr {
        title: payload.title,
        body: payload.body,
        head: payload.head_branch,
        base: payload.base_branch,
    };

    let pr = match crate::github::create_pr(
        &access_token,
        &payload.owner,
        &payload.repo,
        pr_to_create.clone(),
    )
    .await
    {
        Ok(pr) => pr,
        Err(e) => {
            error!("Failed to create PR: {e}");

            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create PR".to_string(),
            ));
        }
    };

    match crate::github::auto_merge_pr(
        &access_token,
        &payload.owner,
        &payload.repo,
        pr.number,
        &pr_to_create.title,
    )
    .await
    {
        Ok(()) => {}
        Err(e) => {
            error!("Failed to auto merge PR: {e}");

            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to auto merge PR".to_string(),
            ));
        }
    }

    Ok(Json(serde_json::json!({ "pr_number": pr.number })))
}
