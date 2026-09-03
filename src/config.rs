use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub const DEFAULT_TENANT: &str = "common";
/// The maintained multitenant public-client registration shipped with outlook-cli.
pub const DEFAULT_CLIENT_ID: &str = "6b126a3c-899a-4767-a88f-120522bae38b";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub client_id: String,
    #[serde(default = "default_tenant")]
    pub tenant: String,
    #[serde(default)]
    pub read_only: bool,
}

impl Profile {
    pub fn require_writable(&self) -> Result<(), AppError> {
        if self.read_only {
            Err(AppError::ReadOnly("read-only mode is enabled (unset OUTLOOK_READ_ONLY or disable read_only in the active profile to allow writes)".into()))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    active_profile: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

fn default_tenant() -> String {
    DEFAULT_TENANT.into()
}

pub fn path() -> PathBuf {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(base).join("outlook").join("config.toml")
    } else {
        config_base().join("outlook").join("config.toml")
    }
}

#[cfg(windows)]
fn config_base() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"))
}

#[cfg(not(windows))]
fn config_base() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".config"))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

pub fn save(profile_name: &str, profile: Profile) -> Result<PathBuf, AppError> {
    let path = path();
    let mut config = read_file()?;
    config.profiles.insert(profile_name.into(), profile);
    config.active_profile = Some(profile_name.into());
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Unexpected("configuration path has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let body =
        toml::to_string_pretty(&config).map_err(|error| AppError::Unexpected(error.to_string()))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".config-")
        .suffix(".toml.tmp")
        .tempfile_in(parent)?;
    temp.write_all(body.as_bytes())?;
    temp.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o600))?;
    }
    temp.persist(&path).map_err(|error| error.error)?;
    Ok(path)
}

pub fn load(requested: Option<&str>) -> Result<(String, Profile), AppError> {
    let config = read_file()?;
    let name = requested
        .map(str::to_owned)
        .or_else(|| std::env::var("OUTLOOK_PROFILE").ok())
        .or(config.active_profile)
        .unwrap_or_else(|| "default".into());
    let stored = config.profiles.get(&name);
    let client_id = std::env::var("OUTLOOK_CLIENT_ID")
        .ok()
        .or_else(|| stored.map(|profile| profile.client_id.clone()))
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "profile '{name}' is not configured; run `outlook init` or set OUTLOOK_CLIENT_ID"
            ))
        })?;
    let tenant = std::env::var("OUTLOOK_TENANT")
        .ok()
        .or_else(|| stored.map(|profile| profile.tenant.clone()))
        .unwrap_or_else(default_tenant);
    let read_only = match std::env::var("OUTLOOK_READ_ONLY") {
        Ok(value) => parse_bool("OUTLOOK_READ_ONLY", &value)?,
        Err(std::env::VarError::NotPresent) => stored.is_some_and(|profile| profile.read_only),
        Err(error) => {
            return Err(AppError::InvalidInput(format!(
                "cannot read OUTLOOK_READ_ONLY: {error}"
            )));
        }
    };
    Ok((
        name,
        Profile {
            client_id,
            tenant,
            read_only,
        },
    ))
}

pub fn load_or_initialize(requested: Option<&str>) -> Result<(String, Profile, bool), AppError> {
    let config = read_file()?;
    let name = requested
        .map(str::to_owned)
        .or_else(|| std::env::var("OUTLOOK_PROFILE").ok())
        .or(config.active_profile)
        .unwrap_or_else(|| "default".into());
    if config.profiles.contains_key(&name) {
        let (name, profile) = load(Some(&name))?;
        return Ok((name, profile, false));
    }

    let client_id = match std::env::var("OUTLOOK_CLIENT_ID") {
        Ok(value) if value.trim().is_empty() => {
            return Err(AppError::InvalidInput(
                "OUTLOOK_CLIENT_ID cannot be empty".into(),
            ));
        }
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => DEFAULT_CLIENT_ID.into(),
        Err(error) => {
            return Err(AppError::InvalidInput(format!(
                "cannot read OUTLOOK_CLIENT_ID: {error}"
            )));
        }
    };
    let tenant = match std::env::var("OUTLOOK_TENANT") {
        Ok(value) if value.trim().is_empty() => {
            return Err(AppError::InvalidInput(
                "OUTLOOK_TENANT cannot be empty".into(),
            ));
        }
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => default_tenant(),
        Err(error) => {
            return Err(AppError::InvalidInput(format!(
                "cannot read OUTLOOK_TENANT: {error}"
            )));
        }
    };
    let read_only = match std::env::var("OUTLOOK_READ_ONLY") {
        Ok(value) => parse_bool("OUTLOOK_READ_ONLY", &value)?,
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => {
            return Err(AppError::InvalidInput(format!(
                "cannot read OUTLOOK_READ_ONLY: {error}"
            )));
        }
    };
    let profile = Profile {
        client_id,
        tenant,
        read_only,
    };
    save(&name, profile.clone())?;
    Ok((name, profile, true))
}

pub fn configured_profile(requested: Option<&str>) -> Option<(String, Profile)> {
    load(requested).ok()
}

fn parse_bool(name: &str, value: &str) -> Result<bool, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(AppError::InvalidInput(format!(
            "{name} must be true or false"
        ))),
    }
}

fn read_file() -> Result<ConfigFile, AppError> {
    match std::fs::read_to_string(path()) {
        Ok(body) => toml::from_str(&body).map_err(|error| {
            AppError::InvalidInput(format!("cannot parse {}: {error}", path().display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_are_strict() {
        assert!(parse_bool("X", "yes").unwrap());
        assert!(!parse_bool("X", "off").unwrap());
        assert!(parse_bool("X", "sometimes").is_err());
    }
}
