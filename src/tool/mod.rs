// 工具注册和分发模块：定义 Tool trait、工具元数据、MCP JSON-RPC 分发

pub mod offline;
pub mod online;

use crate::config::AppConfig;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// 工具执行上下文：持有配置、调试桥客户端等共享状态
#[derive(Clone)]
pub struct ToolContext {
    pub config: AppConfig,
    pub bridge: online::BridgeClient,
}

/// 工具定义：name + description + inputSchema + 执行函数
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema 描述输入参数
    pub input_schema: Value,
    /// 执行函数：接收参数和上下文，返回结果文本（MCP content 数组）
    pub handler: ToolHandler,
}

/// 工具执行函数类型
pub type ToolHandler =
    Arc<dyn Fn(Value, ToolContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>> + Send + Sync>;

/// 把一个 Future 装箱为 Pin<Box<dyn Future<Output = ToolResult> + Send>>
/// 用于工具 handler 闭包的返回值，避免每处手写 as 转换
pub fn box_future<F>(f: F) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolResult> + Send>>
where
    F: std::future::Future<Output = ToolResult> + Send + 'static,
{
    Box::pin(f)
}

/// 工具执行结果：MCP tools/call 返回的 content
pub struct ToolResult {
    /// 内容数组（text 类型）
    pub content: Vec<ContentItem>,
    /// 是否出错（MCP isError 标志）
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(text: String) -> Self {
        Self {
            content: vec![ContentItem::text(text)],
            is_error: false,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            content: vec![ContentItem::text(message)],
            is_error: true,
        }
    }

    pub fn json(value: Value) -> Self {
        Self {
            content: vec![ContentItem::text(serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()))],
            is_error: false,
        }
    }
}

/// MCP content 项
pub struct ContentItem {
    pub item_type: String, // "text"
    pub text: String,
}

impl ContentItem {
    pub fn text(text: String) -> Self {
        Self {
            item_type: "text".to_string(),
            text,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({ "type": self.item_type, "text": self.text })
    }
}

/// 工具注册表
pub struct ToolRegistry {
    tools: HashMap<String, Arc<Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// 注册一个工具
    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name.to_string(), Arc::new(tool));
    }

    /// 列出所有工具的元数据（用于 tools/list 响应）
    pub fn list(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect()
    }

    /// 调用工具
    pub async fn call(&self, name: &str, arguments: Value, ctx: ToolContext) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => {
                log::info!("调用工具: {} 参数: {}", name, arguments);
                let result = (tool.handler)(arguments, ctx).await;
                result
            }
            None => ToolResult::error(format!("未知工具: {}", name)),
        }
    }

    /// 获取工具名列表
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

/// 把 ToolResult 转成 MCP tools/call 响应的 result 部分
pub fn tool_result_to_json(result: ToolResult) -> Value {
    let content: Vec<Value> = result.content.iter().map(|c| c.to_json()).collect();
    let mut map = Map::new();
    map.insert("content".to_string(), Value::Array(content));
    map.insert("isError".to_string(), Value::Bool(result.is_error));
    Value::Object(map)
}

/// 构建所有工具并注册到 registry
pub fn build_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    offline::register_all(&mut registry);
    online::register_all(&mut registry);
    registry
}

/// 从 JSON-RPC params 中提取 tools/call 的 name 和 arguments
pub fn extract_call_params(params: &Value) -> Option<(String, Value)> {
    let name = params.get("name")?.as_str()?.to_string();
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    Some((name, arguments))
}
