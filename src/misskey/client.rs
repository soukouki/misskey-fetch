use std::time::Duration;

use serde_json::Value;

use crate::config::Config;
use crate::errors::ToolError;

/// 呼び出し可能エンドポイント（spec 7）。書き込み系は含まない。
pub const ALLOWED_ENDPOINTS: &[&str] = &[
    "meta",
    "users/me",
    "users/show",
    "users/notes",
    "notes/timeline",
    "notes/local-timeline",
    "notes/global-timeline",
];

const MAX_RETRIES: u32 = 3;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    api_base: String,
    token: String,
}

impl Client {
    pub fn new(cfg: &Config) -> Self {
        Self::with_base(cfg, cfg.api_base.clone())
    }

    /// 結合テスト（wiremock）用にベース URL を差し替えられる。
    pub fn with_base(cfg: &Config, api_base: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            // token の別 origin 漏洩防止（spec 6.3）
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            api_base,
            token: cfg.token.clone(),
        }
    }

    /// allowlist を強制し、Misskey API を POST（JSON）で呼ぶ。
    pub async fn call(&self, endpoint: &str, body: &Value, handle: &str) -> Result<Value, ToolError> {
        if !ALLOWED_ENDPOINTS.contains(&endpoint) {
            return Err(ToolError::DisallowedEndpoint {
                endpoint: endpoint.to_string(),
            });
        }

        let url = format!("{}/{}", self.api_base, endpoint);
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.token))
                .json(body)
                .send()
                .await
                .map_err(|e| ToolError::Http {
                    message: e.to_string(),
                })?;

            let status = response.status();
            if status.as_u16() == 429 {
                if attempt < MAX_RETRIES {
                    // stderr には token を出さない（spec 6.3 / 6.4）
                    eprintln!("rate limit (429), backoff retry {attempt}");
                    tokio::time::sleep(Duration::from_millis(200 * (attempt as u64 + 1))).await;
                    continue;
                }
                return Err(ToolError::RateLimited);
            }

            if status.is_success() {
                return response.json::<Value>().await.map_err(|e| ToolError::Http {
                    message: e.to_string(),
                });
            }

            let text = response.text().await.unwrap_or_default();
            let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let error = parsed.get("error").unwrap_or(&parsed);
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or(&text);
            return Err(ToolError::api(status.as_u16(), code, message, handle));
        }

        Err(ToolError::RateLimited)
    }
}
