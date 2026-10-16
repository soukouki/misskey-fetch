use thiserror::Error;

/// `@username` / `@username@host` のユーザー指定を username / host に分割する（spec 5.2）。
///
/// 内部表現は常にこの構造化型（`None` = ローカルユーザー）。表示用 `handle` 文字列からは
/// 生成しない（生成は `handle_string`）。
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

/// `handle` 引数を解析する（決定5: 入出力とも `@user@host` 形式に統一）。
///
/// - 値は `@` で始まっていなければならない（それ以外はエラー）。
/// - `@username`（host 省略）はローカルユーザー（`host = None`）。
/// - `@username@host` は username / host に分割する。解析した host がインスタンスホストと
///   等しい場合はローカルに正規化する（`host = None`）。Misskey はローカルユーザーを
///   `host: null` で保存するため、自ホストを remote host として渡すと失敗する。
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
    Ok(UserRef {
        username: username.to_string(),
        host: host.filter(|h| *h != instance_host).map(|h| h.to_string()),
    })
}

/// LLM に提示するハンドル（`@username@host.domain`）。UserRef から生成する。
/// host 省略時はインスタンスホストで補完するため、常に `@user@host` の完全形になる。
pub fn handle_string(r: &UserRef, instance_host: &str) -> String {
    format!("@{}@{}", r.username, r.host.as_deref().unwrap_or(instance_host))
}
