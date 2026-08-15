// 反模式检测模块：检测 ArcartX UI 配置中的常见反模式

use crate::tool::{ToolContext, ToolResult};
use serde_json::{json, Value};
use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

/// 从 Yaml 中安全地按键取值（先判断是否为 Hash）
fn yaml_get<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    if let Yaml::Hash(h) = yaml {
        h.get(&Yaml::String(key.to_string()))
    } else {
        None
    }
}

/// 工具 5：ax_check_anti_patterns
pub async fn check_anti_patterns(args: Value, ctx: ToolContext) -> ToolResult {
    let file_path = match args.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return ToolResult::error("缺少参数 file_path".to_string()),
    };

    let path = if Path::new(&file_path).is_absolute() {
        std::path::PathBuf::from(&file_path)
    } else {
        ctx.config.workspace_path().join(&file_path)
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return ToolResult::error(format!("读取文件失败 {}: {}", path.display(), e)),
    };
    let docs = match YamlLoader::load_from_str(&content) {
        Ok(d) => d,
        Err(e) => return ToolResult::error(format!("解析 YAML 失败: {}", e)),
    };
    if docs.is_empty() {
        return ToolResult::error("YAML 文件为空".to_string());
    }
    let root = &docs[0];

    let mut patterns: Vec<Value> = vec![];

    // 收集所有控件名（controls + template + children 递归）
    let mut all_controls: Vec<String> = vec![];
    let mut template_names: Vec<String> = vec![];
    let mut template_used: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(Yaml::Hash(h)) = yaml_get(root, "controls") {
        for (k, v) in h.iter() {
            let name = yaml_key_to_string(k);
            all_controls.push(name.clone());
            collect_children_names(v, &mut all_controls);
        }
    }
    if let Some(Yaml::Hash(h)) = yaml_get(root, "template") {
        for (k, _) in h.iter() {
            template_names.push(yaml_key_to_string(k));
        }
    }

    // 1. 检测 ARIA 脚本中引用不存在的控件
    let script_refs = collect_all_script_refs(root);
    let control_set: std::collections::HashSet<&String> = all_controls.iter().collect();
    for (location, ctrl) in &script_refs {
        if !control_set.contains(ctrl) && !template_names.iter().any(|t| t == ctrl) {
            patterns.push(json!({
                "level": "warning",
                "type": "undefined_control_ref",
                "message": format!("ARIA 脚本引用了不存在的控件: self['{}']", ctrl),
                "location": location,
                "control": ctrl,
                "hint": "确认控件名拼写正确，或在 controls/children/template 中已定义"
            }));
        }
    }

    // 2. packetHandler 中访问控件（init 阶段控件可能未初始化）
    if let Some(Yaml::Hash(ui_h)) = yaml_get(root, "ui") {
        if let Some(Yaml::Hash(ph)) = ui_h.get(&Yaml::String("packetHandler".to_string())) {
            if let Some(Yaml::String(init_script)) = ph.get(&Yaml::String("init".to_string())) {
                // init 中若直接 self['xxx'] 访问控件，提示数据未到达
                let refs = extract_self_refs(init_script);
                for r in refs {
                    patterns.push(json!({
                        "level": "info",
                        "type": "packet_handler_control_access",
                        "message": format!("packetHandler.init 中访问控件 self['{}']，首包可能尚未完成控件初始化", r),
                        "location": "ui.packetHandler.init",
                        "control": r,
                        "hint": "预先发送的包会被缓存但无法访问控件，数据依赖操作应在控件 create 后或 update 中执行"
                    }));
                }
            }
        }
    }

    // 3. 模板控件未使用
    for tpl in &template_names {
        // 检查脚本中是否有 create('tpl', ...) 引用
        let tpl_used_in_script = script_refs.iter().any(|(_, c)| c == tpl)
            || content.contains(format!("create('{}'", tpl).as_str())
            || content.contains(format!("create(\"{}\"", tpl).as_str());
        if tpl_used_in_script {
            template_used.insert(tpl.clone());
        }
    }
    for tpl in &template_names {
        if !template_used.contains(tpl) {
            patterns.push(json!({
                "level": "info",
                "type": "unused_template",
                "message": format!("模板控件 '{}' 已定义但未在脚本中通过 create() 使用", tpl),
                "location": format!("template.{}", tpl),
                "hint": "未使用的模板可删除以减少配置体积，或确认是否遗漏 create 调用"
            }));
        }
    }

    // 4. 动态创建的控件未清理：检测 create/copy 但无对应 remove/clear
    let has_dynamic_create = content.contains("create('") || content.contains("create(\"")
        || content.contains(".copy(") || content.contains("copy('");
    let has_cleanup = content.contains(".remove()") || content.contains(".clear()")
        || content.contains("removeControlWithMeta");
    if has_dynamic_create && !has_cleanup {
        patterns.push(json!({
            "level": "info",
            "type": "missing_cleanup",
            "message": "脚本中存在动态创建控件（create/copy）但未发现 remove/clear 清理逻辑",
            "hint": "UI 关闭或数据刷新时建议清理动态创建的控件，避免重复堆积"
        }));
    }

    // 5. 纯文本属性未用 ~ 前缀
    check_plain_text_prefix(root, &content, &mut patterns);

    // 6. entryKey 浮点陷阱
    if content.contains("entryKey:") {
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("entryKey:") {
                let val_part = trimmed.split(':').nth(1).unwrap_or("").trim();
                // 纯数字且无 round
                if is_pure_number(val_part) && !val_part.contains("round") {
                    patterns.push(json!({
                        "level": "warning",
                        "type": "entrykey_float_trap",
                        "message": format!("entryKey: {} 会被解析为浮点数，字典查找失败", val_part),
                        "line": line_no + 1,
                        "hint": "使用 entryKey: 0.round() 显式声明为整数"
                    }));
                }
            }
        }
    }

    // 7. range() 浮点陷阱：检测 range 用法但未用 .round 做字典 key
    for (line_no, line) in content.lines().enumerate() {
        if line.contains("range(") && line.contains("['") && !line.contains("round") {
            patterns.push(json!({
                "level": "info",
                "type": "range_float_in_dict",
                "line": line_no + 1,
                "message": "range() 返回浮点数，用作字典 key 或控件名拼接需先 .round",
                "hint": "使用 i.round 而非 i 拼接控件名或做字典索引"
            }));
        }
    }

    ToolResult::json(json!({
        "file_path": path.display().to_string(),
        "pattern_count": patterns.len(),
        "patterns": patterns,
    }))
}

/// 递归收集 children 中的控件名
fn collect_children_names(node: &Yaml, out: &mut Vec<String>) {
    if let Some(Yaml::Hash(h)) = yaml_get(node, "children") {
        for (k, v) in h.iter() {
            out.push(yaml_key_to_string(k));
            collect_children_names(v, out);
        }
    }
}

/// 收集所有 ARIA 脚本中的 self['xxx'] 引用，返回 (位置, 控件名)
fn collect_all_script_refs(root: &Yaml) -> Vec<(String, String)> {
    let mut out = vec![];
    let re = regex::Regex::new(r#"self\[['"]([^'"]+)['"]\]"#).unwrap();
    walk_scripts(root, &mut |location, script| {
        for cap in re.captures_iter(script) {
            if let Some(m) = cap.get(1) {
                out.push((location.to_string(), m.as_str().to_string()));
            }
        }
    });
    out
}

/// 从单段脚本中提取 self['xxx'] 引用
fn extract_self_refs(script: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"self\[['"]([^'"]+)['"]\]"#).unwrap();
    re.captures_iter(script)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// 递归遍历所有脚本字符串（action、packetHandler、attribute 中的脚本值）
fn walk_scripts<F: FnMut(&str, &str)>(node: &Yaml, f: &mut F) {
    match node {
        Yaml::Hash(h) => {
            for (k, v) in h.iter() {
                let key = yaml_key_to_string(k);
                match v {
                    Yaml::String(s) => {
                        // action / packetHandler 下的字符串值是脚本
                        if key == "action" || key == "packetHandler" {
                            // 这些是 hash，由下面递归处理
                        }
                        // attribute 下的值也可能是脚本，统一当脚本处理
                        f(&key, s);
                    }
                    _ => {}
                }
                walk_scripts(v, f);
            }
        }
        Yaml::Array(arr) => {
            for item in arr {
                walk_scripts(item, f);
            }
        }
        _ => {}
    }
}

/// 检测纯文本属性未用 ~ 前缀
fn check_plain_text_prefix(root: &Yaml, content: &str, patterns: &mut Vec<Value>) {
    // 检查常见纯文本属性：texts、normal、hover、emptyText、alignment、model、animation
    let plain_props = [
        "texts", "normal", "hover", "emptyText", "alignment", "model", "animation",
        "texture", "background", "filter", "exclude",
    ];

    // 简化：扫描原始内容中这些属性行
    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        for prop in plain_props {
            // 匹配 prop: 后跟非 ~ 且非表达式特征的纯文本
            if let Some(val_part) = extract_yaml_value(trimmed, prop) {
                if should_be_plain_text(&val_part) {
                    patterns.push(json!({
                        "level": "info",
                        "type": "missing_tilde_prefix",
                        "line": line_no + 1,
                        "property": prop,
                        "message": format!("属性 {} 的值 '{}' 疑似纯文本但未用 ~ 前缀", prop, val_part),
                        "hint": "纯文本属性值应加 ~ 前缀（如 ~xxx.png），否则会被当作 ARIA 表达式求值"
                    }));
                }
            }
        }
    }
    let _ = root;
}

/// 提取 YAML 行中某属性的值部分
fn extract_yaml_value(line: &str, prop: &str) -> Option<String> {
    let prefix = format!("{}:", prop);
    if line.starts_with(prefix.as_str()) {
        let val = line[prefix.len()..].trim();
        // 去掉 YAML 引号
        let val = val.trim_matches(|c| c == '"' || c == '\'');
        if val.is_empty() || val == "[]" || val == "{}" {
            return None;
        }
        Some(val.to_string())
    } else {
        None
    }
}

/// 判断值是否疑似应为纯文本（资源路径、颜色代码文本等）
fn should_be_plain_text(val: &str) -> bool {
    if val.starts_with('~') {
        return false; // 已有 ~ 前缀
    }
    if val.starts_with('\'') || val.starts_with('"') {
        return false; // 已是字符串字面量
    }
    // 资源路径特征：xxx.png / xxx.gif / 含 / 的路径
    if val.ends_with(".png") || val.ends_with(".gif") || val.ends_with(".jpg") {
        return true;
    }
    // RGBA 颜色：数字,数字,数字
    if regex::Regex::new(r"^\d+,\d+,\d+").unwrap().is_match(val) {
        return true;
    }
    // 颜色代码文本：&x 或 §x 开头
    if val.starts_with('&') || val.starts_with('§') {
        return true;
    }
    // 纯中文文本
    if val.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' || c == '/' || ('一'..='龥').contains(&c)) {
        // 含中文且非表达式
        if val.chars().any(|c| ('一'..='龥').contains(&c)) && !val.contains('+') && !val.contains('?') && !val.contains("var.") && !val.contains("self") {
            return true;
        }
    }
    false
}

/// 判断是否纯数字字面量
fn is_pure_number(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Yaml key 转字符串
fn yaml_key_to_string(k: &Yaml) -> String {
    match k {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        _ => "?".to_string(),
    }
}
