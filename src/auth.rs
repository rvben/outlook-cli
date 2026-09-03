use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::error::AppError;
use crate::output::Output;

pub const READ_SCOPES: &str = "openid profile offline_access User.Read Mail.Read Calendars.Read";
pub const WRITE_SCOPES: &str =
    "openid profile offline_access User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite";
const SERVICE: &str = "outlook-cli";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
    message: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn entry(profile_name: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE, profile_name)
        .map_err(|error| AppError::Unexpected(format!("credential store is unavailable: {error}")))
}

pub fn has_token(profile_name: &str) -> bool {
    std::env::var("OUTLOOK_ACCESS_TOKEN").is_ok()
        || entry(profile_name)
            .and_then(|entry| {
                entry
                    .get_password()
                    .map_err(|error| AppError::Unexpected(error.to_string()))
            })
            .is_ok()
}

pub fn granted_scopes(profile_name: &str) -> Option<Vec<String>> {
    if std::env::var("OUTLOOK_ACCESS_TOKEN").is_ok() {
        return None;
    }
    load(profile_name).ok().and_then(|token| {
        token
            .scope
            .map(|scope| scope.split_whitespace().map(str::to_owned).collect())
    })
}

fn store(profile_name: &str, token: &TokenBundle) -> Result<(), AppError> {
    let encoded =
        serde_json::to_string(token).map_err(|error| AppError::Unexpected(error.to_string()))?;
    entry(profile_name)?
        .set_password(&encoded)
        .map_err(|error| {
            AppError::Unexpected(format!(
                "could not save credentials in the OS credential store: {error}"
            ))
        })
}

fn load(profile_name: &str) -> Result<TokenBundle, AppError> {
    let encoded = entry(profile_name)?
        .get_password()
        .map_err(|_| AppError::Auth("not signed in; run `outlook auth login`".into()))?;
    serde_json::from_str(&encoded).map_err(|_| {
        AppError::Auth("stored credential is unreadable; run `outlook auth login` again".into())
    })
}

pub fn logout(profile_name: &str) -> Result<(), AppError> {
    match entry(profile_name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(AppError::Unexpected(format!(
            "could not remove stored credentials: {error}"
        ))),
    }
}

pub async fn access_token(profile_name: &str, profile: &Profile) -> Result<String, AppError> {
    if let Ok(token) = std::env::var("OUTLOOK_ACCESS_TOKEN") {
        return Ok(token);
    }
    let bundle = load(profile_name)?;
    if bundle.expires_at > now() + 120 {
        return Ok(bundle.access_token);
    }
    let refresh = bundle.refresh_token.as_deref().ok_or_else(|| {
        AppError::Auth(
            "session expired and has no refresh credential; run `outlook auth login`".into(),
        )
    })?;
    let response = reqwest::Client::new()
        .post(token_url(profile))
        .form(&[
            ("client_id", profile.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("scope", bundle.scope.as_deref().unwrap_or(scopes(profile))),
        ])
        .send()
        .await
        .map_err(|error| {
            AppError::Unexpected(format!("sign-in service is unreachable: {error}"))
        })?;
    let mut replacement = parse_token_response(response).await?;
    if replacement.refresh_token.is_none() {
        replacement.refresh_token = bundle.refresh_token;
    }
    store(profile_name, &replacement)?;
    Ok(replacement.access_token)
}

pub async fn login(
    profile_name: &str,
    profile: &Profile,
    out: &Output,
) -> Result<TokenBundle, AppError> {
    let response = reqwest::Client::new()
        .post(format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode",
            profile.tenant
        ))
        .form(&[
            ("client_id", profile.client_id.as_str()),
            ("scope", scopes(profile)),
        ])
        .send()
        .await
        .map_err(|error| {
            AppError::Unexpected(format!("sign-in service is unreachable: {error}"))
        })?;
    if !response.status().is_success() {
        return Err(oauth_response_error(response).await);
    }
    let device: DeviceCode = response
        .json()
        .await
        .map_err(|error| AppError::Auth(format!("invalid device-code response: {error}")))?;
    out.note(device.message.clone().unwrap_or_else(|| {
        format!(
            "Open {} and enter {}",
            device.verification_uri, device.user_code
        )
    }));
    let deadline = now() + device.expires_in;
    let mut interval = device.interval.unwrap_or(5);
    while now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let response = reqwest::Client::new()
            .post(token_url(profile))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", profile.client_id.as_str()),
                ("device_code", device.device_code.as_str()),
            ])
            .send()
            .await
            .map_err(|error| AppError::Unexpected(error.to_string()))?;
        if response.status().is_success() {
            let bundle = parse_token_response(response).await?;
            store(profile_name, &bundle)?;
            return Ok(bundle);
        }
        let status = response.status();
        let error: OAuthError = response.json().await.unwrap_or(OAuthError {
            error: "unknown_error".into(),
            error_description: None,
        });
        match error.error.as_str() {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += 5;
                continue;
            }
            _ => {
                return Err(AppError::Auth(
                    error
                        .error_description
                        .unwrap_or_else(|| format!("{} ({status})", error.error)),
                ));
            }
        }
    }
    Err(AppError::Auth("device-code sign-in expired".into()))
}

fn scopes(profile: &Profile) -> &'static str {
    if profile.read_only {
        READ_SCOPES
    } else {
        WRITE_SCOPES
    }
}

fn token_url(profile: &Profile) -> String {
    format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        profile.tenant
    )
}

async fn parse_token_response(response: reqwest::Response) -> Result<TokenBundle, AppError> {
    if !response.status().is_success() {
        return Err(oauth_response_error(response).await);
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|error| AppError::Auth(format!("invalid token response: {error}")))?;
    Ok(TokenBundle {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now() + token.expires_in,
        scope: token.scope,
    })
}

async fn oauth_response_error(response: reqwest::Response) -> AppError {
    let status = response.status();
    let error: OAuthError = response.json().await.unwrap_or(OAuthError {
        error: "oauth_error".into(),
        error_description: None,
    });
    AppError::Auth(
        error
            .error_description
            .unwrap_or_else(|| format!("{} ({status})", error.error)),
    )
}
