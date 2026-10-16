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
    pub fn new(config: Config) -> Result<Self, ToolError> {
        Ok(Self {
            tool_router: Self::tool_router(),
            client: Client::new(&config)?,
            config,
        })
    }
}

#[tool_router]
impl MisskeyService {
    #[tool(
        name = "get_timeline",
        description = "Returns the latest notes from a Misskey timeline (type: home | local | global, default local), sorted by createdAt descending. No date parameters are accepted. In the response, cw is the content-warning text and text is the body folded behind it in the UI; isHidden means the note is hidden by policy. Results may be truncated: the response is a page object carrying truncated, nextCursor (the untilId to continue) and an English message."
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
        description = "Returns the profile and recent notes of a user (default: the authenticated user of MISSKEY_TOKEN). username also accepts @username@host, split server-side into username / host; passing an embedded host together with an explicit host parameter is an error. Returns the user object directly, without a page wrapper. In the response, cw is the content-warning text and text is the body folded behind it in the UI; isHidden means the note is hidden by policy."
    )]
    async fn get_user_info(&self, params: Parameters<GetUserParams>) -> Result<CallToolResult, ErrorData> {
        Ok(render(tools::get_user_info(&self.client, &self.config, &params.0).await))
    }

    #[tool(
        name = "get_user_notes",
        description = "Returns a user's notes (default: the authenticated user of MISSKEY_TOKEN), sorted by createdAt descending. Date input is ISO 8601 (YYYY-MM-DD, optional time; offset optional, no-offset means TZ wall-clock); relative expressions (now, -1d) are not accepted. after / before are exclusive boundaries. In the response, cw is the content-warning text and text is the body folded behind it in the UI; isHidden means the note is hidden by policy. Results may be truncated: the response is a page object carrying truncated, nextCursor (the untilId to continue) and an English message."
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
