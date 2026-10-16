use chrono_tz::Tz;
use serde_json::json;

use misskey_fetch::dates::parse_iso8601;
use misskey_fetch::errors::ToolError;
use misskey_fetch::handle::{handle_string, normalize_user_ref};
use misskey_fetch::misskey::types::{RawNote, in_range};

fn tokyo() -> Tz {
    "Asia/Tokyo".parse::<Tz>().expect("tz")
}

fn utc() -> Tz {
    "UTC".parse::<Tz>().expect("tz")
}

// --- ISO 8601 許容ケース表（spec 6.2.2） ---
#[test]
fn accepts_strict_iso8601() {
    let tz = tokyo();
    assert_eq!(
        parse_iso8601("2024-02-03", tz).expect("ok").to_rfc3339(),
        "2024-02-02T15:00:00+00:00"
    );
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56Z", tz).expect("ok").to_rfc3339(),
        "2024-02-03T12:34:56+00:00"
    );
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56+09:00", tz).expect("ok").to_rfc3339(),
        "2024-02-03T03:34:56+00:00"
    );
    // オフセット付きは TZ を尊重しない（spec 6.2.4）
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56.123Z", tz).expect("ok").to_rfc3339(),
        "2024-02-03T12:34:56.123+00:00"
    );
    // 小数秒は任意桁数（chrono の nanosecond 表現に正規化される）
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56.1Z", tz).expect("ok").to_rfc3339(),
        "2024-02-03T12:34:56.100+00:00"
    );
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56.123456Z", tz).expect("ok").to_rfc3339(),
        "2024-02-03T12:34:56.123456+00:00"
    );
}

// --- オフセットなしの時刻は TZ の壁時計時刻（決定済み。以前は拒否） ---
#[test]
fn accepts_time_without_offset_as_tz_wall_clock() {
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56", tokyo()).expect("ok").to_rfc3339(),
        "2024-02-03T03:34:56+00:00"
    );
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56", utc()).expect("ok").to_rfc3339(),
        "2024-02-03T12:34:56+00:00"
    );
    // 小数秒付きのオフセットなしも TZ 壁時計
    assert_eq!(
        parse_iso8601("2024-02-03T12:34:56.123456", tokyo()).expect("ok").to_rfc3339(),
        "2024-02-03T03:34:56.123456+00:00"
    );
}

// --- ISO 8601 拒否ケース表（spec 6.2.2） ---
#[test]
fn rejects_ambiguous_or_invalid_dates() {
    let tz = tokyo();
    for input in [
        "2024-2-3",
        "2024-02-31",
        "2024-02-03T25:00:00",
        "2024-02-03 12:34:56Z",
        "2024/02/03",
        "20240203",
        "2024-W05-3",
        "2024-02-03T12:34",
        "now",
        "-1d",
        "2024-02-03T12:34:60Z",
        "2024-02-03T12:60:00Z",
        "2024-02-03T12:34:56+25:00",
    ] {
        let err = parse_iso8601(input, tz).expect_err("must reject");
        // LLM に返る文言（ToolError 変換後）は英語（spec 6.2.3 の趣旨）
        let llm_message = ToolError::from(err).to_string();
        assert!(
            llm_message.starts_with("Invalid date. Please re-specify as a concrete date in YYYY-MM-DD format:"),
            "unexpected message for {input}: {llm_message}"
        );
    }
}

// --- TZ 境界: Asia/Tokyo と UTC でずれること（design テスト） ---
#[test]
fn date_only_uses_tz_midnight() {
    let tokyo = parse_iso8601("2024-02-03", tokyo()).expect("ok");
    let u = parse_iso8601("2024-02-03", utc()).expect("ok");
    assert_eq!(tokyo.timestamp_millis(), 1706886000000); // 2024-02-02T15:00:00Z（= Asia/Tokyo 0時）
    assert_eq!(u.timestamp_millis(), 1706918400000); // 2024-02-03T00:00:00Z
    assert_ne!(tokyo, u);
}

// --- `@` 正規化と username / host 分割（spec 5.2） ---
#[test]
fn normalizes_handle() {
    assert_eq!(
        normalize_user_ref("@alice@remote.example"),
        Some(normalize_user_ref("alice@remote.example").expect("x"))
    );
    let r = normalize_user_ref("@alice").expect("x");
    assert_eq!(r.username, "alice");
    assert_eq!(r.host, None);
    let r = normalize_user_ref("@alice@remote.example").expect("x");
    assert_eq!(r.username, "alice");
    assert_eq!(r.host.as_deref(), Some("remote.example"));
    assert_eq!(
        handle_string(&r, "misskey.io"),
        "@alice@remote.example"
    );
    assert_eq!(
        handle_string(&normalize_user_ref("alice").expect("x"), "misskey.io"),
        "@alice@misskey.io"
    );
    assert_eq!(normalize_user_ref(""), None);
    assert_eq!(normalize_user_ref("@"), None);
    assert_eq!(normalize_user_ref("@alice@"), None);
}

// --- 排他境界: 0 時ちょうど（境界ちょうど）のノートは除外（spec 6.2.4 / 6.1.5） ---
#[test]
fn exclusive_boundary_excludes_exact_midnight() {
    let after = parse_iso8601("2024-02-03", tokyo()).expect("ok");
    let before = parse_iso8601("2024-02-04", tokyo()).expect("ok");

    let exact = note("n1", "2024-02-02T15:00:00.000Z"); // Asia/Tokyo の 2024-02-03 00:00:00.000
    let inside = note("n2", "2024-02-02T15:00:00.001Z");
    let outside = note("n3", "2024-02-04T14:00:00.000Z");

    assert!(!in_range(&exact, Some(after), Some(before)), "0時ちょうどは除外");
    assert!(in_range(&inside, Some(after), Some(before)));
    assert!(!in_range(&outside, Some(after), Some(before)));
    // createdAt はデシリアライズ時に解析される（解析不能分岐はない）
    assert_eq!(exact.created_at, after);
}

fn note(id: &str, created_at: &str) -> RawNote {
    serde_json::from_value(json!({
        "id": id,
        "createdAt": created_at,
        "visibility": "public",
        "files": []
    }))
    .expect("note")
}
