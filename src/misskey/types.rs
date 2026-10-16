use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::dates::parse_created_at;

/// Misskey API が返すユーザー（`users/show` / `users/me` / packed user）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawUser {
    pub id: String,
    pub name: String,
    pub username: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pinned_notes: Vec<RawNote>,
}

/// Misskey API が返すノート。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNote {
    pub id: String,
    pub created_at: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub cw: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub files: Vec<RawFile>,
    #[serde(default)]
    pub user: Option<RawUser>,
    #[serde(default)]
    pub renote: Option<Box<RawNote>>,
    #[serde(default)]
    pub reply: Option<Box<RawNote>>,
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
    pub created_at: String,
    pub name: String,
    pub handle: String,
    /// `note` / `renote` / `reply`（doc/decisions.md 参照）
    pub note_type: String,
    pub text: Option<String>,
    pub cw: Option<String>,
    pub visibility: Option<String>,
    pub files: Vec<FileView>,
    pub renote: Option<Box<NoteWithUser>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub name: Option<String>,
    pub url: Option<String>,
    pub is_nsfw: bool,
}

/// LLM に返すユーザー（spec 4.3）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserWithNotes {
    pub name: String,
    pub handle: String,
    pub description: Option<String>,
    pub pinned_notes: Vec<NoteWithUser>,
    pub recent_notes: Vec<NoteWithUser>,
}

/// LLM に `userId` は渡さない（spec 4.2 / design）。
pub fn handle_of(user: &RawUser, instance_host: &str) -> String {
    let host = user.host.as_deref().unwrap_or(instance_host);
    crate::handle::handle_string(&user.username, host)
}

/// pure renote（text / cw が null かつ renote 付き）は "renote"、reply 付きは "reply"、
/// それ以外は "note"（doc/decisions.md 参照）。
pub fn note_type_of(note: &RawNote) -> &'static str {
    if note.reply.is_some() {
        "reply"
    } else if note.text.is_none() && note.cw.is_none() && note.renote.is_some() {
        "renote"
    } else {
        "note"
    }
}

/// `fallback` は queried user（renote / reply の著者が異なる場合があるため、
/// packed user が無いときのみ使う）。
pub fn to_view(note: &RawNote, fallback: Option<&RawUser>, instance_host: &str) -> NoteWithUser {
    let author = note.user.as_ref().or(fallback);
    let (name, handle) = match author {
        Some(u) => (u.name.clone(), handle_of(u, instance_host)),
        None => (String::new(), String::new()),
    };
    NoteWithUser {
        created_at: note.created_at.clone(),
        name,
        handle,
        note_type: note_type_of(note).to_string(),
        text: note.text.clone(),
        cw: note.cw.clone(),
        visibility: note.visibility.clone(),
        files: note
            .files
            .iter()
            .map(|f| FileView {
                name: f.name.clone(),
                url: f.url.clone(),
                is_nsfw: f.is_nsfw,
            })
            .collect(),
        renote: note
            .renote
            .as_ref()
            .map(|r| Box::new(to_view(r, fallback, instance_host))),
    }
}

/// `createdAt` の厳密検証（spec 6.1.5）。解析不能は範囲外扱い。
pub fn in_range(note: &RawNote, after: Option<DateTime<Utc>>, before: Option<DateTime<Utc>>) -> bool {
    let Some(created) = parse_created_at(&note.created_at) else {
        return false;
    };
    if let Some(a) = after && created <= a {
        return false;
    }
    if let Some(b) = before && created >= b {
        return false;
    }
    true
}

pub fn created_at(note: &RawNote) -> Option<DateTime<Utc>> {
    parse_created_at(&note.created_at)
}

pub fn sort_desc(notes: &mut [NoteWithUser]) {
    notes.sort_by(|a, b| match (parse_created_at(&b.created_at), parse_created_at(&a.created_at)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => b.created_at.cmp(&a.created_at),
    });
}

pub fn utc_now_marker() -> DateTime<Utc> {
    Utc::now()
}
