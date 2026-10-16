use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::config::Config;
use crate::dates::parse_iso8601;
use crate::errors::ToolError;
use crate::handle::{handle_string, normalize_user_ref};
use crate::misskey::client::Client;
use crate::misskey::types::{self, NoteWithUser, RawNote, RawUser, UserWithNotes};
use crate::params::{
    API_MAX_LIMIT, DEFAULT_TIMELINE_LIMIT, DEFAULT_USER_NOTES_LIMIT, MAX_PAGES, RECENT_NOTES_LIMIT,
    TimelineType, clamp_limit, GetTimelineParams, GetUserNotesParams, GetUserParams,
};

/// `get_timeline`（spec 5.1）
pub async fn get_timeline(
    client: &Client,
    cfg: &Config,
    p: &GetTimelineParams,
) -> Result<Vec<NoteWithUser>, ToolError> {
    let timeline_type = p.r#type.unwrap_or_default();
    let endpoint = match timeline_type {
        TimelineType::Home => "notes/timeline",
        TimelineType::Local => "notes/local-timeline",
        TimelineType::Global => "notes/global-timeline",
    };
    let limit = clamp_limit(p.limit, DEFAULT_TIMELINE_LIMIT);
    let body = json!({ "limit": limit });
    let value = client.call(endpoint, &body, "").await?;
    let notes = as_notes(&value)?;

    let mut views: Vec<NoteWithUser> = notes
        .iter()
        .map(|n| types::to_view(n, None, &cfg.instance_host))
        .collect();
    // Misskey は ID 順で返す。createdAt 降順で返却する（spec 5.1）
    types::sort_desc(&mut views);
    Ok(views)
}

/// `get_user_info`（spec 5.2）
pub async fn get_user_info(
    client: &Client,
    cfg: &Config,
    p: &GetUserParams,
) -> Result<UserWithNotes, ToolError> {
    let user = resolve_user(client, cfg, p.username.as_deref(), p.host.as_deref()).await?;
    let handle = types::handle_of(&user, &cfg.instance_host);

    // recentNotes は users/notes (limit 10) の追加呼び出しで埋める（spec 4.3）
    let body = json!({ "userId": user.id, "limit": RECENT_NOTES_LIMIT });
    let value = client.call("users/notes", &body, &handle).await?;
    let notes = as_notes(&value)?;

    let mut recent: Vec<NoteWithUser> = notes
        .iter()
        .map(|n| types::to_view(n, Some(&user), &cfg.instance_host))
        .collect();
    types::sort_desc(&mut recent);

    let pinned: Vec<NoteWithUser> = user
        .pinned_notes
        .iter()
        .map(|n| types::to_view(n, Some(&user), &cfg.instance_host))
        .collect();

    Ok(UserWithNotes {
        name: user.name.clone(),
        handle,
        description: user.description.clone(),
        pinned_notes: pinned,
        recent_notes: recent,
    })
}

/// `get_user_notes`（spec 5.3）
pub async fn get_user_notes(
    client: &Client,
    cfg: &Config,
    p: &GetUserNotesParams,
) -> Result<Vec<NoteWithUser>, ToolError> {
    let user = resolve_user(client, cfg, p.username.as_deref(), p.host.as_deref()).await?;
    let handle = types::handle_of(&user, &cfg.instance_host);

    let after = p
        .after
        .as_deref()
        .map(|s| parse_iso8601(s, cfg.tz))
        .transpose()?;
    let before = p
        .before
        .as_deref()
        .map(|s| parse_iso8601(s, cfg.tz))
        .transpose()?;
    if let (Some(a), Some(b)) = (after, before) && a > b {
        return Err(ToolError::InvalidParams {
            message: "after は before より後にできません".to_string(),
        });
    }

    let limit = clamp_limit(p.limit, DEFAULT_USER_NOTES_LIMIT);
    let include_renotes = p.include_renotes.unwrap_or(true);
    let has_range = after.is_some() || before.is_some();
    let max_pages = if has_range { MAX_PAGES } else { 1 };

    let mut collected: Vec<RawNote> = Vec::new();
    let mut until_id: Option<String> = None;

    for _ in 0..max_pages {
        if collected.len() >= limit as usize {
            break;
        }
        let page_size = (limit as usize - collected.len()).min(API_MAX_LIMIT as usize);
        let mut body = json!({
            "userId": user.id,
            "limit": page_size,
            "withRenotes": include_renotes,
        });
        // 主経路: sinceDate / untilDate（epoch ms）。ID境界の自前計算はしない（spec 6.1.1）
        if let Some(b) = before {
            body["untilDate"] = json!(b.timestamp_millis());
        }
        if let Some(a) = after {
            body["sinceDate"] = json!(a.timestamp_millis());
        }
        if let Some(id) = &until_id {
            body["untilId"] = json!(id);
        }

        let value = client.call("users/notes", &body, &handle).await?;
        let page = as_notes(&value)?;
        if page.is_empty() {
            break; // 停止条件: 空ページ
        }

        let mut hit_boundary = false;
        for note in &page {
            if collected.len() >= limit as usize {
                break;
            }
            if types::in_range(note, after, before) {
                collected.push(note.clone());
            } else if past_boundary(note, after) {
                hit_boundary = true;
            }
        }

        if collected.len() >= limit as usize || hit_boundary {
            break; // 停止条件: 件数到達 / 境界到達
        }
        until_id = page.last().map(|n| n.id.clone());
    }

    let mut views: Vec<NoteWithUser> = collected
        .iter()
        .map(|n| types::to_view(n, Some(&user), &cfg.instance_host))
        .collect();
    types::sort_desc(&mut views);
    Ok(views)
}

/// 返ってきた件数を `createdAt` で厳密検証して範囲外を落とす（spec 6.1.5）。
fn past_boundary(note: &RawNote, after: Option<DateTime<Utc>>) -> bool {
    let Some(created) = types::created_at(note) else {
        return false;
    };
    after.is_some_and(|a| created <= a)
}

async fn resolve_user(
    client: &Client,
    cfg: &Config,
    username: Option<&str>,
    host: Option<&str>,
) -> Result<RawUser, ToolError> {
    let Some(raw) = username.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        if host.is_some() {
            return Err(ToolError::InvalidParams {
                message: "host は username を指定しているときのみ使えます".to_string(),
            });
        }
        let value = client.call("users/me", &json!({}), "").await?;
        return serde_json::from_value::<RawUser>(value).map_err(|e| ToolError::Http {
            message: e.to_string(),
        });
    };

    let r = normalize_user_ref(raw).ok_or_else(|| ToolError::InvalidParams {
        message: "username を解析できません".to_string(),
    })?;
    let host = r
        .host
        .clone()
        .or_else(|| host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()));
    // `@user@host` を username フィールドにそのまま渡してはいけない（spec 5.2）
    let body = json!({ "username": r.username, "host": host });
    let handle = handle_string(&r.username, host.as_deref().unwrap_or(&cfg.instance_host));

    let value = client.call("users/show", &body, &handle).await?;
    serde_json::from_value::<RawUser>(value).map_err(|e| ToolError::Http {
        message: e.to_string(),
    })
}

fn as_notes(value: &Value) -> Result<Vec<RawNote>, ToolError> {
    serde_json::from_value::<Vec<RawNote>>(value.clone()).map_err(|e| ToolError::Http {
        message: e.to_string(),
    })
}

