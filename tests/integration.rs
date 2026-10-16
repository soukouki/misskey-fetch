use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use misskey_fetch::config::Config;
use misskey_fetch::errors::ToolError;
use misskey_fetch::misskey::client::Client;
use misskey_fetch::misskey::types::NoteType;
use misskey_fetch::params::{GetUserNotesParams, GetUserParams, GetTimelineParams};
use misskey_fetch::tools;

fn cfg(base: &str) -> Config {
    Config {
        token: "test-token".to_string(),
        api_base: base.to_string(),
        instance_host: "mock.test".to_string(),
        tz: "Asia/Tokyo".parse().expect("tz"),
    }
}

fn user(id: &str, username: &str, host: Option<&str>) -> Value {
    json!({
        "id": id,
        "name": "Alice",
        "username": username,
        "host": host,
        "description": "hello",
        "pinnedNotes": []
    })
}

fn note(id: &str, created_at: &str, text: &str) -> Value {
    json!({ "id": id, "createdAt": created_at, "text": text, "cw": null, "visibility": "public", "files": [] })
}

/// 100件ページ（createdAt 降順、last = n100）
fn hundred_notes() -> Vec<Value> {
    (0..100)
        .map(|i| {
            let id = format!("n{:03}", i + 1);
            let created = format!(
                "2024-02-03T{:02}:{:02}:00.000Z",
                23 - i / 60,
                59 - i % 60
            );
            note(&id, &created, "x")
        })
        .collect()
}

#[tokio::test]
async fn get_user_notes_passes_date_boundaries_and_reverifies_created_at() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u1", "alice", None)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            note("n1", "2024-02-03T12:00:00.000Z", "inside"),
            note("n2", "2024-02-02T15:00:00.000Z", "exactly boundary -> excluded"),
            note("n3", "2024-02-01T12:00:00.000Z", "before after -> excluded")
        ])))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserNotesParams {
        handle: Some("@alice".to_string()),
        after: Some("2024-02-03".to_string()),
        before: Some("2024-02-04".to_string()),
        limit: None,
        include_renotes: None,
    };
    let page = tools::get_user_notes(&client, &config, &p).await.expect("ok");

    // createdAt 再検証で範囲外が落ちる（境界ちょうど 0 時は排他）
    assert_eq!(page.notes.len(), 1, "{page:?}");
    assert_eq!(page.notes[0].handle, "@alice@mock.test");

    // 境界到達で要求件数（100）を満たさずに停止 -> truncated / message
    assert!(page.truncated);
    assert_eq!(page.next_cursor.as_deref(), Some("n1"));
    let message = page.message.as_deref().expect("message");
    assert!(
        message.starts_with("Showing 1 notes of 100 requested; the rest is omitted."),
        "{message}"
    );
    assert!(message.contains("untilId=\"n1\""), "{message}");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[1].body_json().expect("body");
    // 主経路: sinceDate / untilDate（epoch ms）
    assert_eq!(body["sinceDate"], 1706886000000i64); // Asia/Tokyo 2024-02-03 00:00
    assert_eq!(body["untilDate"], 1706972400000i64); // Asia/Tokyo 2024-02-04 00:00
    assert_eq!(body["limit"], 100);
}

#[tokio::test]
async fn limit_is_clamped_to_page_size() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u1", "me", None)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserNotesParams {
        handle: None,
        after: None,
        before: None,
        limit: Some(500),
        include_renotes: None,
    };
    let page = tools::get_user_notes(&client, &config, &p).await.expect("ok");
    // 1リクエストの page size は 100（要求 500 は clamp されない）
    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[1].body_json().expect("body");
    assert_eq!(body["limit"], 100);
    assert!(!page.truncated);
    assert!(page.message.is_none());
}

#[tokio::test]
async fn limit_is_clamped_to_1000_and_reports_truncation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u1", "alice", None)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(hundred_notes())))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserNotesParams {
        handle: Some("@alice".to_string()),
        after: None,
        before: None,
        limit: Some(1200),
        include_renotes: None,
    };
    let page = tools::get_user_notes(&client, &config, &p).await.expect("ok");

    // 要求 1200 は 1000 に clamp。100 x 10 ページ = 1000
    assert_eq!(page.notes.len(), 1000);
    assert!(page.truncated);
    assert_eq!(page.next_cursor.as_deref(), Some("n100"));
    assert_eq!(
        page.message.as_deref().expect("message"),
        "Showing 1000 notes of 1200 requested; the rest is omitted. Call again with untilId=\"n100\" to continue."
    );

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 11); // users/show + 10 pages
    let first: Value = requests[1].body_json().expect("body");
    assert_eq!(first["limit"], 100);
    assert!(first.get("untilId").is_none());
    let second: Value = requests[2].body_json().expect("body");
    assert_eq!(second["untilId"], "n100");
}

#[tokio::test]
async fn paginates_with_until_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u1", "alice", None)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(|req: &Request| {
            let body: Value = req.body_json().expect("body");
            if body.get("untilId").is_some() {
                ResponseTemplate::new(200).set_body_json(json!([
                    note("n3", "2024-02-01T12:00:00.000Z", "page 2"),
                    note("n4", "2024-01-31T12:00:00.000Z", "page 2")
                ]))
            } else {
                ResponseTemplate::new(200).set_body_json(json!([
                    note("n1", "2024-02-03T12:00:00.000Z", "page 1"),
                    note("n2", "2024-02-02T12:00:00.000Z", "page 1")
                ]))
            }
        })
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserNotesParams {
        handle: Some("@alice".to_string()),
        after: None,
        before: None,
        limit: Some(4),
        include_renotes: None,
    };
    let page = tools::get_user_notes(&client, &config, &p).await.expect("ok");

    assert_eq!(page.notes.len(), 4);
    // 要求件数を満たしたので truncated ではない
    assert!(!page.truncated);
    assert!(page.next_cursor.is_none());
    assert!(page.message.is_none());
    assert_eq!(page.notes[0].handle, "@alice@mock.test");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 3); // users/show + 2 pages
    let first: Value = requests[1].body_json().expect("body");
    assert!(first.get("untilId").is_none());
    assert_eq!(first["limit"], 4);
    let second: Value = requests[2].body_json().expect("body");
    // 2ページ目の untilId は 1ページ目の最後のノート
    assert_eq!(second["untilId"], "n2");
    assert_eq!(second["limit"], 2);
}

#[tokio::test]
async fn get_user_info_splits_at_handle() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u2", "bob", Some("remote.example"))))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([note("n1", "2024-02-03T12:00:00.000Z", "hi")])))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserParams {
        handle: Some("@bob@remote.example".to_string()),
    };
    let user = tools::get_user_info(&client, &config, &p).await.expect("ok");
    assert_eq!(user.handle, "@bob@remote.example");
    assert_eq!(user.name.as_deref(), Some("Alice"));
    // get_user_info はページラッパーなしで User object を返す
    assert_eq!(user.recent_notes.len(), 1);

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[0].body_json().expect("body");
    // `@user@host` を username にそのまま渡してはいけない（spec 5.2）
    assert_eq!(body["username"], "bob");
    assert_eq!(body["host"], "remote.example");
    let notes_body: Value = requests[1].body_json().expect("body");
    assert_eq!(notes_body["limit"], 10);
}

#[tokio::test]
async fn handle_equal_to_instance_host_is_normalized_to_local() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user("u1", "sou7", None)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/users/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserParams {
        handle: Some("@sou7@mock.test".to_string()),
    };
    let u = tools::get_user_info(&client, &config, &p).await.expect("ok");
    assert_eq!(u.handle, "@sou7@mock.test");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[0].body_json().expect("body");
    // Misskey はローカルユーザーを host: null で保存するため、自ホストは null として渡す
    assert_eq!(body["username"], "sou7");
    assert_eq!(body["host"], Value::Null);
}

#[tokio::test]
async fn get_timeline_sorts_desc_by_created_at() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/notes/local-timeline"))
        .respond_with(|req: &Request| {
            let body: Value = req.body_json().expect("body");
            if body.get("untilId").is_some() {
                ResponseTemplate::new(200).set_body_json(json!([])) // 2ページ目以降は空（データ尽き）
            } else {
                ResponseTemplate::new(200).set_body_json(json!([
                    { "id": "zz", "createdAt": "2024-02-01T00:00:00.000Z", "text": "older", "visibility": "public", "files": [], "isHidden": true, "user": { "id": "u1", "name": null, "username": "a", "host": "remote.example" } },
                    { "id": "aa", "createdAt": "2024-02-03T00:00:00.000Z", "text": "newer", "cw": "warning", "visibility": "home", "files": [], "user": { "id": "u2", "name": "B", "username": "b", "host": null } }
                ]))
            }
        })
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetTimelineParams { r#type: None, limit: Some(200) };
    let page = tools::get_timeline(&client, &config, &p).await.expect("ok");

    assert_eq!(page.notes.len(), 2);
    assert_eq!(page.notes[0].handle, "@b@mock.test");
    assert_eq!(page.notes[0].note_type, NoteType::Note);
    assert_eq!(page.notes[0].visibility, misskey_fetch::misskey::types::Visibility::Home);
    assert!(page.notes[0].has_cw);
    assert_eq!(page.notes[1].handle, "@a@remote.example");
    assert_eq!(page.notes[1].name, None);
    assert!(page.notes[1].is_hidden);
    assert!(!page.truncated);

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[0].body_json().expect("body");
    assert_eq!(body["limit"], 100);
}

#[tokio::test]
async fn no_such_user_is_mapped_to_llm_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/users/show"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "code": "NO_SUCH_USER", "message": "No such user" }
        })))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone()).expect("client");
    let p = GetUserParams {
        handle: Some("@ghost@mock.test".to_string()),
    };
    let err = tools::get_user_info(&client, &config, &p).await.expect_err("error");
    assert!(matches!(err, ToolError::NoSuchUser { .. }), "{err}");
    assert!(err.to_string().contains("@ghost@mock.test"));
}
