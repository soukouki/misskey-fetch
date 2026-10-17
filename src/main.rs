use std::env;
use std::process;

use misskey_fetch::cli::{CliError, TransportKind, parse};
use misskey_fetch::config::Config;
use misskey_fetch::service::MisskeyService;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let cli = match parse(&args) {
        Ok(cli) => cli,
        Err(CliError::Help) => {
            println!("{}", CliError::Help);
            return Ok(());
        }
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };

    let config = Config::from_env()?;
    eprintln!(
        "misskey-fetch: transport={} api_base={} tz={}",
        cli.transport.name(),
        config.api_base,
        config.tz
    );
    if cli.ignored_bind {
        eprintln!("misskey-fetch: --host / --port are ignored for stdio transport");
    }

    match cli.transport {
        // stdout は JSON-RPC のみ、ログは stderr
        TransportKind::Stdio => {
            let service = MisskeyService::new(config)?;
            let server = service.serve(rmcp::transport::stdio()).await?;
            server.waiting().await?;
        }
        TransportKind::Sse => misskey_fetch::sse::serve(config, &cli.host, cli.port).await?,
        TransportKind::Http => misskey_fetch::http::serve(config, &cli).await?,
    }
    Ok(())
}
