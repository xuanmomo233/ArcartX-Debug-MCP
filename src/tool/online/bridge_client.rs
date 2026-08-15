// WebSocket 调试桥客户端：连接服务端 axs-debug 模块，转发 JSON-RPC 风格请求

use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

/// 调试桥内部状态：发送通道 + 待响应请求表 + 序号
struct BridgeInner {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
    pending: std::collections::HashMap<String, oneshot::Sender<Value>>,
    seq: u64,
}

/// 调试桥客户端：维护 WebSocket 连接，按 id 匹配请求/响应
#[derive(Clone)]
pub struct BridgeClient {
    inner: Arc<Mutex<Option<Arc<Mutex<BridgeInner>>>>>,
    config: crate::config::BridgeConfig,
}

impl BridgeClient {
    /// 创建一个新的客户端（未连接状态）
    pub fn new(config: crate::config::BridgeConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            config,
        }
    }

    /// 连接调试桥
    pub async fn connect(&self, url: &str, token: &str) -> Result<()> {
        // 先断开旧连接
        self.disconnect().await;

        let ws_url = if url.contains('?') {
            format!("{}&token={}", url, token)
        } else {
            format!("{}?token={}", url, token)
        };

        log::info!("连接调试桥: {}", ws_url);
        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        // 写入任务：从通道取消息发到 WebSocket
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(Message::text(msg)).await.is_err() {
                    break;
                }
            }
        });

        let inner = Arc::new(Mutex::new(BridgeInner {
            tx,
            pending: std::collections::HashMap::new(),
            seq: 0,
        }));

        // 读取任务：收到响应按 id 匹配 pending
        {
            let inner = inner.clone();
            tokio::spawn(async move {
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                                if let Some(id) = parsed.get("id").and_then(|v| v.as_str()) {
                                    let mut guard = inner.lock().await;
                                    if let Some(sender) = guard.pending.remove(id) {
                                        let _ = sender.send(parsed);
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            log::info!("调试桥连接已关闭");
                            break;
                        }
                        Err(e) => {
                            log::error!("调试桥读取错误: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
                // 连接断开时清空 pending，避免请求永久挂起
                let mut guard = inner.lock().await;
                guard.pending.clear();
            });
        }

        *self.inner.lock().await = Some(inner);
        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&self) {
        let mut guard = self.inner.lock().await;
        *guard = None;
    }

    /// 是否已连接
    pub async fn is_connected(&self) -> bool {
        self.inner.lock().await.is_some()
    }

    /// 发送请求并等待响应
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let (id, resp_rx) = {
            let guard = self.inner.lock().await;
            let inner_arc = guard.as_ref().ok_or_else(|| {
                anyhow::anyhow!("未连接调试桥，请先调用 ax_connect_bridge")
            })?;
            let mut inner = inner_arc.lock().await;
            inner.seq += 1;
            let id = format!("req_{}", inner.seq);
            let (tx, rx) = oneshot::channel();
            inner.pending.insert(id.clone(), tx);

            let req = json!({
                "id": id,
                "method": method,
                "params": params,
            });
            let line = serde_json::to_string(&req)?;
            inner.tx.send(line).map_err(|_| anyhow::anyhow!("发送请求失败：通道已关闭"))?;
            (id, rx)
        };

        // 等待响应（带超时）
        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), resp_rx)
            .await
            .map_err(|_| {
                // 超时清理 pending
                let id_clone = id.clone();
                let inner = self.inner.clone();
                tokio::spawn(async move {
                    if let Some(arc) = inner.lock().await.as_ref() {
                        arc.lock().await.pending.remove(&id_clone);
                    }
                });
                anyhow::anyhow!("调试桥请求超时")
            })??;

        Ok(resp)
    }

    /// 获取配置中的默认连接信息
    pub fn default_config(&self) -> &crate::config::BridgeConfig {
        &self.config
    }
}
