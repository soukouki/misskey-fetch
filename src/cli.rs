use thiserror::Error;

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8080;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Stdio,
    Sse,
    /// MCP の streamable HTTP（レガシー SSE の後継）
    Http,
}

impl TransportKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "stdio" => Some(Self::Stdio),
            "sse" => Some(Self::Sse),
            "http" | "streamable-http" => Some(Self::Http),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Sse => "sse",
            Self::Http => "http",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cli {
    pub transport: TransportKind,
    pub host: String,
    pub port: u16,
    /// `--host` / `--port` が stdio で渡された（stdio では効かない）
    pub ignored_bind: bool,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("usage: misskey-fetch [--transport stdio|sse|http] [--host <addr>] [--port <port>]\n\n  --transport      stdio (default) | sse | http (streamable HTTP)\n  --host           HTTP bind address (default 127.0.0.1)\n  --port           HTTP port (default 8080)\n  -h, --help       show this")]
    Help,
    #[error("missing value for {flag}")]
    MissingValue { flag: String },
    #[error("invalid value for {flag}: {value} (expected stdio, sse or http)")]
    InvalidTransport { flag: String, value: String },
    #[error("invalid port: {value}")]
    InvalidPort { value: String },
    #[error("unknown argument: {arg}")]
    Unknown { arg: String },
}

/// `--transport sse --port 3000` / `--transport=sse` 形式を受け付ける。
pub fn parse(args: &[String]) -> Result<Cli, CliError> {
    let mut transport = TransportKind::Stdio;
    let mut host = DEFAULT_HOST.to_string();
    let mut port = DEFAULT_PORT;
    let mut bind_given = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        // `--flag=value` 形式
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        match flag {
            "-h" | "--help" => return Err(CliError::Help),
            "--transport" => {
                let value = take(&mut it, flag, inline)?;
                transport = TransportKind::parse(&value)
                    .ok_or_else(|| CliError::InvalidTransport {
                        flag: flag.to_string(),
                        value,
                    })?;
            }
            "--host" => {
                host = take(&mut it, flag, inline)?;
                bind_given = true;
            }
            "--port" => {
                let value = take(&mut it, flag, inline)?;
                port = value.parse::<u16>().map_err(|_| CliError::InvalidPort { value })?;
                bind_given = true;
            }
            other => return Err(CliError::Unknown {
                arg: other.to_string(),
            }),
        }
    }

    Ok(Cli {
        transport,
        host,
        port,
        ignored_bind: bind_given && transport == TransportKind::Stdio,
    })
}

fn take<'a, I>(it: &mut I, flag: &str, inline: Option<String>) -> Result<String, CliError>
where
    I: Iterator<Item = &'a String>,
{
    if let Some(v) = inline {
        return Ok(v);
    }
    it.next()
        .map(|v| v.to_string())
        .ok_or_else(|| CliError::MissingValue {
            flag: flag.to_string(),
        })
}
