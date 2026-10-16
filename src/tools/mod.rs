use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::config::Config;
use crate::dates::parse_iso8601;
use crate::errors::ToolError;
use crate::handle::{handle_string, parse_handle};
use crate::misskey::client::Client;
use crate::misskey::types::{self, NoteWithUser, NotesPage, RawNote, RawUser, UserWithNotes};
use crate::params::{
    API_MAX_LIMIT, DEFAULT_TIMELINE_LIMIT, DEFAULT_USER_NOTES_LIMIT, MAX_PAGES, MAX_TOTAL_LIMIT,
    RECENT_NOTES_LIMIT, TimelineType, clamp_limit, GetTimelineParams, GetUserNotesParams, GetUserParams,
};

/// `get_timeline`（spec 5.1）。ページ単位で `untilId` によりサーバ側ページネーションする。
pub async fn get_timeline(
    client: &Client,
    cfg: &Config,
    p: &GetTimelineParams,
) -> Result<NotesPage, ToolError> {
    let timeline_type = p.r#type.unwrap_or_default();
    let endpoint = match timeline_type {
        TimelineType::Home => "notes/timeline",
        TimelineType::Local => "notes/local-timeline",
        TimelineType::Global => "notes/global-timeline",
    };
    let requested = p.limit.unwrap_or(DEFAULT_TIMELINE_LIMIT);
    let effective = clamp_limit(p.limit, DEFAULT_TIMELINE_LIMIT);

    let mut collected: Vec<RawNote> = Vec::new();
    let mut until_id: Option<String> = None;
    let mut pages_used: u32 = 0;

    for _ in 0..MAX_PAGES {
        if collected.len() >= effective as usize {
            break; // 停止条件: 件数到達
        }
        pages_used += 1;
        let page_size = (effective as usize - collected.len()).min(API_MAX_LIMIT as usize);
        let mut body = json!({ "limit": page_size });
        if let Some(id) = &until_id {
            body["untilId"] = json!(id);
        }

        let value = client.call(endpoint, &body, "").await?;
        let page = as_notes(&value)?;
        if page.is_empty() {
            break; // 停止条件: 空ページ
        }
        for note in &page {
            if collected.len() >= effective as usize {
                break;
            }
            collected.push(note.clone());
        }
        if collected.len() >= effective as usize {
            break; // 停止条件: 件数到達
        }
        until_id = page.last().map(|n| n.id.clone());
    }

    let stats = page_stats(requested, effective, collected.len(), pages_used, false);
    Ok(build_page(&mut collected, None, &cfg.instance_host, &stats))
}

/// `get_user_info`（spec 5.2）。User object をそのまま返す（ページラッパーなし）。
pub async fn get_user_info(
    client: &Client,
    cfg: &Config,
    p: &GetUserParams,
) -> Result<UserWithNotes, ToolError> {
    let user = resolve_user(client, cfg, p.handle.as_deref()).await?;
    let handle = types::handle_of(&user, &cfg.instance_host);

    // recentNotes は users/notes (limit 10) の追加呼び出しで埋める（spec 4.3）
    let body = json!({ "userId": user.id, "limit": RECENT_NOTES_LIMIT });
    let value = client.call("users/notes", &body, &handle).await?;
    let mut recent_raw = as_notes(&value)?;
    types::sort_desc(&mut recent_raw);

    Ok(UserWithNotes {
        name: user.name.clone(),
        handle,
        description: user.description.clone(),
        pinned_notes: views(&user.pinned_notes, Some(&user), &cfg.instance_host),
        recent_notes: views(&recent_raw, Some(&user), &cfg.instance_host),
    })
}

/// `get_user_notes`（spec 5.3）。ページ単位で `untilId` によりサーバ側ページネーションする。
pub async fn get_user_notes(
    client: &Client,
    cfg: &Config,
    p: &GetUserNotesParams,
) -> Result<NotesPage, ToolError> {
    let user = resolve_user(client, cfg, p.handle.as_deref()).await?;
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
            message: "after cannot be later than before".to_string(),
        });
    }

    let requested = p.limit.unwrap_or(DEFAULT_USER_NOTES_LIMIT);
    let effective = clamp_limit(p.limit, DEFAULT_USER_NOTES_LIMIT);
    let include_renotes = p.include_renotes.unwrap_or(true);

    let mut collected: Vec<RawNote> = Vec::new();
    let mut until_id: Option<String> = None;
    let mut pages_used: u32 = 0;
    let mut hit_boundary = false;

    for _ in 0..MAX_PAGES {
        if collected.len() >= effective as usize {
            break; // 停止条件: 件数到達
        }
        pages_used += 1;
        let page_size = (effective as usize - collected.len()).min(API_MAX_LIMIT as usize);
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

        for note in &page {
            if collected.len() >= effective as usize {
                break;
            }
            if types::in_range(note, after, before) {
                collected.push(note.clone());
            } else if past_boundary(note, after) {
                hit_boundary = true;
            }
        }

        if collected.len() >= effective as usize || hit_boundary {
            break; // 停止条件: 件数到達 / 境界到達
        }
        until_id = page.last().map(|n| n.id.clone());
    }

    let stats = page_stats(requested, effective, collected.len(), pages_used, hit_boundary);
    Ok(build_page(&mut collected, Some(&user), &cfg.instance_host, &stats))
}

/// 返ってきた件数を `createdAt` で厳密検証して範囲外を落とす（spec 6.1.5）。
fn past_boundary(note: &RawNote, after: Option<DateTime<Utc>>) -> bool {
    after.is_some_and(|a| note.created_at <= a)
}

/// `truncated` は、要求 limit が 1000 に clamp された場合、またはページ / 境界が
/// 要求件数を満たす前に停止した場合（空ページ=データ尽きは除く）。
fn page_stats(
    requested: u32,
    effective: u32,
    collected: usize,
    pages_used: u32,
    hit_boundary: bool,
) -> PageStats {
    let clamped = requested > MAX_TOTAL_LIMIT;
    let short = (collected as u32) < effective;
    PageStats {
        requested,
        collected: collected as u32,
        truncated: clamped || (short && (hit_boundary || pages_used >= MAX_PAGES)),
    }
}

struct PageStats {
    requested: u32,
    collected: u32,
    truncated: bool,
}

fn build_page(
    collected: &mut [RawNote],
    fallback: Option<&RawUser>,
    instance_host: &str,
    stats: &PageStats,
) -> NotesPage {
    types::sort_desc(collected);
    let notes = views(collected, fallback, instance_host);
    let next_cursor = stats.truncated.then(|| collected.last().map(|n| n.id.clone())).flatten();
    let message = stats.truncated.then(|| {
        let base = format!(
            "Showing {} notes of {} requested; the rest is omitted.",
            stats.collected, stats.requested
        );
        match &next_cursor {
            Some(id) => format!("{base} Call again with untilId=\"{id}\" to continue."),
            None => base,
        }
    });
    NotesPage {
        notes,
        truncated: stats.truncated,
        next_cursor,
        message,
    }
}

fn views(collected: &[RawNote], fallback: Option<&RawUser>, instance_host: &str) -> Vec<NoteWithUser> {
    collected
        .iter()
        .map(|n| types::to_view(n, fallback, instance_host))
        .collect()
}

async fn resolve_user(
    client: &Client,
    cfg: &Config,
    handle: Option<&str>,
) -> Result<RawUser, ToolError> {
    let Some(raw) = handle.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        let value = client.call("users/me", &json!({}), "").await?;
        return serde_json::from_value::<RawUser>(value).map_err(|e| ToolError::Http {
            message: e.to_string(),
        });
    };

    let r = parse_handle(raw, &cfg.instance_host)?;
    // ローカルユーザーの host は null（Misskey はローカルユーザーを host: null で保存する）
    let body = json!({ "username": r.username, "host": r.host });
    let display = handle_string(&r, &cfg.instance_host);

    let value = client.call("users/show", &body, &display).await?;
    serde_json::from_value::<RawUser>(value).map_err(|e| ToolError::Http {
        message: e.to_string(),
    })
}

fn as_notes(value: &Value) -> Result<Vec<RawNote>, ToolError> {
    serde_json::from_value::<Vec<RawNote>>(value.clone()).map_err(|e| ToolError::Http {
        message: e.to_string(),
    })
}
