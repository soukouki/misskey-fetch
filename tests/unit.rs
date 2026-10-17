use chrono_tz::Tz;
use serde_json::json;

use misskey_fetch::cli::{parse, CliError, TransportKind};
use misskey_fetch::http::allowed_hosts;
use misskey_fetch::dates::parse_iso8601;
use misskey_fetch::errors::ToolError;
use misskey_fetch::config::{host_of, normalize_api_base, Config};
use misskey_fetch::handle::{handle_string, normalize_host, parse_handle};
use misskey_fetch::misskey::types::{Note, in_range};

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
    assert_eq!(tokyo.timestamp_millis(), 1706886000000i64); // 2024-02-02T15:00:00Z（= Asia/Tokyo 0時）
    assert_eq!(u.timestamp_millis(), 1706918400000i64); // 2024-02-03T00:00:00Z
    assert_ne!(tokyo, u);
}

// --- `@` 正規化と username / host 分割（spec 5.2、決定5） ---
#[test]
fn normalizes_handle() {
    let r = parse_handle("@alice@remote.example", "misskey.io").expect("x");
    assert_eq!(r.username, "alice");
    assert_eq!(r.host.as_deref(), Some("remote.example"));
    assert_eq!(handle_string(&r, "misskey.io"), "@alice@remote.example");
    // 表示は常に完全形
    assert_eq!(
        handle_string(&parse_handle("@alice", "misskey.io").expect("x"), "misskey.io"),
        "@alice@misskey.io"
    );
    for bad in ["", "@", "@alice@", "@@remote.example"] {
        assert!(parse_handle(bad, "misskey.io").is_err(), "{bad}");
    }
}

// --- `@` なしの入力は拒否（決定5） ---
#[test]
fn rejects_handle_without_at() {
    let err = parse_handle("alice", "misskey.io").expect_err("must reject");
    assert_eq!(
        err.to_string(),
        "Invalid handle \"alice\": must be @username or @username@host"
    );
}

// --- host 省略はインスタンスホストで補完（決定5） ---
#[test]
fn handle_without_host_completed_to_instance_host() {
    let r = parse_handle("@sou7", "misskey.io").expect("x");
    assert_eq!(r.username, "sou7");
    assert_eq!(r.host, None); // 内部表現ではローカル
    assert_eq!(handle_string(&r, "misskey.io"), "@sou7@misskey.io");
    // インスタンスホストを明示した場合もローカルに正規化（users/show は null を要求）
    assert_eq!(parse_handle("@sou7@misskey.io", "misskey.io").expect("x").host, None);
}

// --- lookup path の正規化は lowercase + punycode（spec 4.4） ---
#[test]
fn normalizes_host_with_punycode() {
    // `RemoteUserResolveService.resolveUser` は username.toLowerCase() と toPuny(host) を使う
    let r = parse_handle("@Alice@Remote.Example", "misskey.io").expect("x");
    assert_eq!(r.username, "alice");
    assert_eq!(r.host.as_deref(), Some("remote.example"));
    assert_eq!(normalize_host("EXAMPLE.COM"), Some("example.com".to_string()));
    // 非ASCIIホストは punycode に変換される
    assert_eq!(normalize_host("例.com"), Some("xn--fsq.com".to_string()));
    assert_eq!(normalize_host("xn--fsq.example"), Some("xn--fsq.example".to_string()));
    // 自ホストは punycode 比較でローカルに正規化される
    assert_eq!(parse_handle("@sou7@例.example", "xn--fsq.example").expect("x").host, None);
    // display path は stored 値を使う（input の case を反映させない）
    assert_eq!(handle_string(&r, "misskey.io"), "@alice@remote.example");
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

fn note(id: &str, created_at: &str) -> Note {
    serde_json::from_value(json!({
        "id": id,
        "createdAt": created_at,
        "visibility": "public",
        "files": [],
        "user": { "id": "u1", "username": "a", "host": null }
    }))
    .expect("note")
}

// --- CLI ---
fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn cli_defaults_to_stdio() {
    let cli = parse(&args(&["--port", "3000"])).expect("ok");
    assert_eq!(cli.transport, TransportKind::Stdio);
    // stdio では bind 引数は効かない（main が stderr に警告を出す）
    assert!(cli.ignored_bind);
}

#[test]
fn cli_accepts_sse_with_host_and_port() {
    let cli = parse(&args(&["--transport", "sse", "--host", "0.0.0.0", "--port", "3000"])).expect("ok");
    assert_eq!(cli.transport, TransportKind::Sse);
    assert_eq!(cli.host, "0.0.0.0");
    assert_eq!(cli.port, 3000);
    assert!(!cli.ignored_bind);

    // `--flag=value` 形式
    let eq = parse(&args(&["--transport=sse", "--port=3000"])).expect("ok");
    assert_eq!(eq.transport, TransportKind::Sse);
    assert_eq!(eq.port, 3000);

    let d = parse(&args(&[])).expect("ok");
    assert_eq!(d.transport, TransportKind::Stdio);
    assert_eq!(d.host, "127.0.0.1");
    assert_eq!(d.port, 8080);
}

#[test]
fn cli_accepts_http_transport_with_allowed_hosts() {
    let cli = parse(&args(&[
        "--transport",
        "http",
        "--host",
        "0.0.0.0",
        "--port",
        "9293",
        "--allowed-host",
        "Misskey-Fetch",
    ]))
    .expect("ok");
    assert_eq!(cli.transport, TransportKind::Http);
    assert_eq!(cli.host, "0.0.0.0");
    assert_eq!(cli.port, 9293);
    assert_eq!(cli.allowed_hosts, vec!["misskey-fetch"]);
    assert!(!cli.ignored_bind);

    // alias と `--flag=value` 形式
    let eq = parse(&args(&["--transport=streamable-http", "--allowed-host=example.test"])).expect("ok");
    assert_eq!(eq.transport, TransportKind::Http);
    assert_eq!(eq.allowed_hosts, vec!["example.test"]);

    // 既定（ループバック）は http 側で加えるので、CLI 側は空
    assert!(parse(&args(&["--transport", "http"])).expect("ok").allowed_hosts.is_empty());
}

#[test]
fn http_allowed_hosts_are_loopback_plus_extras() {
    let cli = parse(&args(&["--transport", "http", "--allowed-host", "misskey-fetch"])).expect("ok");
    assert_eq!(
        allowed_hosts(&cli),
        vec!["localhost", "127.0.0.1", "::1", "misskey-fetch"]
    );
    assert_eq!(
        allowed_hosts(&parse(&args(&["--transport", "http"])).expect("ok")),
        vec!["localhost", "127.0.0.1", "::1"]
    );
}

#[test]
fn cli_rejects_bad_args() {
    assert!(matches!(parse(&args(&["--port", "abc"])), Err(CliError::InvalidPort { .. })));
    assert!(matches!(
        parse(&args(&["--transport", "wss"])),
        Err(CliError::InvalidTransport { .. })
    ));
    assert!(matches!(parse(&args(&["--port"])), Err(CliError::MissingValue { .. })));
    assert!(matches!(parse(&args(&["-x"])), Err(CliError::Unknown { .. })));
    assert!(matches!(parse(&args(&["--help"])), Err(CliError::Help)));
}

// --- MISSKEY_API_URL の末尾スラッシュ（spec 6.3 の正規化） ---
#[test]
fn strips_trailing_slashes_from_api_base() {
    let norm = |raw: &str| normalize_api_base(raw).map(|s| s.to_string());
    assert_eq!(norm("https://misskey.example/api/"), Some("https://misskey.example/api".to_string()));
    assert_eq!(norm("https://misskey.example/api///"), Some("https://misskey.example/api".to_string()));
    assert_eq!(norm("https://misskey.example/"), Some("https://misskey.example".to_string()));
    assert_eq!(norm("  https://misskey.example/api/  "), Some("https://misskey.example/api".to_string()));
    // 正規化後のホスト部も末尾スラッシュの影響を受けない
    assert_eq!(host_of("https://misskey.example/api"), Some("misskey.example".to_string()));
    // 不正形は従来どおり拒否
    assert_eq!(norm("http://misskey.example/api/"), None);
    assert_eq!(norm("https://"), None);
}

#[test]
fn config_from_env_normalizes_trailing_slash() {
    unsafe {
        std::env::set_var("MISSKEY_TOKEN", "test-token");
        std::env::set_var("MISSKEY_API_URL", "https://misskey.example/api/");
        std::env::set_var("TZ", "Asia/Tokyo");
    }
    let cfg = Config::from_env().expect("config");
    assert_eq!(cfg.api_base, "https://misskey.example/api");
    assert_eq!(cfg.instance_host, "misskey.example");
}
