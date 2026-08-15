// UI YAML 解析模块：解析 ArcartX UI yml，返回结构化控件树；校验结构；查找控件引用；列出/搜索 UI 文件

use crate::tool::{ToolContext, ToolResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use yaml_rust2::{Yaml, YamlLoader};

/// 已知的合法控件类型集合（来自 ArcartX UI 知识文档）
const KNOWN_CONTROL_TYPES: &[&str] = &[
    "Texture", "Text", "9SliceTexture", "Entity", "Model", "Slot", "BossBar", "Compass",
    "TextBox", "Progress", "Import", "Observer", "ChatTextBox", "Suggestion", "Chat", "Card",
    "Adaptive", "Canvas", "HStack", "VStack", "Stack", "HGrid", "Grid", "VGrid", "Scroll",
    "BossBars", "Tip",
];

/// 已知的 UI 触发器名
const KNOWN_UI_TRIGGERS: &[&str] = &[
    "keyPress", "keyRelease", "wheel", "message", "chatOpen", "chatClose", "open", "click",
    "clickLeft", "clickRight", "clickMiddle", "release", "releaseLeft", "releaseRight",
    "releaseMiddle", "resize", "close", "tick", "seconds", "load",
];

/// 已知的控件触发器名
const KNOWN_CONTROL_TRIGGERS: &[&str] = &[
    "click", "clickLeft", "clickRight", "clickMiddle", "release", "releaseLeft", "releaseRight",
    "releaseMiddle", "enter", "leave", "wheel", "keyPress", "keyRelease", "create", "remove",
    "textChange",
];

/// 从 Yaml 中安全地按键取值（先判断是否为 Hash）
fn yaml_get<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    if let Yaml::Hash(h) = yaml {
        h.get(&Yaml::String(key.to_string()))
    } else {
        None
    }
}

/// 解析 file_path 参数为绝对路径（支持相对 workspace）
fn resolve_path(file_path: &str, ctx: &ToolContext) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.config.workspace_path().join(p)
    }
}

/// 读取并解析 YAML 文件
fn load_yaml(file_path: &Path) -> Result<Vec<Yaml>, String> {
    let content = std::fs::read_to_string(file_path)
        .map_err(|e| format!("读取文件失败 {}: {}", file_path.display(), e))?;
    YamlLoader::load_from_str(&content).map_err(|e| format!("解析 YAML 失败: {}", e))
}

/// 把 Yaml 值转成 JSON（用于结构化输出）
fn yaml_to_json(y: &Yaml) -> Value {
    match y {
        Yaml::Real(s) => {
            // 尝试解析为数字
            s.parse::<f64>()
                .map(|n| {
                    if n.fract() == 0.0 && n.abs() < 1e15 {
                        json!(n as i64)
                    } else {
                        json!(n)
                    }
                })
                .unwrap_or_else(|_| json!(s.as_str()))
        }
        Yaml::Integer(i) => json!(i),
        Yaml::String(s) => json!(s),
        Yaml::Boolean(b) => json!(b),
        Yaml::Array(arr) => {
            json!(arr.iter().map(yaml_to_json).collect::<Vec<_>>())
        }
        Yaml::Hash(h) => {
            let mut map = serde_json::Map::new();
            for (k, v) in h.iter() {
                let key = match k {
                    Yaml::String(s) => s.clone(),
                    Yaml::Integer(i) => i.to_string(),
                    _ => continue,
                };
                map.insert(key, yaml_to_json(v));
            }
            Value::Object(map)
        }
        Yaml::Null | Yaml::BadValue => Value::Null,
        _ => Value::Null,
    }
}

/// 递归解析控件节点，返回控件 JSON
fn parse_control(name: &str, node: &Yaml) -> Value {
    let mut ctrl = serde_json::Map::new();
    ctrl.insert("name".to_string(), json!(name));

    if let Yaml::Hash(h) = node {
        // type
        if let Some(t) = h.get(&Yaml::String("type".to_string())) {
            ctrl.insert("type".to_string(), yaml_to_json(t));
        }
        // val
        if let Some(v) = h.get(&Yaml::String("val".to_string())) {
            ctrl.insert("val".to_string(), yaml_to_json(v));
        }
        // attribute
        if let Some(attr) = h.get(&Yaml::String("attribute".to_string())) {
            ctrl.insert("attribute".to_string(), yaml_to_json(attr));
        }
        // effect
        if let Some(effect) = h.get(&Yaml::String("effect".to_string())) {
            ctrl.insert("effect".to_string(), yaml_to_json(effect));
        }
        // action
        if let Some(action) = h.get(&Yaml::String("action".to_string())) {
            ctrl.insert("action".to_string(), yaml_to_json(action));
        }
        // children
        if let Some(children) = h.get(&Yaml::String("children".to_string())) {
            let child_list = parse_children(children);
            ctrl.insert("children".to_string(), json!(child_list));
        }
    }
    Value::Object(ctrl)
}

/// 解析 children 块
fn parse_children(node: &Yaml) -> Vec<Value> {
    match node {
        Yaml::Hash(h) => {
            h.iter()
                .map(|(k, v)| {
                    let name = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        _ => "?".to_string(),
                    };
                    parse_control(&name, v)
                })
                .collect()
        }
        Yaml::Array(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| parse_control(&i.to_string(), v))
            .collect(),
        _ => vec![],
    }
}

/// 解析 controls / template 块
fn parse_control_block(node: &Yaml) -> Vec<Value> {
    parse_children(node)
}

/// 工具 1：ax_parse_ui_yaml
pub async fn parse_ui_yaml(args: Value, ctx: ToolContext) -> ToolResult {
    let file_path = match args.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return ToolResult::error("缺少参数 file_path".to_string()),
    };

    let path = resolve_path(&file_path, &ctx);
    let docs = match load_yaml(&path) {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    if docs.is_empty() {
        return ToolResult::error("YAML 文件为空".to_string());
    }

    let root = &docs[0];
    let mut result = serde_json::Map::new();
    result.insert("file_path".to_string(), json!(path.display().to_string()));

    // ui 块
    if let Some(ui) = yaml_get(root, "ui") {
        result.insert("ui".to_string(), yaml_to_json(ui));
    }
    // controls 块
    if let Some(controls) = yaml_get(root, "controls") {
        result.insert("controls".to_string(), json!(parse_control_block(controls)));
    }
    // template 块
    if let Some(template) = yaml_get(root, "template") {
        result.insert("template".to_string(), json!(parse_control_block(template)));
    }
    // tasks 块（UI 定时任务）
    if let Some(tasks) = yaml_get(root, "tasks") {
        result.insert("tasks".to_string(), yaml_to_json(tasks));
    }

    ToolResult::json(Value::Object(result))
}

/// 工具 3：ax_validate_ui_structure
pub async fn validate_ui_structure(args: Value, ctx: ToolContext) -> ToolResult {
    let file_path = match args.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return ToolResult::error("缺少参数 file_path".to_string()),
    };

    let path = resolve_path(&file_path, &ctx);
    let docs = match load_yaml(&path) {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };
    if docs.is_empty() {
        return ToolResult::error("YAML 文件为空".to_string());
    }
    let root = &docs[0];

    let mut issues: Vec<Value> = vec![];

    // 检查 ui 块存在
    let ui_node = yaml_get(root, "ui");
    if ui_node.is_none() {
        issues.push(json!({
            "level": "error",
            "message": "缺少 ui 块",
            "location": "root"
        }));
    } else if let Some(Yaml::Hash(ui_h)) = ui_node {
        // 检查 ui 触发器名
        if let Some(Yaml::Hash(action_h)) = ui_h.get(&Yaml::String("action".to_string())) {
            for (k, _) in action_h.iter() {
                if let Yaml::String(name) = k {
                    if !KNOWN_UI_TRIGGERS.contains(&name.as_str()) {
                        issues.push(json!({
                            "level": "warning",
                            "message": format!("ui.action 中存在未知触发器名: {}", name),
                            "location": "ui.action"
                        }));
                    }
                }
            }
        }
    }

    // 检查 controls 块
    if let Some(Yaml::Hash(controls_h)) = yaml_get(root, "controls") {
        for (k, v) in controls_h.iter() {
            let name = yaml_key_to_string(k);
            validate_control(&name, v, "controls", &mut issues);
        }
    }

    // 检查 template 块
    if let Some(Yaml::Hash(template_h)) = yaml_get(root, "template") {
        for (k, v) in template_h.iter() {
            let name = yaml_key_to_string(k);
            validate_control(&name, v, "template", &mut issues);
        }
    }

    let summary = json!({
        "file_path": path.display().to_string(),
        "issue_count": issues.len(),
        "issues": issues,
    });
    ToolResult::json(summary)
}

/// 递归校验单个控件
fn validate_control(name: &str, node: &Yaml, block: &str, issues: &mut Vec<Value>) {
    let path = format!("{}.{}", block, name);

    if let Yaml::Hash(h) = node {
        // 检查 type 存在
        let type_val = h.get(&Yaml::String("type".to_string()));
        if type_val.is_none() {
            issues.push(json!({
                "level": "error",
                "message": format!("控件 {} 缺少 type 属性", path),
                "location": path
            }));
        } else if let Some(Yaml::String(t)) = type_val {
            // 检查 type 是否已知（大小写不敏感比对）
            let known = KNOWN_CONTROL_TYPES
                .iter()
                .any(|k| k.eq_ignore_ascii_case(t));
            if !known {
                issues.push(json!({
                    "level": "warning",
                    "message": format!("控件 {} 使用未知控件类型: {}", path, t),
                    "location": path
                }));
            }
            // 大小写规范检查：建议首字母大写
            if t.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
                issues.push(json!({
                    "level": "warning",
                    "message": format!("控件 {} 的 type '{}' 建议首字母大写（如 {}）", path, t, capitalize(t)),
                    "location": path
                }));
            }
        }

        // 检查 action 触发器名
        if let Some(Yaml::Hash(action_h)) = h.get(&Yaml::String("action".to_string())) {
            for (k, _) in action_h.iter() {
                if let Yaml::String(tname) = k {
                    if !KNOWN_CONTROL_TRIGGERS.contains(&tname.as_str()) {
                        issues.push(json!({
                            "level": "warning",
                            "message": format!("控件 {} 的 action 中存在未知触发器名: {}", path, tname),
                            "location": format!("{}.action", path)
                        }));
                    }
                }
            }
        }

        // 检查 attribute 中 width/height 是否存在（基础控件建议有）
        if let Some(Yaml::Hash(attr_h)) = h.get(&Yaml::String("attribute".to_string())) {
            if !attr_h.contains_key(&Yaml::String("width".to_string()))
                && !attr_h.contains_key(&Yaml::String("point".to_string()))
            {
                // stretch_all 等锚点控件可省略 width，仅提示
                issues.push(json!({
                    "level": "info",
                    "message": format!("控件 {} 的 attribute 缺少 width，可能影响布局", path),
                    "location": format!("{}.attribute", path)
                }));
            }
        }

        // 递归检查 children
        if let Some(Yaml::Hash(children_h)) = h.get(&Yaml::String("children".to_string())) {
            for (ck, cv) in children_h.iter() {
                let cname = yaml_key_to_string(ck);
                validate_control(&cname, cv, &format!("{}.children", path), issues);
            }
        }
    }
}

/// 工具 4：ax_find_control_refs
pub async fn find_control_refs(args: Value, ctx: ToolContext) -> ToolResult {
    let ui_id = match args.get("ui_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 ui_id".to_string()),
    };
    let control_name = match args.get("control_name").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 control_name".to_string()),
    };

    // 在 ui 目录下查找 ui_id.yml
    let ui_dir = ctx.config.ui_dir();
    let file_path = ui_dir.join(format!("{}.yml", ui_id));
    if !file_path.exists() {
        return ToolResult::error(format!("UI 文件不存在: {}", file_path.display()));
    }

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => return ToolResult::error(format!("读取文件失败: {}", e)),
    };

    let mut refs: Vec<Value> = vec![];
    // 搜索 self['control_name']、self["control_name"]、val.control_name 等引用模式
    let patterns = [
        format!("self['{}']", control_name),
        format!("self[\"{}\"]", control_name),
        format!("val.{}", control_name),
    ];

    for (line_no, line) in content.lines().enumerate() {
        for pat in &patterns {
            if line.contains(pat.as_str()) {
                refs.push(json!({
                    "file": file_path.display().to_string(),
                    "line": line_no + 1,
                    "pattern": pat,
                    "context": line.trim()
                }));
            }
        }
        // 也匹配控件定义处（作为 control_name: 出现的 key 定义）
        let def_pattern = format!("{}:", control_name);
        if line.trim_start().starts_with(def_pattern.as_str()) {
            refs.push(json!({
                "file": file_path.display().to_string(),
                "line": line_no + 1,
                "pattern": "definition",
                "context": line.trim()
            }));
        }
    }

    ToolResult::json(json!({
        "ui_id": ui_id,
        "control_name": control_name,
        "ref_count": refs.len(),
        "refs": refs,
    }))
}

/// 工具 6：ax_list_ui_files
pub async fn list_ui_files(args: Value, ctx: ToolContext) -> ToolResult {
    let workspace = args
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| ctx.config.workspace_path());

    // 自动检测 ArcartX-Suite / ArcartXSuite 目录名
    let plugins = workspace.join("plugins");
    let ui_dir = ["ArcartX-Suite", "ArcartXSuite"]
        .iter()
        .map(|name| plugins.join(name).join("ui"))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| plugins.join("ArcartX-Suite").join("ui"));

    let mut files: Vec<String> = vec![];
    if ui_dir.exists() {
        collect_yml_files(&ui_dir, &ui_dir, &mut files);
    } else {
        return ToolResult::error(format!("UI 目录不存在: {}", ui_dir.display()));
    }

    ToolResult::json(json!({
        "workspace": workspace.display().to_string(),
        "ui_dir": ui_dir.display().to_string(),
        "count": files.len(),
        "files": files,
    }))
}

/// 递归收集 yml 文件，返回相对 ui_dir 的路径
fn collect_yml_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_yml_files(&path, base, out);
            } else if path.extension().map(|e| e == "yml" || e == "yaml").unwrap_or(false) {
                if let Ok(rel) = path.strip_prefix(base) {
                    let id = rel.with_extension("").to_string_lossy().replace('\\', "/");
                    out.push(id);
                }
            }
        }
    }
}

/// 工具 7：ax_search_in_files
pub async fn search_in_files(args: Value, ctx: ToolContext) -> ToolResult {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return ToolResult::error("缺少参数 pattern".to_string()),
    };
    let workspace = args
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| PathBuf::from(s))
        .unwrap_or_else(|| ctx.config.workspace_path());
    let glob = args.get("glob").and_then(|v| v.as_str()).map(|s| s.to_string());

    let plugins = workspace.join("plugins");
    let ui_dir = ["ArcartX-Suite", "ArcartXSuite"]
        .iter()
        .map(|name| plugins.join(name).join("ui"))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| plugins.join("ArcartX-Suite").join("ui"));

    let mut matches: Vec<Value> = vec![];
    if ui_dir.exists() {
        search_in_dir(&ui_dir, &ui_dir, &pattern, glob.as_deref(), &mut matches);
    }

    ToolResult::json(json!({
        "pattern": pattern,
        "match_count": matches.len(),
        "matches": matches,
    }))
}

/// 递归搜索目录
fn search_in_dir(
    dir: &Path,
    base: &Path,
    pattern: &str,
    glob: Option<&str>,
    out: &mut Vec<Value>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                search_in_dir(&path, base, pattern, glob, out);
            } else if path.extension().map(|e| e == "yml" || e == "yaml").unwrap_or(false) {
                // glob 过滤文件名
                if let Some(g) = glob {
                    let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if !match_glob(g, &fname) {
                        continue;
                    }
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let rel = path.strip_prefix(base).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                    for (line_no, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            out.push(json!({
                                "file": rel,
                                "line": line_no + 1,
                                "context": line.trim()
                            }));
                        }
                    }
                }
            }
        }
    }
}

/// 简单 glob 匹配（支持 * 通配符）
fn match_glob(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern.is_empty() {
        return true;
    }
    // 简单实现：把 * 转成正则
    let regex_str = regex::escape(pattern).replace("\\*", ".*");
    match regex::Regex::new(&format!("^{}$", regex_str)) {
        Ok(re) => re.is_match(name),
        Err(_) => name.contains(pattern),
    }
}

/// Yaml key 转字符串
fn yaml_key_to_string(k: &Yaml) -> String {
    match k {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        _ => "?".to_string(),
    }
}

/// 首字母大写
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
