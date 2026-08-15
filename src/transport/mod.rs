// 传输层模块：MCP 协议基于 JSON-RPC 2.0，支持 stdio 和 HTTP/SSE 两种传输方式

pub mod sse;
pub mod stdio;

use anyhow::Result;
use serde_json::Value;

/// MCP 请求处理函数类型：接收 JSON-RPC 请求，返回 JSON-RPC 响应
/// 用 BoxFuture 避免引入 async-trait 依赖
pub type RequestHandler =
    std::sync::Arc<dyn Fn(Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>> + Send + Sync>;

/// 启动 stdio 传输
pub async fn run_stdio(handler: RequestHandler) -> Result<()> {
    stdio::run(handler).await
}

/// 启动 SSE 传输
pub async fn run_sse(listen: &str, handler: RequestHandler) -> Result<()> {
    sse::run(listen, handler).await
}
