// ARIA 脚本静态分析模块：提取变量引用、函数调用，检测未定义变量/函数、作用域问题

use crate::tool::{ToolContext, ToolResult};
use serde_json::{json, Value};

/// 已知的 ARIA 内置函数/对象（用于检测未定义函数引用）
const KNOWN_BUILTINS: &[&str] = &[
    "print", "println", "range", "use", "type", "self", "args", "packet", "none", "true", "false",
    "Message", "Screen", "Packet", "Chat", "Thread", "server", "client", "global",
];

/// 已知的 self 方法（控件/UI 函数）
const KNOWN_SELF_METHODS: &[&str] = &[
    "close", "getID", "create", "childrenCount", "get", "clickSlot", "getSlotItemStack",
    "getCarriedItemStack", "delayAction", "getHoverScroll", "slotCount", "removeControlWithMeta",
    "getControlWithMeta", "getOriginalName", "parent", "remove", "clear", "copy", "setDragXRatio",
    "setDragYRatio", "getDragXRatio", "getDragYRatio", "getStackWidth", "getStackHeight",
    "getItemValueSum", "getItemText", "setContent", "getContent", "insert", "setFocus",
    "isHovered", "setItemIcon", "getSameCount", "needShowAll", "getVisibleChildren", "send",
    "replay", "seticon",
];

/// 分析结果
struct AnalysisResult {
    /// 变量声明（var.x / val.x / global.x）
    var_declarations: Vec<String>,
    /// 变量引用（var.x / val.x / global.x 的读取）
    var_references: Vec<String>,
    /// 函数调用（xxx(...) 形式）
    function_calls: Vec<String>,
    /// self 方法调用
    self_method_calls: Vec<String>,
    /// 控件引用 self['xxx'] / self["xxx"]
    control_refs: Vec<String>,
    /// 检测到的问题
    issues: Vec<Value>,
}

impl AnalysisResult {
    fn new() -> Self {
        Self {
            var_declarations: Vec::new(),
            var_references: Vec::new(),
            function_calls: Vec::new(),
            self_method_calls: Vec::new(),
            control_refs: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "var_declarations": self.var_declarations,
            "var_references": self.var_references,
            "function_calls": self.function_calls,
            "self_method_calls": self.self_method_calls,
            "control_refs": self.control_refs,
            "issues": self.issues,
        })
    }
}

/// 工具 2：ax_analyze_aria_script
pub async fn analyze_aria_script(args: Value, _ctx: ToolContext) -> ToolResult {
    let script = match args.get("script").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 script".to_string()),
    };
    let context = args
        .get("context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut result = AnalysisResult::new();
    analyze_script(&script, &mut result);

    // 检测未定义变量：被引用但未声明的 var/val
    let declared: std::collections::HashSet<&String> = result.var_declarations.iter().collect();
    for vref in &result.var_references {
        if !declared.contains(vref) {
            // 排除常见内置（虽 var.x 形式不太会撞内置，但保险）
            result.issues.push(json!({
                "level": "warning",
                "type": "undefined_variable",
                "message": format!("引用了未声明的变量: {}", vref),
                "hint": "确认变量在当前作用域或上层（ui 块）已用 var.x / val.x 声明"
            }));
        }
    }

    // 检测未定义函数：裸标识符函数调用且非已知内置/self 方法
    let self_methods: std::collections::HashSet<&String> = result.self_method_calls.iter().collect();
    for fcall in &result.function_calls {
        if self_methods.contains(fcall) {
            continue;
        }
        if KNOWN_BUILTINS.contains(&fcall.as_str()) {
            continue;
        }
        // 函数名以大写开头视为类构造（如 HashMap()），不报错
        if fcall.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            continue;
        }
        result.issues.push(json!({
            "level": "info",
            "type": "unknown_function",
            "message": format!("调用了非内置函数: {}()", fcall),
            "hint": "确认该函数已通过 var.f = -> {} 定义，或为 Java 互操作类构造"
        }));
    }

    // 检测 self 方法是否已知
    for m in &result.self_method_calls {
        if !KNOWN_SELF_METHODS.contains(&m.as_str()) {
            result.issues.push(json!({
                "level": "info",
                "type": "unknown_self_method",
                "message": format!("self.{}() 不在已知控件/UI 方法列表中", m),
                "hint": "可能是自定义 attribute 函数或动态属性，确认拼写无误"
            }));
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("analysis".to_string(), result.to_json());
    if let Some(ctx) = context {
        out.insert("context".to_string(), json!(ctx));
    }
    out.insert("issue_count".to_string(), json!(result.issues.len()));
    ToolResult::json(Value::Object(out))
}

/// 分析脚本主体
fn analyze_script(script: &str, result: &mut AnalysisResult) {
    // 按行扫描，先做字符串/注释剥离，再做模式匹配
    for (line_no, raw_line) in script.lines().enumerate() {
        let line = strip_comments_and_strings(raw_line);
        let ln = line_no + 1;

        // 变量声明：var.x / val.x / global.x = ...
        for cap in regex_find_all(r"(var|val|global)\.([A-Za-z_\x{4e00}-\x{9fa5}][A-Za-z0-9_\x{4e00}-\x{9fa5}]*)", &line) {
            if cap.len() >= 3 {
                let full = format!("{}.{}", cap[1], cap[2]);
                let p1 = format!("{} =", cap[1]);
                let p2 = format!("{}.{} =", cap[1], cap[2]);
                if line.contains(p1.as_str()) || raw_line.contains(p2.as_str()) {
                    // 声明（出现在赋值左侧）
                    if is_assignment_left(raw_line, &cap[1], &cap[2]) {
                        if !result.var_declarations.contains(&full) {
                            result.var_declarations.push(full.clone());
                        }
                    } else {
                        if !result.var_references.contains(&full) {
                            result.var_references.push(full);
                        }
                    }
                } else {
                    if !result.var_references.contains(&full) {
                        result.var_references.push(full);
                    }
                }
            }
        }

        // self['xxx'] / self["xxx"] 控件引用
        for cap in regex_find_all(r#"self\[['"]([^'"]+)['"]\]"#, &line) {
            if cap.len() >= 2 {
                let r = format!("self['{}']", cap[1]);
                if !result.control_refs.contains(&r) {
                    result.control_refs.push(r);
                }
            }
        }

        // self.xxx() 方法调用
        for cap in regex_find_all(r"self\.([A-Za-z_][A-Za-z0-9_]*)\s*\(", &line) {
            if cap.len() >= 2 {
                let m = cap[1].to_string();
                if !result.self_method_calls.contains(&m) {
                    result.self_method_calls.push(m.clone());
                }
                // 同时记为函数调用
                if !result.function_calls.contains(&m) {
                    result.function_calls.push(m);
                }
            }
        }

        // 裸函数调用 xxx(  （非 self. 前缀，非属性访问 .xxx(）
        // Rust regex 不支持 lookbehind，用捕获前导字符判断
        for cap in regex_find_all(r"(?:^|[^.\w])([A-Za-z_][A-Za-z0-9_]*)\s*\(", &line) {
            if cap.len() >= 2 {
                let f = cap[1].to_string();
                // 跳过关键字
                if matches!(f.as_str(), "if" | "for" | "while" | "return" | "async" | "class" | "extends" | "new" | "super") {
                    continue;
                }
                if !result.function_calls.contains(&f) {
                    result.function_calls.push(f);
                }
            }
        }

        // 作用域问题：检测 if/for 块内声明但外部引用（简化：标记块内声明的变量）
        // 这里仅做轻量提示：检测 for 循环变量
        for cap in regex_find_all(r"for\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s+in\s+", &line) {
            if cap.len() >= 2 {
                result.issues.push(json!({
                    "level": "info",
                    "type": "loop_variable",
                    "line": ln,
                    "message": format!("for 循环变量 {} 为整数需用 .round 做字典 key", cap[1]),
                    "hint": "range() 返回浮点数，字典索引/控件名拼接前用 i.round"
                }));
            }
        }

        // 检测 entryKey 浮点陷阱：entryKey: 0 或 entryKey = 0（无 .round）
        if line.contains("entryKey") {
            // 简化检测：包含 entryKey 且同行有数字赋值但无 round
            if regex_is_match(r"entryKey\s*[:=]\s*0\b", &line) && !line.contains("round") {
                result.issues.push(json!({
                    "level": "warning",
                    "type": "entrykey_float_trap",
                    "line": ln,
                    "message": "entryKey 直接用数字字面量会被解析为浮点，字典查找失败",
                    "hint": "使用 entryKey: 0.round() 显式转为整数"
                }));
            }
        }
    }
}

/// 判断 var.x / val.x 是否出现在赋值左侧（声明/赋值）
fn is_assignment_left(line: &str, prefix: &str, name: &str) -> bool {
    let target = format!("{}.{}", prefix, name);
    if let Some(idx) = line.find(target.as_str()) {
        let after = &line[idx + target.len()..];
        // 跳过空白后是否为 =
        let trimmed = after.trim_start();
        trimmed.starts_with('=') && !trimmed.starts_with("==")
    } else {
        false
    }
}

/// 剥离注释和字符串内容（避免字符串内的内容被误匹配）
fn strip_comments_and_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_single = false; // '...'
    let mut in_double = false; // "..."
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' && !in_single && !in_double {
            // 注释开始，剩余丢弃
            break;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
            out.push(' ');
        } else if c == '"' && !in_single {
            in_double = !in_double;
            out.push(' ');
        } else if (in_single || in_double) && c != '\\' {
            out.push(' '); // 字符串内容替换为空格，保留位置
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// 简单正则 find_all 封装：返回每组的捕获组向量
fn regex_find_all(pattern: &str, text: &str) -> Vec<Vec<String>> {
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let mut out = vec![];
    for cap in re.captures_iter(text) {
        let groups: Vec<String> = cap
            .iter()
            .map(|m| m.map(|s| s.as_str().to_string()).unwrap_or_default())
            .collect();
        out.push(groups);
    }
    out
}

/// 正则 is_match 封装
fn regex_is_match(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}
