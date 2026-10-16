use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use misskey_fetch::config::Config;
use misskey_fetch::errors::ToolError;
use misskey_fetch::misskey::client::Client;
use misskey_fetch::params::{GetUserNotesParams, GetUserParams};
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
    let client = Client::with_base(&config, config.api_base.clone());
    let p = GetUserNotesParams {
        username: Some("alice".to_string()),
        host: None,
        after: Some("2024-02-03".to_string()),
        before: Some("2024-02-04".to_string()),
        limit: None,
        include_renotes: None,
    };
    let notes = tools::get_user_notes(&client, &config, &p).await.expect("ok");

    // createdAt 再検証で範囲外が落ちる（境界ちょうど 0 時は排他）
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].handle, "@alice@mock.test");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[1].body_json().expect("body");
    // 主経路: sinceDate / untilDate（epoch ms）
    assert_eq!(body["sinceDate"], 1706886000000i64); // Asia/Tokyo 2024-02-03 00:00
    assert_eq!(body["untilDate"], 1706972400000i64); // Asia/Tokyo 2024-02-04 00:00
    assert_eq!(body["limit"], 100);
}

#[tokio::test]
async fn limit_is_clamped_to_100() {
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
    let client = Client::with_base(&config, config.api_base.clone());
    let p = GetUserNotesParams {
        username: None,
        host: None,
        after: None,
        before: None,
        limit: Some(500),
        include_renotes: None,
    };
    tools::get_user_notes(&client, &config, &p).await.expect("ok");
    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[1].body_json().expect("body");
    assert_eq!(body["limit"], 100);
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
    let client = Client::with_base(&config, config.api_base.clone());
    let p = GetUserParams {
        username: Some("@bob@remote.example".to_string()),
        host: None,
    };
    let user = tools::get_user_info(&client, &config, &p).await.expect("ok");
    assert_eq!(user.handle, "@bob@remote.example");
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
async fn get_timeline_sorts_desc_by_created_at() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/notes/local-timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "zz", "createdAt": "2024-02-01T00:00:00.000Z", "text": "older", "user": { "id": "u1", "name": "A", "username": "a", "host": "remote.example" } },
            { "id": "aa", "createdAt": "2024-02-03T00:00:00.000Z", "text": "newer", "user": { "id": "u2", "name": "B", "username": "b", "host": null } }
        ])))
        .mount(&server)
        .await;

    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone());
    let p = misskey_fetch::params::GetTimelineParams { r#type: None, limit: Some(200) };
    let notes = tools::get_timeline(&client, &config, &p).await.expect("ok");
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].handle, "@b@mock.test");
    assert_eq!(notes[0].note_type, "note");
    assert_eq!(notes[1].handle, "@a@remote.example");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = requests[0].body_json().expect("body");
    assert_eq!(body["limit"], 100);
}

#[tokio::test]
async fn allowlist_blocks_write_endpoints_without_calling_them() {
    let server = MockServer::start().await;
    let config = cfg(&format!("{}/api", server.uri()));
    let client = Client::with_base(&config, config.api_base.clone());

    for endpoint in ["drive", "announcements/read", "notes/follow"] {
        let err = client.call(endpoint, &json!({}), "").await.expect_err("must be blocked");
        assert!(matches!(err, ToolError::DisallowedEndpoint { .. }), "{err}");
    }
    // allowlist 外は呼ばれていない（リクエスト数は 0）
    assert!(server.received_requests().await.expect("requests").is_empty());
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
    let client = Client::with_base(&config, config.api_base.clone());
    let p = GetUserParams {
        username: Some("@ghost@mock.test".to_string()),
        host: None,
    };
    let err = tools::get_user_info(&client, &config, &p).await.expect_err("error");
    assert!(matches!(err, ToolError::NoSuchUser { .. }), "{err}");
    assert!(err.to_string().contains("@ghost@mock.test"));
}
