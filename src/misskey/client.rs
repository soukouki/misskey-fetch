use std::time::Duration;

use serde_json::Value;

use crate::config::Config;
use crate::errors::ToolError;

const MAX_RETRIES: u32 = 3;
const BACKOFF_BASE_MS: u64 = 200;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    api_base: String,
    token: String,
}

impl Client {
    pub fn new(cfg: &Config) -> Result<Self, ToolError> {
        Self::with_base(cfg, cfg.api_base.clone())
    }

    /// 結合テスト（wiremock）用にベース URL を差し替えられる。
    pub fn with_base(cfg: &Config, api_base: String) -> Result<Self, ToolError> {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            // token の別 origin 漏洩防止（spec 6.3）
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ToolError::Http {
                message: e.to_string(),
            })?;
        Ok(Self {
            http,
            // 末尾スラッシュが残ると `{base}/{endpoint}` が `//endpoint` になるため、ここで除去する。
            api_base: api_base.trim().trim_end_matches('/').to_string(),
            token: cfg.token.clone(),
        })
    }

    /// Misskey API（read エンドポイントのみ）を POST（JSON）で呼ぶ。
    /// read-only の保証はコードが read エンドポイントしか呼ばないことから得られる（spec 7）。
    pub async fn call(&self, endpoint: &str, body: &Value, handle: &str) -> Result<Value, ToolError> {
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
                    tokio::time::sleep(Duration::from_millis(BACKOFF_BASE_MS * (1u64 << attempt))).await;
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
