use std::env;

use chrono_tz::Tz;
use thiserror::Error;

pub const DEFAULT_TZ: &str = "Asia/Tokyo";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("環境変数 {name} が設定されていません")]
    MissingEnv { name: &'static str },
    #[error("MISSKEY_API_URL は https の URL でなければなりません: {url}")]
    InvalidUrl { url: String },
    #[error("TZ を解析できません（IANA タイムゾーン名で指定してください）: {tz}")]
    InvalidTz { tz: String },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub token: String,
    pub api_base: String,
    pub instance_host: String,
    pub tz: Tz,
}

impl Config {
    /// 環境変数を起動時に検証する（spec 6.3）。
    ///
    /// `TZ` は design.md のデフォルト `Asia/Tokyo` に従い、未設定時はフォールバックする。
    pub fn from_env() -> Result<Self, ConfigError> {
        let token = env::var("MISSKEY_TOKEN").unwrap_or_default();
        if token.trim().is_empty() {
            return Err(ConfigError::MissingEnv {
                name: "MISSKEY_TOKEN",
            });
        }

        let raw_url = env::var("MISSKEY_API_URL").unwrap_or_default();
        let api_base = normalize_api_base(&raw_url).ok_or_else(|| ConfigError::InvalidUrl {
            url: raw_url.clone(),
        })?;
        let instance_host = host_of(&api_base).ok_or_else(|| ConfigError::InvalidUrl {
            url: raw_url.clone(),
        })?;

        let raw_tz = env::var("TZ").unwrap_or_else(|_| DEFAULT_TZ.to_string());
        let tz = raw_tz
            .parse::<Tz>()
            .map_err(|_| ConfigError::InvalidTz { tz: raw_tz.clone() })?;

        Ok(Self {
            token,
            api_base,
            instance_host,
            tz,
        })
    }
}

/// `https://` に限定し、末尾スラッシュを除去する。
fn normalize_api_base(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if !trimmed.starts_with("https://") {
        return None;
    }
    if trimmed.len() <= 8 {
        return None;
    }
    Some(trimmed.to_string())
}

/// API ベース URL のホスト部（`@user@host` のローカルホスト表現に使う）。
fn host_of(api_base: &str) -> Option<String> {
    let rest = api_base.get(8..)?;
    let host = rest.split('/').next()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}
