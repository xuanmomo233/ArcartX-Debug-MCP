// ArcartX-Debug-MCP 入口：解析参数选择 stdio/SSE 传输，实现 MCP JSON-RPC 2.0 协议

mod config;
mod tool;
mod transport;

use anyhow::Result;
use clap::Parser;
use serde_json::{json, Value};
use std::sync::Arc;
use transport::RequestHandler;

/// 命令行参数
#[derive(Parser, Debug)]
#[command(name = "arcartx-debug-mcp", about = "ArcartXSuite 专属 MCP Server")]
struct Cli {
    /// 传输模式：stdio 或 sse（覆盖配置文件）
    #[arg(long)]
    mode: Option<String>,

    /// 配置文件路径
    #[arg(long, default_value = "config.toml")]
    config: String,

    /// SSE 监听地址（覆盖配置文件）
    #[arg(long)]
    listen: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let config = config::AppConfig::load(&cli.config)?;

    // 决定传输模式
    let mode = cli
        .mode
        .clone()
        .unwrap_or_else(|| config.server.mode.clone());

    // 构建工具注册表
    let registry = Arc::new(tool::build_registry());
    log::info!("已注册工具: {:?}", registry.names());

    // 构建工具上下文（含调试桥客户端）
    let bridge = tool::online::BridgeClient::new(config.bridge.clone());
    let ctx = tool::ToolContext {
        config: config.clone(),
        bridge,
    };

    // 自动连接调试桥（若配置开启）
    if config.bridge.auto_connect {
        let b = ctx.bridge.clone();
        let url = config.bridge.url.clone();
        let token = config.bridge.token.clone();
        tokio::spawn(async move {
            if let Err(e) = b.connect(&url, &token).await {
                log::warn!("自动连接调试桥失败: {}", e);
            }
        });
    }

    // 构建请求处理器：实现 MCP JSON-RPC 分发
    let handler = build_handler(registry, ctx);

    // 启动对应传输
    match mode.as_str() {
        "sse" => {
            let listen = cli
                .listen
                .clone()
                .unwrap_or_else(|| config.server.listen.clone());
            log::info!("启动 SSE 传输，监听 {}", listen);
            transport::run_sse(&listen, handler).await?;
        }
        "stdio" | _ => {
            log::info!("启动 stdio 传输");
            transport::run_stdio(handler).await?;
        }
    }

    Ok(())
}

/// 构建 MCP JSON-RPC 请求处理器
fn build_handler(
    registry: Arc<tool::ToolRegistry>,
    ctx: tool::ToolContext,
) -> RequestHandler {
    Arc::new(move |request: Value| {
        let registry = registry.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            dispatch(&registry, &ctx, request).await
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>
    })
}

/// JSON-RPC 分发：处理 initialize / tools/list / tools/call
async fn dispatch(
    registry: &tool::ToolRegistry,
    ctx: &tool::ToolContext,
    request: Value,
) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // 通知（无 id）静默处理
    let is_notification = id == Value::Null;

    let result: Value = match method {
        "initialize" => {
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "arcartx-debug-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })
        }
        "notifications/initialized" => {
            // 客户端初始化完成通知，无需响应
            return json!({});
        }
        "tools/list" => {
            json!({ "tools": registry.list() })
        }
        "tools/call" => {
            if let Some((name, arguments)) = tool::extract_call_params(&params) {
                let result = registry.call(&name, arguments, ctx.clone()).await;
                tool::tool_result_to_json(result)
            } else {
                return error_response(id.clone(), -32602, "Invalid params: missing name", Value::Null);
            }
        }
        "ping" => {
            json!({})
        }
        _ => {
            if is_notification {
                return json!({});
            }
            return error_response(
                id.clone(),
                -32601,
                &format!("Method not found: {}", method),
                Value::Null,
            );
        }
    };

    if is_notification {
        return json!({});
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

/// 构造 JSON-RPC 错误响应
fn error_response(id: Value, code: i64, message: &str, data: Value) -> Value {
    let mut error = serde_json::Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), json!(message));
    if data != Value::Null {
        error.insert("data".to_string(), data);
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": Value::Object(error),
    })
}
