use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Serialize;

use crate::config::Config;
use crate::errors::ToolError;
use crate::misskey::client::Client;
use crate::params::{GetTimelineParams, GetUserNotesParams, GetUserParams};
use crate::tools;

pub struct MisskeyService {
    #[expect(dead_code, reason = "tool_handler マクロが参照する")]
    tool_router: ToolRouter<Self>,
    client: Client,
    config: Config,
}

impl MisskeyService {
    pub fn new(config: Config) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Client::new(&config),
            config,
        }
    }
}

#[tool_router]
impl MisskeyService {
    #[tool(
        name = "get_timeline",
        description = "Misskey のタイムラインの最新投稿を createdAt 降順で返す。type は home | local | global（省略時は local）。日付引数は受け取らない。"
    )]
    async fn get_timeline(
        &self,
        params: Parameters<GetTimelineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(render(
            tools::get_timeline(&self.client, &self.config, &params.0).await,
        ))
    }

    #[tool(
        name = "get_user_info",
        description = "特定ユーザー（省略時は MISSKEY_TOKEN に紐づく認証済みユーザー）のプロフィールと最近の投稿を返す。username は @username@host 形式でも受け入れ、サーバー側で username / host に分割する。"
    )]
    async fn get_user_info(&self, params: Parameters<GetUserParams>) -> Result<CallToolResult, ErrorData> {
        Ok(render(tools::get_user_info(&self.client, &self.config, &params.0).await))
    }

    #[tool(
        name = "get_user_notes",
        description = "特定ユーザー（または自分）の投稿一覧を createdAt 降順で返す。after / before は排他境界。日付は ISO 8601（YYYY-MM-DD）で渡すこと。相対表現（now, -1d 等）は渡さないこと。"
    )]
    async fn get_user_notes(
        &self,
        params: Parameters<GetUserNotesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(render(
            tools::get_user_notes(&self.client, &self.config, &params.0).await,
        ))
    }
}

#[tool_handler]
impl ServerHandler for MisskeyService {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

fn render<T: Serialize>(result: Result<T, ToolError>) -> CallToolResult {
    match result {
        Ok(value) => match serde_json::to_value(&value) {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
        },
        // LLM 向けエラーは JSON-RPC エラーではなく isError: true で返す（spec 6.2.3）
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
    }
}
