use misskey_fetch::config::Config;
use misskey_fetch::service::MisskeyService;
use rmcp::ServiceExt;

/// stdio トランスポート: stdout は JSON-RPC のみ、ログは stderr（spec 6.4）。
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    eprintln!(
        "misskey-fetch: api_base={} tz={}",
        config.api_base, config.tz
    );

    let service = MisskeyService::new(config)?;
    let server = service.serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}
