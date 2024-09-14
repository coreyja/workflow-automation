// require 'openssl'
// require 'jwt'  # https://rubygems.org/gems/jwt

// # Private key contents
// private_pem = File.read("YOUR_PATH_TO_PEM")
// private_key = OpenSSL::PKey::RSA.new(private_pem)

// # Generate the JWT
// payload = {
//   # issued at time, 60 seconds in the past to allow for clock drift
//   iat: Time.now.to_i - 60,
//   # JWT expiration time (10 minute maximum)
//   exp: Time.now.to_i + (10 * 60),

// # GitHub App's client ID
//   iss: "YOUR_CLIENT_ID"
// }

use eyre::Context;
use github_oidc::{fetch_jwks, GitHubOIDCConfig};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;

pub fn create_jwt(app_state: &AppState) -> cja::Result<String> {
    let client_id = app_state.github_app.client_id.clone();
    let private_pem = app_state.github_app.private_key.clone();
    let private_key = EncodingKey::from_rsa_pem(private_pem.as_bytes())?;
    let payload = json!({
      "iat": chrono::Utc::now().timestamp() - 60,
      "exp": chrono::Utc::now().timestamp() + (10 * 60),
      "iss": client_id,
    });
    let jwt = encode(&Header::new(Algorithm::RS256), &payload, &private_key)?;
    Ok(jwt)
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessTokenResponse {
    token: String,
}

pub async fn get_access_token(app_state: &AppState) -> cja::Result<String> {
    let installation_id = app_state.github_app.installation_id.clone();

    let jwt = create_jwt(app_state)?;
    let client = reqwest::Client::new();
    let res = client
        .post(format!(
            "https://api.github.com/app/installations/{}/access_tokens",
            installation_id
        ))
        .header("User-Agent", "workflow-automation")
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    let body = res.text().await?;
    let access_token = serde_json::from_str::<AccessTokenResponse>(&body)?.token;

    Ok(access_token)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GithubPr {
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GithubPrResponse {
    pub number: u64,
}

pub async fn create_pr(
    access_token: &str,
    owner: &str,
    repo: &str,
    pr: GithubPr,
) -> cja::Result<GithubPrResponse> {
    let client = reqwest::Client::new();
    let res = client
        .post(format!("https://api.github.com/repos/{owner}/{repo}/pulls"))
        .header("User-Agent", "workflow-automation")
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&pr)
        .send()
        .await?;
    let body = res.text().await?;
    println!("body: {}", body);
    let pr_response = serde_json::from_str::<GithubPrResponse>(&body)?;
    Ok(pr_response)
}

pub async fn validate_github_oidc_jwt(jwt: &str) -> cja::Result<()> {
    let jwks = fetch_jwks("https://token.actions.githubusercontent.com")
        .await
        .context("Failed to get JWTs")?;

    let config = GitHubOIDCConfig {
        audience: Some("https://github.com/coreyja".to_string()),
        repository: Some("coreyja/coreyja.com".to_string()),
        repository_owner: Some("coreyja".to_string()),
    };

    jwks.validate_github_token(jwt, &config)
        .wrap_err("Failed to validate JWT")?;
    Ok(())
}

pub async fn auto_merge_pr(
    access_token: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
    commit_headline: &str,
) -> cja::Result<()> {
    // GraphQL Mutation enablePullRequestAutoMerge
    let client = reqwest::Client::new();
    let pull_request_id = get_pull_request_id(access_token, owner, repo, pr_number).await?;
    let res = client.post("https://api.github.com/graphql")
        .header("User-Agent", "workflow-automation")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({
            "query": "mutation enablePullRequestAutoMerge($input: EnablePullRequestAutoMergeInput!) { enablePullRequestAutoMerge(input: $input) { clientMutationId } }",
            "variables": {
                "input": {
                    "pullRequestId": pull_request_id,
                    "mergeMethod": "SQUASH",
                    "commitBody": "Auto-merged by workflow-automation",
                    "commitHeadline": commit_headline,
                }
            }
        }))
        .send()
        .await?;

    let resp = res.json::<serde_json::Value>().await?;
    println!("resp: {}", resp);
    Ok(())
}

async fn get_pull_request_id(
    access_token: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
) -> cja::Result<String> {
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.github.com/graphql")
        .header("User-Agent", "workflow-automation")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({
            "query": "query pullRequest($owner: String!, $repo: String!, $prNumber: Int!) { repository(owner: $owner, name: $repo) { pullRequest(number: $prNumber) { id } } }",
            "variables": {
                "owner": owner,
                "repo": repo,
                "prNumber": pr_number,
            }
        }))
        .send()
        .await?;
    let json = res.json::<serde_json::Value>().await?;
    let pull_request_id = json["data"]["repository"]["pullRequest"]["id"]
        .as_str()
        .ok_or(eyre::eyre!("Failed to get pull request id"))?;

    Ok(pull_request_id.to_string())
}
