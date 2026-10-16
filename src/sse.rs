//! SSE（HTTP + Server-Sent Events）トランスポート。
//!
//! rmcp 3.5.1 には SSE サーバートランスポートがない（`transport-sse-server` は #562 で削除され、
//! 後継は streamable HTTP）。レガシー HTTP+SSE プロトコルをこのファイルで自前実装する:
//!
//! - `GET /sse` → `event: endpoint` で message endpoint を伝え、以降は `event: message` で応答を流す
//! - `POST /messages?sessionId=...` → JSON-RPC を受け取り `202 Accepted`、応答は SSE 側に出る
//!
//! rmcp の service loop は [`Transport`] 上で動かす。チャネルトランスポートを介して
//! HTTP ハンドラと service を接続する。

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use rmcp::{RoleServer, ServiceExt};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::service::MisskeyService;

const CHANNEL_CAPACITY: usize = 64;
const MAX_BODY_BYTES: usize = 1 << 20;

type ResBody = BoxBody<Bytes, Infallible>;

#[derive(Debug, Error)]
#[error("SSE session is closed")]
struct SessionClosed;

struct Session {
    client_tx: mpsc::Sender<ClientJsonRpcMessage>,
}

type Registry = Arc<Mutex<HashMap<String, Arc<Session>>>>;

/// HTTP ハンドラと rmcp service loop を結ぶチャネルトランスポート。
struct ChannelTransport {
    rx: mpsc::Receiver<ClientJsonRpcMessage>,
    tx: mpsc::Sender<ServerJsonRpcMessage>,
}

impl Transport<RoleServer> for ChannelTransport {
    type Error = SessionClosed;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let tx = self.tx.clone();
        async move { tx.send(item).await.map_err(|_| SessionClosed) }
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        async move { self.rx.recv().await }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async { Ok(()) }
    }
}

pub async fn serve(config: Config, host: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
    let listener = TcpListener::bind((host, port)).await?;
    eprintln!("misskey-fetch: sse listening on http://{host}:{port}/sse");

    loop {
        let (stream, _) = listener.accept().await?;
        let registry = registry.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |req| {
                let registry = registry.clone();
                let config = config.clone();
                async move { handle(req, registry, config).await }
            });
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                // SSE 接続が切れると hyper はこれを返す。セッション掃除は SseBody::Drop で行うので記録しない。
                if !e.to_string().contains("connection closed before message completed") {
                    eprintln!("sse connection error: {e}");
                }
            }
        });
    }
}

async fn handle(req: Request<Incoming>, registry: Registry, config: Config) -> Result<Response<ResBody>, Infallible> {
    let response = match (req.method(), req.uri().path()) {
        (&Method::GET, "/sse") => open_session(registry, config).await,
        // 公式 SDK は `/messages` と `/message` の両方を使う
        (&Method::POST, "/messages") | (&Method::POST, "/message") => post_message(req, &registry).await,
        _ => error_response(StatusCode::NOT_FOUND, "not found"),
    };
    Ok(response)
}

async fn open_session(registry: Registry, config: Config) -> Response<ResBody> {
    let id = uuid::Uuid::new_v4().to_string();
    let (client_tx, client_rx) = mpsc::channel::<ClientJsonRpcMessage>(CHANNEL_CAPACITY);
    let (out_tx, out_rx) = mpsc::channel::<ServerJsonRpcMessage>(CHANNEL_CAPACITY);
    let (frame_tx, frame_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

    registry.lock().unwrap().insert(id.clone(), Arc::new(Session { client_tx }));
    eprintln!("sse session {id} opened");

    // spec の例どおり relative path（クライアントは base URL に解決する）。
    // handshake はこの応答の後に POST /messages で行われるので、先に endpoint を伝える。
    if frame_tx
        .try_send(Bytes::from(format!("event: endpoint\ndata: /messages?sessionId={id}\n\n")))
        .is_err()
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "SSE stream closed");
    }

    // この task が running を保持しているあいだ service loop が生きている。
    // クライアントが切れて frame_tx が閉じると running が drop され、service も終わる。
    tokio::spawn(async move {
        let mut out_rx = out_rx;
        let service = match MisskeyService::new(config) {
            Ok(service) => service,
            Err(e) => {
                eprintln!("sse service error: {e}");
                return;
            }
        };
        let running = match service
            .serve(ChannelTransport {
                rx: client_rx,
                tx: out_tx,
            })
            .await
        {
            Ok(running) => running,
            Err(e) => {
                eprintln!("sse service error: {e}");
                return;
            }
        };
        while let Some(msg) = out_rx.recv().await {
            let data = match serde_json::to_string(&msg) {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("sse encode error: {e}");
                    continue;
                }
            };
            if frame_tx
                .send(Bytes::from(format!("event: message\ndata: {data}\n\n")))
                .await
                .is_err()
            {
                break;
            }
        }
        drop(running);
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .body(BoxBody::new(SseBody {
            rx: frame_rx,
            registry,
            session_id: id,
        }))
        .unwrap()
}

async fn post_message(req: Request<Incoming>, registry: &Registry) -> Response<ResBody> {
    let id = query_value(req.uri().query(), "sessionId");
    let Some(id) = id else {
        return error_response(StatusCode::BAD_REQUEST, "missing sessionId");
    };
    let Some(session) = registry.lock().unwrap().get(id).cloned() else {
        return error_response(StatusCode::NOT_FOUND, "no such session");
    };

    let bytes = match BodyExt::collect(req.into_body()).await {
        Ok(bytes) => bytes.to_bytes(),
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    if bytes.len() > MAX_BODY_BYTES {
        return error_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
    }

    let msg = match serde_json::from_slice::<ClientJsonRpcMessage>(&bytes) {
        Ok(msg) => msg,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    // レガシー SSE では POST の応答は 202 で、JSON-RPC の応答は SSE 側に出る
    if session.client_tx.send(msg).await.is_err() {
        return error_response(StatusCode::GONE, "session closed");
    }
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(BoxBody::new(Full::new(Bytes::new())))
        .unwrap()
}

/// SSE 接続が切れた（body が drop された）ときにセッションを掃除する。
/// client_tx が閉じると service loop の receive が None を返して終わる。
struct SseBody {
    rx: mpsc::Receiver<Bytes>,
    registry: Registry,
    session_id: String,
}

impl HttpBody for SseBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        match this.rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => Poll::Ready(Some(Ok(Frame::data(data)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.rx.is_closed()
    }
}

impl Drop for SseBody {
    fn drop(&mut self) {
        if self.registry.lock().unwrap().remove(&self.session_id).is_some() {
            eprintln!("sse session {} closed", self.session_id);
        }
    }
}

fn query_value<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    query?
        .split('&')
        .map(|pair| pair.split_once('='))
        .flatten()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn error_response(status: StatusCode, message: &str) -> Response<ResBody> {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(BoxBody::new(Full::new(Bytes::from(body))))
        .unwrap()
}
