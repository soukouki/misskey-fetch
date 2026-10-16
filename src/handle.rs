use thiserror::Error;

/// `@user@host` 形式のユーザー指定を username / host に分割する（spec 5.2）。
///
/// 内部表現は常にこの構造化型。表示用 `handle` 文字列からは生成しない（生成は `handle_string`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRef {
    pub username: String,
    pub host: Option<String>,
}

#[derive(Debug, Error)]
pub enum HandleError {
    #[error("Cannot parse username: {input}")]
    InvalidUsername { input: String },
    #[error("Conflicting host: username already contains a host ({input}); do not pass the host parameter together with an embedded host")]
    HostConflict { input: String },
}

/// 先頭の `@` の有無を正規化し、`@username@host` 形式は username / host に分割する。
///
/// `username` フィールドに `@user@host` をそのまま渡してはいけない（spec 5.2）。
pub fn normalize_user_ref(raw: &str) -> Option<UserRef> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if without_at.is_empty() {
        return None;
    }
    match without_at.split_once('@') {
        Some((username, host)) => {
            if username.is_empty() || host.is_empty() {
                return None;
            }
            Some(UserRef {
                username: username.to_string(),
                host: Some(host.to_string()),
            })
        }
        None => Some(UserRef {
            username: without_at.to_string(),
            host: None,
        }),
    }
}

/// username に埋め込まれた host と明示的な `host` 引数の同時指定はエラー（spec 5.2）。
pub fn resolve_user_ref(raw: &str, explicit_host: Option<&str>) -> Result<UserRef, HandleError> {
    let r = normalize_user_ref(raw).ok_or_else(|| HandleError::InvalidUsername {
        input: raw.to_string(),
    })?;
    let explicit = explicit_host.map(|h| h.trim()).filter(|h| !h.is_empty());
    if r.host.is_some() && explicit.is_some() {
        return Err(HandleError::HostConflict {
            input: raw.to_string(),
        });
    }
    Ok(UserRef {
        username: r.username,
        host: r.host.or_else(|| explicit.map(|h| h.to_string())),
    })
}

/// LLM に提示するハンドル（`@username@host.domain`）。UserRef から生成する。
pub fn handle_string(r: &UserRef, instance_host: &str) -> String {
    format!("@{}@{}", r.username, r.host.as_deref().unwrap_or(instance_host))
}
