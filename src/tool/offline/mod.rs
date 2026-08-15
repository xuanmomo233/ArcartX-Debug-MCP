// 离线工具模块：直接读 workspace 文件做静态分析

pub mod anti_patterns;
pub mod aria_analyzer;
pub mod ui_yaml;

use crate::tool::{Tool, ToolRegistry, box_future};
use serde_json::json;

/// 注册所有离线工具
pub fn register_all(registry: &mut ToolRegistry) {
    // 1. ax_parse_ui_yaml
    registry.register(Tool {
        name: "ax_parse_ui_yaml",
        description: "解析 ArcartX UI yml 文件，返回结构化控件树（ui 块属性 + controls 列表 + template 列表）",
        input_schema: json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "UI yml 文件路径（绝对路径或相对 workspace 的路径）" }
            },
            "required": ["file_path"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_yaml::parse_ui_yaml(args, ctx).await })
        }),
    });

    // 2. ax_analyze_aria_script
    registry.register(Tool {
        name: "ax_analyze_aria_script",
        description: "静态分析 ARIA 脚本：提取变量引用、函数调用，检测未定义变量/函数、作用域问题",
        input_schema: json!({
            "type": "object",
            "properties": {
                "script": { "type": "string", "description": "ARIA 脚本内容" },
                "context": { "type": "string", "description": "脚本所在上下文（如 ui.action.open、control.attribute），用于辅助分析" }
            },
            "required": ["script"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { aria_analyzer::analyze_aria_script(args, ctx).await })
        }),
    });

    // 3. ax_validate_ui_structure
    registry.register(Tool {
        name: "ax_validate_ui_structure",
        description: "校验 UI 结构合法性：检查必需属性缺失、未知控件类型、未知触发器名、属性类型错误",
        input_schema: json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "UI yml 文件路径" }
            },
            "required": ["file_path"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_yaml::validate_ui_structure(args, ctx).await })
        }),
    });

    // 4. ax_find_control_refs
    registry.register(Tool {
        name: "ax_find_control_refs",
        description: "查找控件在 UI 文件中的引用位置（文件+行号+上下文）",
        input_schema: json!({
            "type": "object",
            "properties": {
                "ui_id": { "type": "string", "description": "UI ID（文件名，不含扩展名）" },
                "control_name": { "type": "string", "description": "控件名称" }
            },
            "required": ["ui_id", "control_name"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_yaml::find_control_refs(args, ctx).await })
        }),
    });

    // 5. ax_check_anti_patterns
    registry.register(Tool {
        name: "ax_check_anti_patterns",
        description: "检测 UI 反模式：ARIA 引用不存在的控件、packetHandler 访问未初始化控件、模板未使用、动态创建控件未清理、纯文本属性未用 ~ 前缀",
        input_schema: json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "UI yml 文件路径" }
            },
            "required": ["file_path"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { anti_patterns::check_anti_patterns(args, ctx).await })
        }),
    });

    // 6. ax_list_ui_files
    registry.register(Tool {
        name: "ax_list_ui_files",
        description: "列出 workspace 中所有 UI yml 文件路径",
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": { "type": "string", "description": "workspace 根路径（可选，默认用配置中的路径）" }
            }
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_yaml::list_ui_files(args, ctx).await })
        }),
    });

    // 7. ax_search_in_files
    registry.register(Tool {
        name: "ax_search_in_files",
        description: "在 UI 文件中搜索文本（支持 glob 过滤），返回匹配位置列表",
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "搜索文本（字面量匹配）" },
                "workspace": { "type": "string", "description": "workspace 根路径（可选）" },
                "glob": { "type": "string", "description": "文件名 glob 过滤（可选，如 *.yml）" }
            },
            "required": ["pattern"]
        }),
        handler: std::sync::Arc::new(|args, ctx| {
            box_future(async move { ui_yaml::search_in_files(args, ctx).await })
        }),
    });
}
