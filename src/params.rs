use schemars::JsonSchema;
use serde::Deserialize;

pub const API_MAX_LIMIT: u32 = 100;
pub const DEFAULT_TIMELINE_LIMIT: u32 = 20;
pub const DEFAULT_USER_NOTES_LIMIT: u32 = 100;
pub const RECENT_NOTES_LIMIT: u32 = 10;
/// 暴走防止（spec 6.1.4 の「例: 20ページ」に対し、doc/decisions.md で 10 ページに決定）
pub const MAX_PAGES: u32 = 10;

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TimelineType {
    Home,
    #[default]
    Local,
    Global,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetTimelineParams {
    #[serde(default)]
    pub r#type: Option<TimelineType>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetUserParams {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetUserNotesParams {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    /// ISO 8601（YYYY-MM-DD、任意で時刻付き）。排他境界。
    #[serde(default)]
    pub after: Option<String>,
    /// ISO 8601（YYYY-MM-DD、任意で時刻付き）。排他境界。
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_renotes: Option<bool>,
}

pub fn clamp_limit(limit: Option<u32>, default: u32) -> u32 {
    limit.unwrap_or(default).clamp(1, API_MAX_LIMIT)
}
