use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::handle::{UserRef, handle_string};

/// Misskey の `NoteLite.visibility`（json-schema と同一の文字列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Home,
    Followers,
    Specified,
}

/// `renote` / `reply` の判定優先順位は renote > reply > note（doc/decisions.md）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteType {
    Note,
    Renote,
    Reply,
}

/// Misskey API が返すユーザー（`users/show` / `users/me` / packed user）。
///
/// `UserLite.name` は nullable（非 optional は実バグだった）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawUser {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub username: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pinned_notes: Vec<RawNote>,
}

/// Misskey API が返すノート。`createdAt` はデシリアライズ時に解析する（spec 6.1.5）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNote {
    pub id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub cw: Option<String>,
    pub visibility: Visibility,
    #[serde(default)]
    pub files: Vec<RawFile>,
    #[serde(default)]
    pub user: Option<RawUser>,
    #[serde(default)]
    pub renote: Option<Box<RawNote>>,
    #[serde(default)]
    pub reply: Option<Box<RawNote>>,
    /// Misskey の `isHidden`（policy 由来の隠蔽）。デフォルト false。
    #[serde(default)]
    pub is_hidden: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFile {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub is_nsfw: bool,
}

/// LLM に返すノート（spec 4.2）。著者はノートごとに解決する。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWithUser {
    pub created_at: DateTime<Utc>,
    pub name: Option<String>,
    pub handle: String,
    pub note_type: NoteType,
    /// `cw` は警告文（UI で常時表示）、`text` はその折りたまれた本文。
    pub text: Option<String>,
    pub cw: Option<String>,
    /// `cw` があれば UI は本文を警告の後ろに折る。
    pub has_cw: bool,
    /// policy 由来の隠蔽（`hideNote()` は text / cw を null にする）。
    pub is_hidden: bool,
    pub visibility: Visibility,
    pub files: Vec<FileView>,
    pub renote: Option<Box<NoteWithUser>>,
    /// 深度上限（10）で打ち切られた場合 true。
    pub renote_depth_limited: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub name: Option<String>,
    pub url: Option<String>,
    pub is_nsfw: bool,
}

/// LLM に返すページ（spec 4.2 / 6.1.4）。`get_timeline` / `get_user_notes` の応答形状。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesPage {
    pub notes: Vec<NoteWithUser>,
    pub truncated: bool,
    /// 続きを取得するための `untilId`（最後に返したノートの id）。
    pub next_cursor: Option<String>,
    /// `truncated` のときだけ入る英語のメッセージ。
    pub message: Option<String>,
}

/// LLM に返すユーザー（spec 4.3）。ページラッパーは付けない。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserWithNotes {
    pub name: Option<String>,
    pub handle: String,
    pub description: Option<String>,
    pub pinned_notes: Vec<NoteWithUser>,
    pub recent_notes: Vec<NoteWithUser>,
}

/// LLM に `userId` は渡さない（spec 4.2 / design）。表示用 handle は UserRef から生成する。
pub fn handle_of(user: &RawUser, instance_host: &str) -> String {
    let r = UserRef {
        username: user.username.clone(),
        host: user.host.clone(),
    };
    handle_string(&r, instance_host)
}

/// renote 付きは "renote"（引用リポストは renote 側）、reply 付きは "reply"、
/// それ以外は "note"（doc/decisions.md 参照）。
pub fn note_type_of(note: &RawNote) -> NoteType {
    if note.renote.is_some() {
        NoteType::Renote
    } else if note.reply.is_some() {
        NoteType::Reply
    } else {
        NoteType::Note
    }
}

/// `renote` の再帰深度上限（深い入れ子 / 自己参照からの保護）。これを超えたら `renote: null`。
pub const MAX_RENOTE_DEPTH: u32 = 10;

/// `fallback` は queried user（renote / reply の著者が異なる場合があるため、
/// packed user が無いときのみ使う）。
pub fn to_view(note: &RawNote, fallback: Option<&RawUser>, instance_host: &str) -> NoteWithUser {
    to_view_at_depth(note, fallback, instance_host, 0)
}

fn to_view_at_depth(
    note: &RawNote,
    fallback: Option<&RawUser>,
    instance_host: &str,
    depth: u32,
) -> NoteWithUser {
    let author = note.user.as_ref().or(fallback);
    let handle = match author {
        Some(u) => handle_of(u, instance_host),
        None => String::new(),
    };
    let depth_limited = depth >= MAX_RENOTE_DEPTH && note.renote.is_some();
    NoteWithUser {
        created_at: note.created_at,
        name: author.and_then(|u| u.name.clone()),
        handle,
        note_type: note_type_of(note),
        text: note.text.clone(),
        cw: note.cw.clone(),
        has_cw: note.cw.is_some(),
        is_hidden: note.is_hidden,
        visibility: note.visibility,
        files: note
            .files
            .iter()
            .map(|f| FileView {
                name: f.name.clone(),
                url: f.url.clone(),
                is_nsfw: f.is_nsfw,
            })
            .collect(),
        renote: if depth_limited {
            None
        } else {
            note.renote
                .as_ref()
                .map(|r| Box::new(to_view_at_depth(r, fallback, instance_host, depth + 1)))
        },
        renote_depth_limited: depth_limited,
    }
}

/// `createdAt` の厳密検証（spec 6.1.5）。型で保証されるため解析不能分岐は不要。
pub fn in_range(note: &RawNote, after: Option<DateTime<Utc>>, before: Option<DateTime<Utc>>) -> bool {
    if let Some(a) = after && note.created_at <= a {
        return false;
    }
    if let Some(b) = before && note.created_at >= b {
        return false;
    }
    true
}

/// API の ID 順に依存せず返却前に `createdAt` 降順でソート（doc/decisions.md）。
pub fn sort_desc(notes: &mut [RawNote]) {
    notes.sort_by_key(|a| std::cmp::Reverse(a.created_at));
}
