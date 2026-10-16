use thiserror::Error;

/// lookup 用に正規化されたユーザー参照（spec 4.4）。
///
/// handle 文字列でも `id` でもない。表示用 `handle` は `handle_string` から生成する。
/// 正規化は lookup path だけ（display path は API が返した stored 値を使う）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRef {
    pub username: String,
    pub host: Option<String>,
}

#[derive(Debug, Error)]
pub enum HandleError {
    #[error("Invalid handle \"{input}\": must be @username or @username@host")]
    InvalidHandle { input: String },
}

/// `handle` 引数を解析する（決定5、spec 4.4）。
///
/// - 値は `@` で始まっていなければならない（それ以外はエラー）。
/// - `@username`（host 省略）はローカルユーザー（`host = None`）。
/// - `@username@host` は username / host に分割する。解析した host がインスタンスホストと
///   等しい場合はローカルに正規化する（`host = None`）。Misskey はローカルユーザーを
///   `host: null` で保存するため、自ホストを remote host として渡すと失敗する。
/// - lookup path の正規化: username は lowercase、host は lowercase + punycode。
///   `RemoteUserResolveService.resolveUser` は `username.toLowerCase()` と
///   `utilityService.toPuny(host)` を使い、`host === toPuny(config.host)` で local と判定する。
pub fn parse_handle(raw: &str, instance_host: &str) -> Result<UserRef, HandleError> {
    let err = || HandleError::InvalidHandle {
        input: raw.to_string(),
    };
    let trimmed = raw.trim();
    let without_at = trimmed.strip_prefix('@').ok_or_else(err)?;
    if without_at.is_empty() {
        return Err(err());
    }
    let (username, host): (&str, Option<&str>) = match without_at.split_once('@') {
        Some((u, h)) if !u.is_empty() && !h.is_empty() => (u, Some(h)),
        None => (without_at, None),
        _ => return Err(err()),
    };
    let host = match host {
        Some(h) => Some(normalize_host(h).ok_or_else(err)?),
        None => None,
    };
    Ok(UserRef {
        username: username.to_lowercase(),
        host: host.filter(|h| *h != instance_host),
    })
}

/// LLM に提示するハンドル（`@username@host.domain`）。常に完全形になる。
///
/// `UserRef` がすでに正規化済みであることを前提とする（`parse_handle` の結果、または API 応答の
/// stored 値）。remote user の username は case を保存するので、ここで lowercase しない。
pub fn handle_string(r: &UserRef, instance_host: &str) -> String {
    format!(
        "@{}@{}",
        r.username,
        r.host.as_deref().unwrap_or(instance_host)
    )
}

/// lowercase + punycode。不正な label は None。
pub fn normalize_host(host: &str) -> Option<String> {
    let lower = host.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    Some(idna::domain_to_ascii(&lower).ok().unwrap_or(lower))
}
