use serde_json::Value;
use std::time::Duration;

use super::{FetchOutcome, cache};
use crate::segments::external::{CommandResult, command_with_timeout};

const MAX_TOKEN_FILE_BYTES: u64 = 65_536;
const MAX_RESPONSE_BYTES: u64 = 65_536;

/// Extracts the token from an accessToken JSON blob, trimming whitespace.
fn token_from_json(contents: &str) -> Option<String> {
    let value: Value = serde_json::from_str(contents).ok()?;
    let token = value
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub fn obtain_token() -> Option<String> {
    if let Some(path) = std::env::var_os("PPULSE_USAGE_TOKEN_FILE") {
        return cache::read_bounded(std::path::Path::new(&path), MAX_TOKEN_FILE_BYTES)
            .and_then(|contents| token_from_json(&contents));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::Path::new(&home).join(".claude/.credentials.json");
        if let Some(contents) = cache::read_bounded(&path, MAX_TOKEN_FILE_BYTES) {
            return token_from_json(&contents);
        }
    }
    if cfg!(target_os = "macos") {
        if let CommandResult::Output(output) = command_with_timeout(
            "security",
            &["find-generic-password", "-s", "Claude Code-credentials", "-w"],
            None,
            5_000,
        ) {
            return token_from_json(&output);
        }
    }
    None
}

fn url_host(url: &str) -> &str {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let end = without_scheme
        .find(['/', ':'])
        .unwrap_or(without_scheme.len());
    &without_scheme[..end]
}

pub fn api_base() -> String {
    const DEFAULT: &str = "https://api.anthropic.com";
    let override_url = std::env::var("PPULSE_USAGE_API_URL").ok();
    let token_file = std::env::var_os("PPULSE_USAGE_TOKEN_FILE");
    match (override_url, token_file) {
        (Some(url), Some(_)) if matches!(url_host(&url), "127.0.0.1" | "localhost") => {
            url.trim_end_matches('/').to_string()
        }
        _ => DEFAULT.to_string(),
    }
}

pub fn fetch_usage(token: &str) -> FetchOutcome {
    let url = format!("{}/api/oauth/usage", api_base());
    let cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(cfg);
    let mut response = match agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
    {
        Ok(response) => response,
        Err(_) => return FetchOutcome::OtherFailure,
    };
    let status = response.status().as_u16();
    match status {
        200..=299 => {
            let Ok(bytes) = response
                .body_mut()
                .with_config()
                .limit(MAX_RESPONSE_BYTES)
                .read_to_vec()
            else {
                return FetchOutcome::OtherFailure;
            };
            serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| super::parse_limits(&value))
                .map(FetchOutcome::Success)
                .unwrap_or(FetchOutcome::OtherFailure)
        }
        429 => FetchOutcome::RateLimited(
            response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok()),
        ),
        401 | 403 => FetchOutcome::AuthFailed,
        _ => FetchOutcome::OtherFailure,
    }
}
