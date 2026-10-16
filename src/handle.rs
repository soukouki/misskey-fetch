/// `@user@host` 形式のユーザー指定を username / host に分割する（spec 5.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRef {
    pub username: String,
    pub host: Option<String>,
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

/// LLM に提示するハンドル（`@username@host.domain`）。
pub fn handle_string(username: &str, host: &str) -> String {
    format!("@{username}@{host}")
}
