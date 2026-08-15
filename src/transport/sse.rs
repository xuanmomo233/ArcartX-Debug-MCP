// HTTP/SSE 传输：GET /sse 建立 SSE 连接，POST /messages 发送 JSON-RPC 请求

use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_stream::Stream;

use super::RequestHandler;

/// SSE 会话：每个 /sse 连接对应一个会话，持有发送端用于推送消息
#[derive(Clone)]
struct Session {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

/// 共享状态：会话表 + 请求处理器
#[derive(Clone)]
struct AppState {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    handler: RequestHandler,
}

/// 运行 SSE 传输
pub async fn run(listen: &str, handler: RequestHandler) -> Result<()> {
    log::info!("SSE 传输已启动，监听 {}", listen);

    let state = AppState {
        sessions: Arc::new(Mutex::new(HashMap::new())),
        handler,
    };

    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// GET /sse：建立 SSE 长连接，返回 endpoint 事件告知客户端消息提交地址
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 注册会话
    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(
            session_id.clone(),
            Session {
                tx: tx.clone(),
            },
        );
    }
    log::info!("新 SSE 会话: {}", session_id);

    // endpoint 事件：告知客户端 POST 地址（带 session_id 查询参数）
    let endpoint_url = format!("/messages?session_id={}", session_id);
    let init_event = Event::default()
        .event("endpoint")
        .data(endpoint_url);

    // 构造流：先发 endpoint，再持续转发通道消息
    let stream = async_stream::stream! {
        yield Ok(init_event);
        while let Some(msg) = rx.recv().await {
            yield Ok(Event::default().data(msg));
        }
        // 客户端断开时清理会话
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&session_id);
        log::info!("SSE 会话已断开: {}", session_id);
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// POST /messages?session_id=xxx：接收 JSON-RPC 请求，处理后通过对应会话的 SSE 推送响应
async fn messages_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let session_id = match params.get("session_id") {
        Some(id) => id.clone(),
        None => return (StatusCode::BAD_REQUEST, "缺少 session_id").into_response(),
    };

    let session = {
        let sessions = state.sessions.lock().await;
        sessions.get(&session_id).cloned()
    };

    let session = match session {
        Some(s) => s,
        None => return (StatusCode::NOT_FOUND, "会话不存在").into_response(),
    };

    // 异步处理请求，把响应推送到 SSE 通道
    let handler = state.handler.clone();
    tokio::spawn(async move {
        let resp = handler(request).await;
        // 通知（无 id）不返回
        if resp.get("id").is_some() {
            if let Ok(line) = serde_json::to_string(&resp) {
                let _ = session.tx.send(line);
            }
        }
    });

    (StatusCode::ACCEPTED, "已接收").into_response()
}
