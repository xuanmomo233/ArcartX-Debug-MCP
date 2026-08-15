// 配置模块：解析 config.toml，保存 workspace 路径、调试桥连接信息等

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 顶层配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub bridge: BridgeConfig,
}

/// 服务端传输配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 传输模式：stdio 或 sse
    #[serde(default = "default_mode")]
    pub mode: String,
    /// SSE 模式监听地址
    #[serde(default = "default_listen")]
    pub listen: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            listen: default_listen(),
        }
    }
}

/// workspace 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// 默认 workspace 根路径
    #[serde(default = "default_workspace")]
    pub path: String,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            path: default_workspace(),
        }
    }
}

/// 调试桥连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// 调试桥 WebSocket 地址
    #[serde(default = "default_bridge_url")]
    pub url: String,
    /// 认证 token
    #[serde(default = "default_bridge_token")]
    pub token: String,
    /// 是否启动时自动连接
    #[serde(default)]
    pub auto_connect: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            url: default_bridge_url(),
            token: default_bridge_token(),
            auto_connect: false,
        }
    }
}

fn default_mode() -> String {
    "stdio".to_string()
}
fn default_listen() -> String {
    "127.0.0.1:18900".to_string()
}
fn default_workspace() -> String {
    "D:\\IDEA\\project\\ArcartXSuite".to_string()
}
fn default_bridge_url() -> String {
    "ws://127.0.0.1:18899".to_string()
}
fn default_bridge_token() -> String {
    "axs-debug-local".to_string()
}

impl AppConfig {
    /// 从 TOML 文件加载配置，文件不存在则返回默认配置
    pub fn load(path: &str) -> Result<Self> {
        if !std::path::Path::new(path).exists() {
            log::warn!("配置文件 {} 不存在，使用默认配置", path);
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {}", path))?;
        let config: AppConfig = toml::from_str(&content)
            .with_context(|| format!("解析配置文件失败: {}", path))?;
        Ok(config)
    }

    /// 获取 workspace 路径
    pub fn workspace_path(&self) -> PathBuf {
        PathBuf::from(&self.workspace.path)
    }

    /// UI 文件目录：workspace/plugins/ArcartX-Suite/ui 或 workspace/plugins/ArcartXSuite/ui
    /// 自动检测两种目录名（ArcartX-Suite 为运行时目录名，ArcartXSuite 为源码目录名）
    pub fn ui_dir(&self) -> PathBuf {
        let ws = self.workspace_path();
        let plugins = ws.join("plugins");
        // 优先匹配 ArcartX-Suite（运行时目录名），其次 ArcartXSuite（源码目录名）
        for name in &["ArcartX-Suite", "ArcartXSuite"] {
            let ui = plugins.join(name).join("ui");
            if ui.is_dir() {
                return ui;
            }
        }
        // 都不存在时返回默认路径（让调用方报错）
        plugins.join("ArcartX-Suite").join("ui")
    }
}
