// 在线工具模块：通过 WebSocket 连接服务端调试桥（axs-debug），获取运行时数据

pub mod bridge_client;

use crate::tool::{Tool, ToolContext, ToolResult, box_future};
use serde_json::{json, Value};

pub use bridge_client::BridgeClient;

/// 注册所有在线工具
pub fn register_all(registry: &mut crate::tool::ToolRegistry) {
    // 8. ax_connect_bridge
    registry.register(Tool {
        name: "ax_connect_bridge",
        description: "连接服务端调试桥（axs-debug WebSocket），返回连接状态",
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "调试桥 WebSocket 地址（可选，默认用配置）" },
                "token": { "type": "string", "description": "认证 token（可选，默认用配置）" }
            }
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { connect_bridge(args, ctx).await })
        }),
    });

    // 9. ax_get_ui_config
    registry.register(Tool {
        name: "ax_get_ui_config",
        description: "从服务端获取指定 UI 的 yml 配置文件内容",
        input_schema: json!({
            "type": "object",
            "properties": {
                "ui_id": { "type": "string", "description": "UI ID（文件名，不含扩展名）" }
            },
            "required": ["ui_id"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge(args, ctx, "ui.read", "ui_id").await })
        }),
    });

    // 10. ax_reload_module
    registry.register(Tool {
        name: "ax_reload_module",
        description: "重载服务端指定模块",
        input_schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string", "description": "模块 ID" }
            },
            "required": ["module_id"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge(args, ctx, "module.reload", "module_id").await })
        }),
    });

    // 11. ax_list_modules
    registry.register(Tool {
        name: "ax_list_modules",
        description: "列出服务端已加载模块（id + name + version + ready）",
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { list_modules(args, ctx).await })
        }),
    });

    // 12. ax_run_server_command
    registry.register(Tool {
        name: "ax_run_server_command",
        description: "在服务端执行命令（以控制台身份），返回命令输出",
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "要执行的命令（不含 / 前缀）" }
            },
            "required": ["command"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { run_server_command(args, ctx).await })
        }),
    });

    // 13. ax_list_ui_files_online
    registry.register(Tool {
        name: "ax_list_ui_files_online",
        description: "从服务端列出 ui/ 目录下所有 UI yml 文件名",
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge_no_params(args, ctx, "ui.list").await })
        }),
    });

    // 14. ax_is_ui_open
    registry.register(Tool {
        name: "ax_is_ui_open",
        description: "查询指定玩家的某个 UI 是否处于打开状态",
        input_schema: json!({
            "type": "object",
            "properties": {
                "player": { "type": "string", "description": "玩家名" },
                "ui_id": { "type": "string", "description": "UI ID" }
            },
            "required": ["player", "ui_id"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_player_query(args, ctx, "ui.is_open").await })
        }),
    });

    // 15. ax_diagnose_config
    registry.register(Tool {
        name: "ax_diagnose_config",
        description: "对指定模块运行配置诊断，返回模块声明的 ModuleConfigSpec 列表（同步策略、版本、校验规则等）",
        input_schema: json!({
            "type": "object",
            "properties": {
                "module_id": { "type": "string", "description": "模块 ID" }
            },
            "required": ["module_id"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge(args, ctx, "config.diagnose", "module_id").await })
        }),
    });

    // 16. ax_list_players
    registry.register(Tool {
        name: "ax_list_players",
        description: "列出服务端在线玩家（名称、UUID、所在世界）",
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge_no_params(args, ctx, "player.list").await })
        }),
    });

    // 17. ax_tail_log
    registry.register(Tool {
        name: "ax_tail_log",
        description: "获取服务端最近 N 行日志（读取 logs/latest.log）",
        input_schema: json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "description": "获取的行数（默认 100）" }
            }
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { tail_log(args, ctx).await })
        }),
    });

    // 18. ax_get_captured_packets
    registry.register(Tool {
        name: "ax_get_captured_packets",
        description: "查询服务端捕获的客户端自定义包（按时间倒序，可限制数量）",
        input_schema: json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "最多返回的包数量（默认 100）" }
            }
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { get_captured_packets(args, ctx).await })
        }),
    });

    // 19. ax_clear_captured_packets
    registry.register(Tool {
        name: "ax_clear_captured_packets",
        description: "清空服务端包流捕获缓冲区",
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge_no_params(args, ctx, "packet.clear").await })
        }),
    });

    // 20. ax_packet_capture_status
    registry.register(Tool {
        name: "ax_packet_capture_status",
        description: "查询服务端包流捕获状态（当前缓冲区中的记录数）",
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { call_bridge_no_params(args, ctx, "packet.status").await })
        }),
    });
}

/// 工具 8：ax_connect_bridge
async fn connect_bridge(args: Value, ctx: ToolContext) -> ToolResult {
    let cfg = ctx.bridge.default_config();
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or(cfg.url.as_str())
        .to_string();
    let token = args
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or(cfg.token.as_str())
        .to_string();

    match ctx.bridge.connect(&url, &token).await {
        Ok(_) => {
            let connected = ctx.bridge.is_connected().await;
            ToolResult::json(json!({
                "success": true,
                "url": url,
                "connected": connected,
                "message": "调试桥连接成功"
            }))
        }
        Err(e) => ToolResult::json(json!({
            "success": false,
            "url": url,
            "connected": false,
            "error": format!("{}", e),
            "message": "调试桥连接失败"
        })),
    }
}

/// 通用桥接调用：从参数取 ui_id/module_id 字段，转发到指定 method
async fn call_bridge(args: Value, ctx: ToolContext, method: &str, id_field: &str) -> ToolResult {
    let id = match args.get(id_field).and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error(format!("缺少参数 {}", id_field)),
    };
    // 根据 id_field 决定发给调试桥的 JSON key 名：
    //   ui_id    -> uiId    (ui.read)
    //   module_id -> moduleId (module.reload / config.diagnose)
    let param_key = match id_field {
        "module_id" => "moduleId",
        _ => "uiId",
    };
    let params = json!({ param_key: id });
    call_bridge_method(&ctx, method, params).await
}

/// 玩家级 UI 只读查询（ax_is_ui_open -> ui.is_open）
async fn ui_player_query(args: Value, ctx: ToolContext, method: &str) -> ToolResult {
    let player = match args.get("player").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 player".to_string()),
    };
    let ui_id = match args.get("ui_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 ui_id".to_string()),
    };
    let params = json!({ "player": player, "uiId": ui_id });
    call_bridge_method(&ctx, method, params).await
}

/// ax_list_modules -> module.list
async fn list_modules(_args: Value, ctx: ToolContext) -> ToolResult {
    call_bridge_method(&ctx, "module.list", json!({})).await
}

/// ax_run_server_command -> server.command
async fn run_server_command(args: Value, ctx: ToolContext) -> ToolResult {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 command".to_string()),
    };
    let params = json!({ "command": command });
    call_bridge_method(&ctx, "server.command", params).await
}

/// ax_tail_log -> log.tail
async fn tail_log(args: Value, ctx: ToolContext) -> ToolResult {
    let lines = args.get("lines").and_then(|v| v.as_i64());
    let params = match lines {
        Some(n) => json!({ "lines": n }),
        None => json!({}),
    };
    call_bridge_method(&ctx, "log.tail", params).await
}

/// ax_get_captured_packets -> packet.get_captured
async fn get_captured_packets(args: Value, ctx: ToolContext) -> ToolResult {
    let limit = args.get("limit").and_then(|v| v.as_i64());
    let params = match limit {
        Some(n) => json!({ "limit": n }),
        None => json!({}),
    };
    call_bridge_method(&ctx, "packet.get_captured", params).await
}

/// 无参数的桥接方法调用（ui.list、player.list、packet.clear、packet.status）
async fn call_bridge_no_params(_args: Value, ctx: ToolContext, method: &str) -> ToolResult {
    call_bridge_method(&ctx, method, json!({})).await
}

/// 统一的桥接 method 调用封装
async fn call_bridge_method(ctx: &ToolContext, method: &str, params: Value) -> ToolResult {
    if !ctx.bridge.is_connected().await {
        return ToolResult::error(
            "未连接调试桥，请先调用 ax_connect_bridge 连接服务端".to_string(),
        );
    }
    match ctx.bridge.request(method, params).await {
        Ok(resp) => ToolResult::json(resp),
        Err(e) => ToolResult::error(format!("调试桥请求失败 ({}): {}", method, e)),
    }
}
