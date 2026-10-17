//! Streamable HTTP トランスポート（レガシー HTTP+SSE の後継、spec 2025-03-26 以降）。
//!
//! rmcp の `StreamableHttpService`（tower の `Service`）を hyper の接続で動かす。
//! rmcp はパスを見ないので URL は任意（慣例的に `/mcp`）。
//!
//! - `POST`   → JSON-RPC。応答は JSON、または SSE ストリーム
//! - `GET`    → セッションに紐づく SSE ストリーム
//! - `DELETE` → セッションクローズ
//!
//! `Host` 検証（DNS rebinding 対策）は既定で無効。任意の Host を受け付ける。

use std::sync::Arc;

use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::net::TcpListener;
use tower_service::Service;

use crate::cli::Cli;
use crate::config::Config;
use crate::service::MisskeyService;

const MAX_BODY_BYTES: usize = 1 << 20;

pub async fn serve(config: Config, cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind((cli.host.as_str(), cli.port)).await?;
    eprintln!(
        "misskey-fetch: http listening on http://{}:{} (any Host accepted)",
        cli.host, cli.port
    );

    let service = StreamableHttpService::new(
        move || {
            MisskeyService::new(config.clone()).map_err(|e| std::io::Error::other(e.to_string()))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_max_request_body_bytes(MAX_BODY_BYTES)
            .disable_allowed_hosts(),
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let service = service.clone();
        tokio::spawn(async move {
            let handler = service_fn(move |req| {
                let mut service = service.clone();
                async move { service.call(req).await }
            });
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, handler)
                .await
            {
                eprintln!("http connection error: {e}");
            }
        });
    }
}
