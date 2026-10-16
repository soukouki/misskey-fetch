use std::env;

use chrono_tz::Tz;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Environment variable {name} is not set")]
    MissingEnv { name: &'static str },
    #[error("MISSKEY_API_URL must be an https URL: {url}")]
    InvalidUrl { url: String },
    #[error("Cannot parse TZ (specify an IANA time zone name): {tz}")]
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
    /// `TZ` は必須（未設定時は起動エラー）。`chrono-tz` は env を自動参照しないため
    /// `TZ` 文字列を明示 parse する。
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

        let raw_tz = env::var("TZ").map_err(|_| ConfigError::MissingEnv { name: "TZ" })?;
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

/// `https://` に限定し、末尾スラッシュ（1 個以上、`https://host/api///` のような連続も含む）を除去する。
/// 末尾スラッシュ付きの `MISSKEY_API_URL` でも、後続の URL 組み立て（`{base}/{endpoint}`）が
/// `//endpoint` にならないよう、正規化は読み込み時に行う。
pub fn normalize_api_base(raw: &str) -> Option<String> {
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
/// `instance_host` は punycode で保存する（spec 6.3）。Misskey は `toPuny(host)` で比較する。
pub fn host_of(api_base: &str) -> Option<String> {
    let rest = api_base.get(8..)?;
    let host = rest.split('/').next()?;
    if host.is_empty() {
        return None;
    }
    crate::handle::normalize_host(host)
}
