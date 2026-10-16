use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::errors::ToolError;
use crate::handle::{UserRef, handle_string, normalize_host};
use crate::misskey::types::{Channel, DriveFile, Note, Poll, UserLite, Visibility};

/// relation の入れ子深度上限（hostile な instance に対する size policy。model では cycle が
/// 不可能なので本来不要。spec 4.3）。
pub const MAX_RELATION_DEPTH: u32 = 10;

/// LLM に返すユーザー（spec 4.2）。`userId` は含めない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub name: Option<String>,
    pub handle: String,
    pub is_bot: bool,
}

/// `UserView` + `UserDetailed` 由来の field（spec 4.2）。`get_user_info` の返却型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserViewDetailed {
    pub user: UserView,
    pub description: Option<String>,
    pub pinned_notes: Vec<NoteView>,
    /// Misskey の model field ではない。view layer の組み立て（spec 4.2）。
    pub recent_notes: Vec<NoteView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub is_sensitive: bool,
    pub url: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollChoiceView {
    pub text: String,
    pub votes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollView {
    pub expires_at: Option<DateTime<Utc>>,
    pub multiple: bool,
    pub choices: Vec<PollChoiceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub name: String,
    pub is_sensitive: bool,
}

/// `reply` / `renote` の relation（spec 4.3）。4状態を明示的な tagged structure で返す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RelationView {
    None,
    NotJoined { id: String },
    NotFound { id: String },
    Present { id: String, note: Box<NoteView> },
}

/// LLM に返すノート（spec 4.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub user: UserView,
    /// `cw` は警告文（UI で常時表示）、`text` はその折りたまれた本文。
    pub text: Option<String>,
    pub cw: Option<String>,
    pub visibility: Visibility,
    /// policy 由来の隠蔽。viewer-relative の relation（ノート固有の property ではない）。
    pub is_hidden: bool,
    pub poll: Option<PollView>,
    pub files: Vec<FileView>,
    pub channel: Option<ChannelView>,
    pub reply: RelationView,
    pub renote: RelationView,
}

/// LLM に返すページ（spec 5.4）。`get_timeline` / `get_user_notes` の応答形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageView {
    pub notes: Vec<NoteView>,
    pub truncated: bool,
    /// 続きを取得するための `untilId`（最後に返したノートの id）。
    pub next_cursor: Option<String>,
    /// `truncated` のときだけ入る英語のメッセージ。
    pub message: Option<String>,
}

pub fn to_user_view(user: &UserLite, instance_host: &str) -> UserView {
    UserView {
        name: user.name.clone(),
        handle: handle_string(
            &UserRef {
                username: user.username.clone(),
                host: user.host.as_deref().and_then(normalize_host),
            },
            instance_host,
        ),
        is_bot: user.is_bot,
    }
}

/// top-level note は `detail: true` で pack される（spec 4.1）。
pub fn to_view(note: &Note, instance_host: &str) -> Result<NoteView, ToolError> {
    to_view_at(note, instance_host, true, 0)
}

fn to_view_at(note: &Note, instance_host: &str, detail: bool, depth: u32) -> Result<NoteView, ToolError> {
    Ok(NoteView {
        id: note.id.clone(),
        created_at: note.created_at,
        user: to_user_view(&note.user, instance_host),
        text: note.text.clone(),
        cw: note.cw.clone(),
        visibility: note.visibility,
        is_hidden: note.is_hidden,
        poll: note.poll.as_ref().map(to_poll_view),
        files: note.files.iter().map(to_file_view).collect(),
        channel: note.channel.as_ref().map(to_channel_view),
        // 入れ子 reply は detail:false、入れ子 renote は detail:true で pack される（非対称）
        reply: to_relation("reply", &note.reply_id, &note.reply, detail, false, depth, instance_host)?,
        renote: to_relation("renote", &note.renote_id, &note.renote, detail, true, depth, instance_host)?,
    })
}

/// serde は key absent と null を区別できない（spec 4.3）。entity が None のとき、その note が
/// detail:true で pack されたなら key は存在し null だった（= notFound）、detail:false なら key
/// 自体がなかった（= notJoined）。depth 上限での打ち切りも notJoined（「応答に含まれなかった」）。
fn to_relation(
    field: &str,
    id: &Option<String>,
    entity: &Option<Box<Note>>,
    parent_detail: bool,
    child_detail: bool,
    depth: u32,
    instance_host: &str,
) -> Result<RelationView, ToolError> {
    // invariant: entity present ⇒ FK present（型ではなく実行時 check。spec 4.3）
    if entity.is_some() && id.is_none() {
        return Err(ToolError::InconsistentResponse {
            field: field.to_string(),
        });
    }
    let Some(id) = id else {
        return Ok(RelationView::None);
    };
    match entity {
        None => Ok(if depth >= MAX_RELATION_DEPTH || !parent_detail {
            RelationView::NotJoined { id: id.clone() }
        } else {
            RelationView::NotFound { id: id.clone() }
        }),
        Some(child) => {
            if depth >= MAX_RELATION_DEPTH {
                return Ok(RelationView::NotJoined { id: id.clone() });
            }
            Ok(RelationView::Present {
                id: id.clone(),
                note: Box::new(to_view_at(child, instance_host, child_detail, depth + 1)?),
            })
        }
    }
}

fn to_file_view(f: &DriveFile) -> FileView {
    FileView {
        name: f.name.clone(),
        mime: f.mime.clone(),
        size: f.size,
        is_sensitive: f.is_sensitive,
        url: f.url.clone(),
        comment: f.comment.clone(),
    }
}

fn to_poll_view(p: &Poll) -> PollView {
    PollView {
        expires_at: p.expires_at,
        multiple: p.multiple,
        choices: p
            .choices
            .iter()
            .map(|c| PollChoiceView {
                text: c.text.clone(),
                votes: c.votes,
            })
            .collect(),
    }
}

fn to_channel_view(c: &Channel) -> ChannelView {
    ChannelView {
        name: c.name.clone(),
        is_sensitive: c.is_sensitive,
    }
}
