// stdio 传输：每行一个 JSON-RPC 消息，从 stdin 读、向 stdout 写

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{BufRead, Write};
use tokio::sync::mpsc;

use super::RequestHandler;

/// 运行 stdio 传输循环
pub async fn run(handler: RequestHandler) -> Result<()> {
    log::info!("stdio 传输已启动");

    // 用通道把 stdin 行交给异步处理，结果再写回 stdout
    let (tx, mut rx) = mpsc::channel::<String>(64);

    // 读 stdin 线程：阻塞读行，推入通道
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let lock = stdin.lock();
        for line in lock.lines() {
            match line {
                Ok(l) => {
                    if l.trim().is_empty() {
                        continue;
                    }
                    if tx.blocking_send(l).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    log::error!("读取 stdin 失败: {}", e);
                    break;
                }
            }
        }
    });

    // 异步消费：解析 JSON、调用 handler、写回 stdout
    while let Some(line) = rx.recv().await {
        let parsed: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // 解析失败：返回 JSON-RPC parse error
                let resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                write_response(&resp)?;
                continue;
            }
        };

        // 批量请求处理
        if parsed.is_array() {
            let mut results = Vec::new();
            for req in parsed.as_array().unwrap() {
                let resp = handler(req.clone()).await;
                // 通知（无 id）不返回响应
                if has_id(&resp) {
                    results.push(resp);
                }
            }
            if !results.is_empty() {
                write_response(&Value::Array(results))?;
            }
        } else {
            let resp = handler(parsed).await;
            // 通知（无 id）不返回响应
            if has_id(&resp) {
                write_response(&resp)?;
            }
        }
    }

    Ok(())
}

/// 判断响应是否携带 id（通知无 id，不需要回写）
fn has_id(resp: &Value) -> bool {
    resp.get("id").is_some()
}

/// 把 JSON-RPC 响应写回 stdout（一行一个消息）
fn write_response(value: &Value) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    let line = serde_json::to_string(value).context("序列化响应失败")?;
    stdout
        .write_all(line.as_bytes())
        .context("写入 stdout 失败")?;
    stdout.write_all(b"\n").context("写入 stdout 换行失败")?;
    stdout.flush().context("flush stdout 失败")?;
    Ok(())
}
