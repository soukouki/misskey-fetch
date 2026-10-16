use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Misskey の `NoteLite.visibility`（json-schema と同一の文字列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Home,
    Followers,
    Specified,
}

use serde::Serialize;

/// `UserLite`（note の著者は常にこの shape）。
/// `name` は nullable（非 optional は実バグだった）。`description` / `pinnedNotes` を持たない。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLite {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub username: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub is_bot: bool,
}

/// `UserDetailed` = UserLite + description + pinnedNotes。
/// `users/show` は常に `schema: 'UserDetailed'` で pack するため `pinned_notes` は常に存在する。
/// UserLite との 2 shape を潰さない（spec 4.1）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDetailed {
    #[serde(flatten)]
    pub lite: UserLite,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pinned_notes: Vec<Note>,
}

/// Misskey API が返すノート。`createdAt` はデシリアライズ時に解析する（spec 6.1.5）。
///
/// `replyId` / `renoteId` は detail gate の外（常に pack される）。`reply` / `renote` / `poll` /
/// `clippedCount` / `myReaction` は detail gate 内。detail gate は API 側の選択であり model の
/// 制約ではない（spec 4.1）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub user: UserLite,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub cw: Option<String>,
    pub visibility: Visibility,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub poll: Option<Poll>,
    #[serde(default)]
    pub files: Vec<DriveFile>,
    #[serde(default)]
    pub channel: Option<Channel>,
    #[serde(default)]
    pub reply_id: Option<String>,
    #[serde(default)]
    pub reply: Option<Box<Note>>,
    #[serde(default)]
    pub renote_id: Option<String>,
    #[serde(default)]
    pub renote: Option<Box<Note>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub name: String,
    #[serde(rename = "type", default = "default_mime")]
    pub mime: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub is_sensitive: bool,
    #[serde(default = "default_url")]
    pub url: String,
    #[serde(default)]
    pub comment: Option<String>,
}

fn default_mime() -> String {
    String::new()
}

fn default_url() -> String {
    String::new()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub choices: Vec<PollChoice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollChoice {
    pub text: String,
    #[serde(default)]
    pub votes: i64,
    #[serde(default)]
    pub is_voted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub name: String,
    #[serde(default)]
    pub is_sensitive: bool,
}

/// 返ってきた件数を `createdAt` で厳密検証して範囲外を落とす（spec 6.1.5）。
pub fn in_range(note: &Note, after: Option<DateTime<Utc>>, before: Option<DateTime<Utc>>) -> bool {
    if let Some(a) = after && note.created_at <= a {
        return false;
    }
    if let Some(b) = before && note.created_at >= b {
        return false;
    }
    true
}

/// API の ID 順に依存せず返却前に `createdAt` 降順でソート（doc/decisions.md）。
pub fn sort_desc(notes: &mut [Note]) {
    notes.sort_by_key(|a| std::cmp::Reverse(a.created_at));
}
