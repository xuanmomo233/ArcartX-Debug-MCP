# ArcartX-Debug-MCP

ArcartXSuite 专属 MCP (Model Context Protocol) Server，为 AI 客户端（Devin、Claude Desktop 等）提供 ArcartX 专属的离线分析工具和在线调试工具。

## 功能概述

本 MCP Server 是 ArcartXSuite AI 调试工具体系的一部分，提供两类工具：

- **离线工具**：直接读 workspace 文件（UI yml、配置、源码），做静态分析
- **在线工具**：通过 WebSocket 连接服务端调试桥模块（axs-debug），获取运行时数据

## 环境要求

- Rust 工具链（rustup + cargo）
- 网络可访问调试桥（在线工具需要服务端运行 axs-debug 模块）

## 构建

```bash
cargo build --release
```

## 配置

复制 `config.example.toml` 为 `config.toml`，按需修改：

```toml
[server]
mode = "stdio"              # 传输模式：stdio 或 sse
listen = "127.0.0.1:18900"  # SSE 模式监听地址

[workspace]
path = "D:\\path\\to\\your\\server"  # 指向服务端根目录（含 plugins/ArcartX-Suite/ui/）

[bridge]
url = "ws://127.0.0.1:18899"  # 调试桥 WebSocket 地址
token = "axs-debug-local"     # 认证 token
auto_connect = false          # 启动时是否自动连接调试桥
```

## 运行

### stdio 模式（供 Claude Desktop / Devin 等通过标准输入输出调用）

```bash
cargo run -- --mode stdio --config config.toml
```

### SSE 模式（HTTP/SSE 传输）

```bash
cargo run -- --mode sse --listen 127.0.0.1:18900 --config config.toml
```

SSE 模式下：
- `GET /sse` 建立 SSE 长连接，首个 `endpoint` 事件告知消息提交地址
- `POST /messages?session_id=xxx` 提交 JSON-RPC 请求

## MCP 协议

基于 JSON-RPC 2.0，支持以下方法：

- `initialize` — 握手，返回 server info 和 capabilities
- `notifications/initialized` — 客户端初始化完成通知
- `tools/list` — 返回工具列表（name + description + inputSchema）
- `tools/call` — 调用工具，返回结果
- `ping` — 心跳

## 工具列表

### 离线工具（读 workspace 文件）

| 工具名 | 说明 |
|--------|------|
| `ax_parse_ui_yaml` | 解析 UI yml，返回结构化控件树（ui 块 + controls + template） |
| `ax_analyze_aria_script` | 静态分析 ARIA 脚本：变量引用、函数调用、未定义检测、作用域问题 |
| `ax_validate_ui_structure` | 校验 UI 结构合法性：必需属性、未知控件类型、未知触发器、属性类型 |
| `ax_find_control_refs` | 查找控件在 UI 文件中的引用位置（文件+行号+上下文） |
| `ax_check_anti_patterns` | 检测反模式：未定义控件引用、packetHandler 访问未初始化控件、模板未使用、动态控件未清理、纯文本未用 ~ 前缀、entryKey 浮点陷阱 |
| `ax_list_ui_files` | 列出 workspace 中所有 UI 文件 |
| `ax_search_in_files` | 在 UI 文件中搜索文本（支持 glob 过滤） |

### 在线工具（通过 WebSocket 连服务端调试桥）

| 工具名 | 说明 | 对接调试桥 method |
|--------|------|-------------------|
| `ax_connect_bridge` | 连接服务端调试桥 | — |
| `ax_get_ui_config` | 获取服务端 UI 配置 | `ui.read` |
| `ax_reload_ui` | 热重载 UI | `ui.reload` |
| `ax_open_ui` | 打开 UI | `ui.open` |
| `ax_close_ui` | 关闭 UI | `ui.close` |
| `ax_send_ui_packet` | 向 UI 发包 | `ui.send_packet` |
| `ax_eval_aria` | 执行 ARIA 脚本 | `aria.eval` |
| `ax_reload_module` | 重载模块 | `module.reload` |
| `ax_list_modules` | 列出已加载模块 | `module.list` |
| `ax_run_server_command` | 执行服务端命令 | `server.command` |

## WebSocket 协议（连调试桥）

请求格式：
```json
{"id":"req_001","method":"ui.reload","params":{"uiId":"market_shop"}}
```

响应格式：
```json
{"id":"req_001","result":{"success":true}}
```

调试桥支持的 method（见 axs-debug 模块）：
- `ui.list`, `ui.read`, `ui.reload`, `ui.open`, `ui.close`, `ui.is_open`, `ui.send_packet`
- `aria.eval`, `aria.available`
- `module.list`, `module.reload`
- `player.list`, `server.command`, `log.tail`
- `packet.capture`, `packet.get_captured`

## 项目结构

```
ArcartX-Debug-MCP/
├── Cargo.toml
├── config.example.toml
├── README.md
└── src/
    ├── main.rs                 # 入口，解析参数选 stdio/SSE，MCP JSON-RPC 分发
    ├── config.rs               # 配置（workspace 路径、调试桥连接等）
    ├── transport/
    │   ├── mod.rs              # 传输层接口
    │   ├── stdio.rs            # MCP stdio 传输
    │   └── sse.rs              # MCP HTTP/SSE 传输
    └── tool/
        ├── mod.rs              # 工具注册和分发
        ├── offline/
        │   ├── mod.rs          # 离线工具注册
        │   ├── ui_yaml.rs      # UI YAML 解析
        │   ├── aria_analyzer.rs # ARIA 脚本静态分析
        │   └── anti_patterns.rs # 反模式检测
        └── online/
            ├── mod.rs          # 在线工具注册
            └── bridge_client.rs # WebSocket 客户端
```

## 客户端集成示例

### Claude Desktop（stdio）

在 `claude_desktop_config.json` 中添加：
```json
{
  "mcpServers": {
    "arcartx-debug": {
      "command": "path/to/arcartx-debug-mcp.exe",
      "args": ["--mode", "stdio", "--config", "path/to/config.toml"]
    }
  }
}
```

## 注意事项

- 离线工具直接读取 workspace 文件，无需服务端运行
- 在线工具需要服务端运行 axs-debug 调试桥模块，并配置正确的 WebSocket 地址和 token
- 本项目为个人本地使用，不考虑安全因素
