# ArcartX AI 调试工具 — 使用文档与部署指南

> 本文档面向 AI Agent（LLM），描述如何使用 ArcartX AI 调试工具套件进行 UI 开发调试、ARIA 脚本分析、运行时诊断和自动化测试。

---

## 目录

- [架构总览](#架构总览)
- [部署方式](#部署方式)
  - [组件 1：axs-debug 服务端调试桥模块](#组件-1axs-debug-服务端调试桥模块)
  - [组件 2：Herald 客户端 Mod（Fork）](#组件-2herald-客户端-modfork)
  - [组件 3：ArcartX-Debug-MCP Server](#组件-3arcartx-debug-mcp-server)
  - [MCP 客户端配置](#mcp-客户端配置)
- [AI 使用文档](#ai-使用文档)
  - [工作流 A：离线 UI 静态分析](#工作流-a离线-ui-静态分析)
  - [工作流 B：在线 UI 调试（打开 → 截图 → 分析 → 修改 → 重载）](#工作流-b在线-ui-调试打开--截图--分析--修改--重载)
  - [工作流 C：ARIA 脚本调试](#工作流-caria-脚本调试)
  - [工作流 D：自动化 UI 测试](#工作流-d自动化-ui-测试)
  - [工作流 E：模块代码变更部署（提交 → CI 构建 → 云端同步）](#工作流-e模块代码变更部署提交--ci-构建--云端同步)
- [API 参考手册](#api-参考手册)
  - [MCP Server 工具列表（17 个）](#mcp-server-工具列表17-个)
  - [调试桥 RPC 方法列表（16 个）](#调试桥-rpc-方法列表16-个)
  - [Herald 客户端 Action 列表（317 个）](#herald-客户端-action-列表317-个)
- [典型调试场景示例](#典型调试场景示例)

---

## 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                     AI Agent (LLM)                          │
│                   通过 MCP 协议调用工具                       │
└──────────────────────────┬──────────────────────────────────┘
                           │ MCP (stdio / SSE)
┌──────────────────────────▼──────────────────────────────────┐
│              ArcartX-Debug-MCP Server (Rust)                │
│                                                             │
│  离线工具（7个）              在线工具（10个）                 │
│  ├─ ax_parse_ui_yaml         ├─ ax_connect_bridge            │
│  ├─ ax_analyze_aria_script   ├─ ax_get_ui_config             │
│  ├─ ax_validate_ui_structure ├─ ax_reload_ui                 │
│  ├─ ax_find_control_refs     ├─ ax_open_ui                   │
│  ├─ ax_check_anti_patterns   ├─ ax_close_ui                  │
│  ├─ ax_list_ui_files         ├─ ax_send_ui_packet            │
│  └─ ax_search_in_files       ├─ ax_eval_aria                 │
│                              ├─ ax_reload_module             │
│                              ├─ ax_list_modules              │
│                              └─ ax_run_server_command        │
└──────────┬──────────────────────────────┬───────────────────┘
           │ WebSocket JSON-RPC            │ HTTP REST
┌──────────▼──────────────┐  ┌────────────▼──────────────────┐
│  axs-debug 调试桥模块     │  │  Herald 客户端 Mod (Fork)      │
│  (服务端 Bukkit 插件)     │  │  (客户端 Fabric Mod)          │
│                         │  │                               │
│  WebSocket :18899        │  │  HTTP :8888-8898              │
│  Token: axs-debug-local  │  │  317 个 Action                │
│                         │  │  ├─ 311 原始 Herald Action    │
│  16 个 RPC 方法          │  │  └─ 6 ArcartX 专属 Action     │
│  ├─ UI 操控 (7)          │  │     ├─ ax_listen_ui_events    │
│  ├─ ARIA 脚本 (2)        │  │     ├─ ax_get_ui_events       │
│  ├─ 模块管理 (2)         │  │     ├─ ax_capture_packets     │
│  ├─ 配置诊断 (1)         │  │     ├─ ax_get_captured_packets│
│  ├─ 玩家 (1)             │  │     ├─ ax_send_packet         │
│  ├─ 命令 (1)             │  │     └─ ax_get_global_storage  │
│  ├─ 日志 (1)             │  │                               │
│  └─ 包流 (3)             │  │  端口/Token 写入:             │
│                         │  │  <gameDir>/.herald/           │
└─────────────────────────┘  └───────────────────────────────┘
           │                              │
    ┌──────▼──────┐              ┌────────▼────────┐
    │ Minecraft   │◄── 网络连接 ──│ Minecraft 客户端 │
    │ 服务端      │              │ (ArcartX Mod)   │
    │ (Paper 1.20)│              └─────────────────┘
    └─────────────┘
```

**三组件分工**：

| 组件 | 角色 | 运行位置 | 通信方式 |
|---|---|---|---|
| axs-debug | 服务端调试桥 | 服务端 Bukkit 插件 | WebSocket :18899 |
| Herald fork | 客户端操控 | 客户端 Fabric Mod | HTTP :8888-8898 |
| ArcartX-Debug-MCP | AI 工具层 | 独立进程 | MCP (stdio/SSE) |

---

## 部署方式

### 前置条件

- Minecraft 服务端：Paper 1.20.1+
- ArcartXSuite 主插件已安装且正常加载（含 Native 安全库）
- ArcartX 客户端 Mod 已安装（Fabric 1.20.1）
- Rust 工具链（编译 MCP Server，可选，也可用预编译二进制）
- JDK 17+（编译 debug 模块）

---

### 组件 1：axs-debug 服务端调试桥模块

#### 1.1 编译

```bash
cd ArcartXSuite
gradlew.bat :modules:debug:jar --no-daemon --console=plain
```

产物：`modules/debug/build/libs/ArcartXSuite-Debug-1.3.2.jar`

> **注意**：debug 模块已从 `buildAll`/`buildModules`/`encryptAllModuleAxb`/`buildDev` 等批量构建任务中排除，不会进入生产构建或云端分发。只能通过 `:modules:debug:jar` 单独编译。

#### 1.2 部署

1. 将 jar 复制到服务端的模块目录：
   ```
   <服务端根目录>/plugins/ArcartX-Suite/modules/ArcartXSuite-Debug-1.3.2.jar
   ```

2. 在 `plugins/ArcartX-Suite/config.yml` 中启用 debug 模块：
   ```yaml
   modules:
     debug:
       enabled: true
   ```

3. 重启服务端。启动日志中应出现：
   ```
   ◆ ArcartXSuite | [debug] INFO: Debug Bridge 已启动，WebSocket 监听 0.0.0.0:18899 (token=axs-debug-local)
   ```

#### 1.3 配置

调试桥的监听端口和 Token 在 `DebugWebSocketServer.kt` 中硬编码：

| 配置项 | 默认值 | 说明 |
|---|---|---|
| 监听地址 | `0.0.0.0:18899` | WebSocket 服务端地址 |
| Token | `axs-debug-local` | 认证令牌（URL query 参数 `?token=xxx`） |

> 生产环境建议修改 Token 并限制监听地址为 `127.0.0.1`。

#### 1.4 验证

```python
import asyncio, websockets, json

async def test():
    async with websockets.connect('ws://127.0.0.1:18899/?token=axs-debug-local') as ws:
        await ws.send(json.dumps({'jsonrpc':'2.0','id':'1','method':'module.list'}))
        print(await ws.recv())

asyncio.run(test())
```

---

### 组件 2：Herald 客户端 Mod（Fork）

#### 2.1 编译

```bash
cd Herald-MCClientMCP/client-mod/1.20.1
gradlew.bat :fabric:build --no-daemon --console=plain
```

产物：`fabric/build/libs/herald-client-fabric-0.1.0.jar`

> **Gradle Wrapper 配置**：如果官方 Gradle 下载超时，修改 `gradle/wrapper/gradle-wrapper.properties` 的 `distributionUrl` 为阿里云镜像：
> `https\://mirrors.aliyun.com/macports/distfiles/gradle/gradle-8.10.2-bin.zip`

#### 2.2 部署

1. 将 jar 复制到客户端 mods 目录：
   ```
   <gameDir>/mods/herald-client-fabric-0.1.0.jar
   ```

2. 确保客户端已安装：
   - Fabric Loader 0.15.0+
   - Fabric API 0.92.7+1.20.1
   - ArcartX Fabric Mod 1.20.1

3. 启动客户端。日志中应出现：
   ```
   Booting Herald Client 0.1.0 on fabric (MC 1.20.1)
   Herald HTTP server listening on 127.0.0.1:8888
   [Herald-Client] ready on 127.0.0.1:8888 loader=fabric mc=1.20.1 actions=317
   ```

#### 2.3 端口与 Token 发现

Herald 启动后，端口和 Token 写入以下文件：

| 文件 | 内容 | 说明 |
|---|---|---|
| `<gameDir>/.herald/client-port` | `8888` | HTTP 服务监听端口 |
| `<gameDir>/.herald/client-token` | `b01da04a-eea8-...` | Bearer Token |

> 端口范围 8888-8898，自动选择第一个可用端口。

#### 2.4 验证

```bash
# 读取端口和 Token
port=$(cat .herald/client-port)
token=$(cat .herald/client-token)

# 测试连接
curl -H "Authorization: Bearer $token" http://127.0.0.1:$port/ping
```

预期返回：
```json
{"status":"success","data":{"ok":true,"mod_version":"0.1.0","mc_version":"1.20.1","loader":"fabric","registered_actions":317}}
```

---

### 组件 3：ArcartX-Debug-MCP Server

#### 3.1 编译

```bash
cd ArcartX-Debug-MCP
cargo build --release
```

产物：`target/release/arcartx-debug-mcp.exe`（Windows）或 `arcartx-debug-mcp`（Linux/macOS）

#### 3.2 配置

在 MCP Server 目录下创建 `config.toml`：

```toml
[server]
mode = "stdio"                    # "stdio" 或 "sse"
listen = "127.0.0.1:18900"        # SSE 模式监听地址

[workspace]
path = "<服务端根目录>"            # 指向服务端根目录（含 plugins/ArcartX-Suite/ui/）

[bridge]
url = "ws://127.0.0.1:18899"      # 调试桥 WebSocket 地址
token = "axs-debug-local"          # 调试桥 Token
auto_connect = false               # 是否启动时自动连接
```

> **workspace.path 说明**：必须指向服务端根目录（如 `D:\Server\island`），MCP Server 会自动检测 `plugins/ArcartX-Suite/ui/` 或 `plugins/ArcartXSuite/ui/` 目录。

#### 3.3 运行

```bash
# stdio 模式（供 MCP 客户端通过 stdin/stdout 通信）
./arcartx-debug-mcp

# SSE 模式（HTTP 服务，供远程 MCP 客户端连接）
# 修改 config.toml 中 mode = "sse" 后运行
./arcartx-debug-mcp
```

---

### MCP 客户端配置

#### Claude Desktop / Cursor / Windsurf

在 MCP 客户端配置文件中添加：

```json
{
  "mcpServers": {
    "arcartx-debug": {
      "command": "D:\\path\\to\\arcartx-debug-mcp.exe",
      "args": [],
      "cwd": "D:\\path\\to\\ArcartX-Debug-MCP"
    }
  }
}
```

> `cwd` 必须指向包含 `config.toml` 的目录。

#### Devin CLI

在 `.devin/config.json` 中配置 MCP Server：

```json
{
  "mcpServers": {
    "arcartx-debug": {
      "command": "D:\\path\\to\\arcartx-debug-mcp.exe",
      "cwd": "D:\\path\\to\\ArcartX-Debug-MCP"
    }
  }
}
```

---

## AI 使用文档

### 工作流 A：离线 UI 静态分析

**适用场景**：不需要运行服务端，直接分析 workspace 中的 UI yml 文件。

**步骤**：

1. **列出所有 UI 文件**
   ```
   工具: ax_list_ui_files
   参数: {}
   返回: 60 个 UI 文件列表
   ```

2. **解析 UI 控件树**
   ```
   工具: ax_parse_ui_yaml
   参数: { "file_path": "plugins/ArcartX-Suite/ui/lottery_case.yml" }
   返回: ui 块属性 + controls 控件树 + template 模板列表
   ```

3. **校验 UI 结构合法性**
   ```
   工具: ax_validate_ui_structure
   参数: { "file_path": "plugins/ArcartX-Suite/ui/lottery_case.yml" }
   返回: issue 列表（必需属性缺失、未知控件类型、属性类型错误等）
   ```

4. **检测反模式**
   ```
   工具: ax_check_anti_patterns
   参数: { "file_path": "plugins/ArcartX-Suite/ui/lottery_case.yml" }
   返回: 反模式列表（ARIA 引用不存在控件、packetHandler 访问未初始化控件等）
   ```

5. **查找控件引用**
   ```
   工具: ax_find_control_refs
   参数: { "ui_id": "lottery_case", "control_name": "background" }
   返回: 引用位置列表（文件+行号+上下文）
   ```

6. **搜索文件内容**
   ```
   工具: ax_search_in_files
   参数: { "pattern": "packetHandler", "glob": "*.yml" }
   返回: 匹配位置列表
   ```

7. **静态分析 ARIA 脚本**
   ```
   工具: ax_analyze_aria_script
   参数: { "script": "var.title = packet['title']", "context": "ui.packetHandler.init" }
   返回: 变量引用、函数调用、未定义变量检测
   ```

---

### 工作流 B：在线 UI 调试（打开 → 截图 → 分析 → 修改 → 重载）

**适用场景**：服务端运行中，需要实际打开 UI 查看渲染效果，发现问题后修改并热重载。

**步骤**：

1. **连接调试桥**
   ```
   工具: ax_connect_bridge
   参数: {}
   返回: { "connected": true, "url": "ws://127.0.0.1:18899" }
   ```

2. **确认模块状态**
   ```
   工具: ax_list_modules
   参数: {}
   返回: 7 个模块全部 ready=true
   ```

3. **获取 UI 配置内容**
   ```
   工具: ax_get_ui_config
   参数: { "ui_id": "lottery_case" }
   返回: yml 文件完整内容
   ```

4. **让玩家打开 UI**（两种方式）

   **方式 A：通过调试桥直接打开**（不经过模块逻辑，UI 无初始化数据）
   ```
   工具: ax_open_ui
   参数: { "player": "LiuYun_King", "ui_id": "lottery_case" }
   ```

   **方式 B：通过 Herald 模拟玩家执行命令**（推荐，经过完整模块逻辑）
   ```
   Herald Action: chat_command
   参数: { "command": "lottery open default_weapon_case" }
   ```

5. **截图查看渲染效果**
   ```
   Herald Action: screenshot
   参数: {}
   返回: 截图保存到 <gameDir>/screenshots/ 目录
   ```

6. **发现问题后修改 yml 文件**（AI 直接编辑文件）

7. **热重载 UI**
   ```
   工具: ax_reload_ui
   参数: { "ui_id": "lottery_case" }
   返回: 重载结果
   ```

8. **重新打开 UI 验证修复**
   ```
   重复步骤 4-5
   ```

9. **关闭 UI**
   ```
   工具: ax_close_ui
   参数: { "player": "LiuYun_King", "ui_id": "AXS:lottery_case" }
   ```

> **UI ID 命名空间**：模块注册的 UI ID 格式为 `模块名:ui_id`（如 `AXS:lottery_case`），直接在 ui/ 目录下的 UI ID 为文件名（如 `lottery_case`）。关闭时需要用注册时的完整 ID。

---

### 工作流 C：ARIA 脚本调试

**适用场景**：需要验证 ARIA 脚本逻辑、调试变量值、测试函数调用。

**步骤**：

1. **检查 ARIA 可用性**
   ```
   工具: ax_eval_aria (会自动检查)
   或直接调用调试桥: aria.available
   返回: { "available": true, "version": "new" }
   ```

2. **执行简单表达式**
   ```
   工具: ax_eval_aria
   参数: { "code": "1 + 2 * 3" }
   返回: { "result": 7 }
   ```

3. **执行带绑定的脚本**
   ```
   工具: ax_eval_aria
   参数: {
     "code": "player('LiuYun_King').getHealth()",
     "bindings": {}
   }
   ```

4. **静态分析脚本问题**
   ```
   工具: ax_analyze_aria_script
   参数: { "script": "var.x = packet['title']; self['label'].texts = var.x" }
   返回: 变量引用分析、未定义变量检测
   ```

5. **结合离线分析 + 在线验证**
   - 先用 `ax_analyze_aria_script` 静态分析发现问题
   - 再用 `ax_eval_aria` 在服务端实际执行验证

---

### 工作流 D：自动化 UI 测试

**适用场景**：自动化测试 UI 交互流程（打开 → 点击 → 验证 → 关闭）。

**步骤**：

1. **通过 Herald 监听 UI 事件**
   ```
   Herald Action: ax_listen_ui_events
   参数: { "eventTypes": ["screen_open", "screen_close", "layer_open", "layer_close"] }
   返回: { "listenerId": "ui-xxx", "status": "listening" }
   ```

   > **有效事件类型**：`screen_open`、`screen_close`、`layer_open`、`layer_close`、`layer_render`
   > **注意**：`layer_render` 每帧触发，量很大，仅在需要时监听。

2. **通过 Herald 模拟玩家执行命令打开 UI**
   ```
   Herald Action: chat_command
   参数: { "command": "lottery open default_weapon_case" }
   ```

3. **获取 UI 事件验证 UI 已打开**
   ```
   Herald Action: ax_get_ui_events
   参数: { "maxCount": 20 }
   返回: { "events": [{ "type": "screen_open", "uiId": "AXS:lottery_case" }] }
   ```

4. **截图记录当前状态**
   ```
   Herald Action: screenshot
   ```

5. **模拟鼠标点击 UI 按钮**
   ```
   Herald Action: mouse_click
   参数: { "x": 960, "y": 540, "button": 0 }
   ```

6. **模拟键盘输入**
   ```
   Herald Action: keyboard_input
   参数: { "key": "escape", "action": "press" }
   ```

7. **获取 UI 事件验证交互结果**
   ```
   Herald Action: ax_get_ui_events
   ```

8. **查询玩家状态变化**
   ```
   Herald Action: query_player_state
   返回: 坐标、血量、饥饿、物品栏等
   ```

9. **查询屏幕状态**
   ```
   Herald Action: query_screen_state
   返回: { "open": true/false, "screenClass": "...", "title": "..." }
   ```

10. **通过调试桥关闭 UI**
    ```
    工具: ax_close_ui
    参数: { "player": "LiuYun_King", "ui_id": "AXS:lottery_case" }
    ```

11. **验证 screen_close 事件**
    ```
    Herald Action: ax_get_ui_events
    返回: { "events": [{ "type": "screen_close", "uiId": "AXS:lottery_case" }] }
    ```

---

## API 参考手册

### MCP Server 工具列表（17 个）

#### 离线工具（7 个，不需要服务端运行）

| # | 工具名 | 参数 | 说明 |
|---|---|---|---|
| 1 | `ax_list_ui_files` | `workspace?` | 列出 workspace 中所有 UI yml 文件 |
| 2 | `ax_parse_ui_yaml` | `file_path` | 解析 UI yml，返回结构化控件树 |
| 3 | `ax_validate_ui_structure` | `file_path` | 校验 UI 结构合法性 |
| 4 | `ax_find_control_refs` | `ui_id`, `control_name` | 查找控件引用位置 |
| 5 | `ax_check_anti_patterns` | `file_path` | 检测 UI 反模式 |
| 6 | `ax_search_in_files` | `pattern`, `workspace?`, `glob?` | 搜索 UI 文件内容 |
| 7 | `ax_analyze_aria_script` | `script`, `context?` | 静态分析 ARIA 脚本 |

#### 在线工具（10 个，需要连接调试桥）

| # | 工具名 | 参数 | 说明 |
|---|---|---|---|
| 8 | `ax_connect_bridge` | `url?`, `token?` | 连接服务端调试桥 |
| 9 | `ax_get_ui_config` | `ui_id` | 获取服务端 UI yml 内容 |
| 10 | `ax_reload_ui` | `ui_id` | 热重载 UI |
| 11 | `ax_open_ui` | `player`, `ui_id` | 为玩家打开 UI |
| 12 | `ax_close_ui` | `player`, `ui_id` | 为玩家关闭 UI |
| 13 | `ax_send_ui_packet` | `player`, `ui_id`, `handler`, `payload?` | 向 UI 发包 |
| 14 | `ax_eval_aria` | `code`, `bindings?` | 执行 ARIA 脚本 |
| 15 | `ax_reload_module` | `module_id` | 重载模块 |
| 16 | `ax_list_modules` | (无) | 列出已加载模块 |
| 17 | `ax_run_server_command` | `command` | 执行服务端命令 |

---

### 调试桥 RPC 方法列表（16 个）

**连接方式**：WebSocket `ws://<host>:18899/?token=axs-debug-local`

**协议**：JSON-RPC 2.0
```json
{ "jsonrpc": "2.0", "id": "1", "method": "方法名", "params": { ... } }
```

#### UI 操控（7 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `ui.list` | (无) | `uiFiles: [string]` | 列出 ui/ 目录所有 yml 文件 |
| `ui.read` | `uiId` | `uiId, content` | 读取 UI yml 文件内容 |
| `ui.reload` | `uiId` | `success, runtimeUiId, registeredUiId, action, message` | 热重载 UI |
| `ui.open` | `player, uiId` | `success, message` | 打开 UI |
| `ui.close` | `player, uiId` | `success, message` | 关闭 UI |
| `ui.is_open` | `player, uiId` | `open: boolean` | 查询 UI 是否打开 |
| `ui.send_packet` | `player, uiId, handler, payload?` | `success, message` | 向 UI 发包 |

#### ARIA 脚本（2 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `aria.available` | (无) | `available: boolean, version: string` | 检查 ARIA 可用性 |
| `aria.eval` | `code, bindings?` | `result: any` | 执行 ARIA 脚本 |

#### 模块管理（2 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `module.list` | (无) | `modules: [{id, ready}]` | 列出已加载模块 |
| `module.reload` | `moduleId` | `success, message` | 重载模块 |

#### 配置诊断（1 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `config.diagnose` | `moduleId` | `moduleId, specs: [...]` | 获取模块配置规约 |

#### 玩家（1 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `player.list` | (无) | `players: [{name, uuid, world}]` | 列出在线玩家 |

#### 服务端命令（1 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `server.command` | `command, sender?` | `success, message` | 执行命令（默认控制台身份，可指定玩家） |

#### 日志（1 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `log.tail` | `lines?` | `lines: [string], total: int` | 读取最近 N 行日志（默认 100） |

#### 包流捕获（3 个）

| 方法 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `packet.status` | (无) | `size: int` | 查询捕获缓冲区大小 |
| `packet.get_captured` | `maxCount?` | `packets: [...], count` | 获取并清空捕获的包 |
| `packet.clear` | (无) | `success: boolean` | 清空捕获缓冲区 |

---

### Herald 客户端 Action 列表（317 个）

**连接方式**：HTTP `http://127.0.0.1:<port>/action/<action_id>`

**协议**：POST + Bearer Token
```
POST /action/<action_id>
Authorization: Bearer <token>
Content-Type: application/json

{ "参数名": "参数值", ... }
```

#### ArcartX 专属 Action（6 个）

| Action | 参数 | 返回 | 说明 |
|---|---|---|---|
| `ax_listen_ui_events` | `eventTypes?: [string]` | `listenerId, status, eventTypes` | 开始监听 ArcartX UI 事件 |
| `ax_get_ui_events` | `maxCount?, since?` | `events: [...], count, listening` | 获取已捕获的 UI 事件 |
| `ax_capture_packets` | `filter?: string` | `captureId, status` | 开始捕获 ArcartX 自定义包 |
| `ax_get_captured_packets` | `maxCount?, since?` | `packets: [...], count, capturing` | 获取已捕获的包 |
| `ax_send_packet` | `id, args: [string]` | `status, id, argCount` | 发送 ArcartX 自定义包 |
| `ax_get_global_storage` | `key` | `key, exists, value, number, boolean` | 读取 ArcartX GlobalStorage |

**UI 事件类型**：`screen_open`、`screen_close`、`layer_open`、`layer_close`、`layer_render`

#### 核心 Action 分类（311 个原始 Herald Action）

| 分类 | 代表 Action | 说明 |
|---|---|---|
| **玩家操控** | `player_move`, `player_look`, `jump`, `sneak_start`, `sprint_start` | 移动、视角、跳跃、潜行、冲刺 |
| **交互** | `use_item`, `attack_entity`, `interact_entity`, `place_block`, `break_block` | 使用物品、攻击实体、交互、放置/破坏方块 |
| **聊天/命令** | `chat_message`, `chat_command`, `query_chat_history` | 发送消息、执行命令、查询聊天历史 |
| **容器/UI** | `click_slot`, `close_container`, `query_container_state`, `query_screen_state` | 点击槽位、关闭容器、查询状态 |
| **截图/视觉** | `screenshot`, `gui_screenshot_element`, `gui_list_widgets` | 截图、元素截图、列出控件 |
| **输入模拟** | `keyboard_input`, `mouse_click`, `mouse_move`, `gui_type_text` | 键盘、鼠标、文字输入 |
| **状态查询** | `query_player_state`, `query_full_inventory`, `query_world_state`, `query_entity_detail` | 玩家/物品栏/世界/实体状态 |
| **导航/寻路** | `navigate_to`, `pathfind_to`, `fly_to` | 自动寻路、飞行 |
| **建造/挖掘** | `build_fill`, `build_wall`, `mine_area`, `mine_tunnel` | 批量建造、挖掘 |
| **战斗** | `combat_target_nearest`, `combat_dodge`, `combat_combo` | 自动战斗 |
| **断言/测试** | `assert_player_health`, `assert_block`, `assert_inventory_contains` | 测试断言 |
| **任务编排** | `task_create`, `task_list`, `goal_reach_location` | 异步任务、目标编排 |
| **包操控** | `packet_capture_start`, `packet_intercept`, `packet_send_custom` | 原版包捕获/拦截/发送 |
| **调试** | `debug_set_gamemode`, `debug_teleport`, `debug_give_item`, `debug_set_time` | 调试快捷操作 |

> 完整 Action 列表可通过 `GET /actions` API 获取。

---

## 典型调试场景示例

### 场景 1：UI 打开后白屏

```
1. ax_list_ui_files → 找到目标 UI 文件
2. ax_parse_ui_yaml → 检查控件树结构
3. ax_validate_ui_structure → 发现缺少必需属性
4. ax_check_anti_patterns → 发现 ARIA 引用不存在的控件
5. 修复 yml 文件
6. ax_reload_ui → 热重载
7. Herald: chat_command → 重新打开 UI
8. Herald: screenshot → 验证修复
```

### 场景 2：UI 按钮点击无响应

```
1. Herald: ax_listen_ui_events → 监听 UI 事件
2. Herald: chat_command → 打开 UI
3. Herald: mouse_click → 点击按钮位置
4. Herald: ax_get_ui_events → 检查是否触发交互事件
5. ax_get_ui_config → 读取 UI 配置，检查按钮的 click 触发器
6. ax_analyze_aria_script → 分析 click 触发器中的 ARIA 脚本
7. 修复脚本
8. ax_reload_ui → 热重载
9. 重新测试
```

### 场景 3：packetHandler 收不到数据

```
1. ax_get_ui_config → 读取 UI 配置，检查 packetHandler 定义
2. Herald: ax_capture_packets → 开始捕获自定义包
3. Herald: chat_command → 打开 UI（触发服务端 sendPacket）
4. Herald: ax_get_captured_packets → 检查是否收到包
   (注意：packetBridge.sendPacket 走 ArcartX UI 通讯通道，不走 CustomPacketEvent)
5. 调试桥: ui.send_packet → 直接向 UI 发包测试 packetHandler
6. Herald: screenshot → 检查 UI 是否响应
```

### 场景 4：ARIA 脚本运行时报错

```
1. ax_analyze_aria_script → 静态分析发现未定义变量
2. ax_eval_aria → 在服务端执行脚本片段验证
3. ax_get_ui_config → 读取完整 UI 配置上下文
4. 修复 ARIA 脚本
5. ax_reload_ui → 热重载
6. Herald: chat_command → 重新打开 UI 验证
```

### 场景 5：模块配置问题导致功能异常

```
1. ax_list_modules → 确认模块 ready=true
2. 调试桥: config.diagnose → 获取模块配置规约
3. 调试桥: log.tail → 查看最近日志中的错误
4. ax_run_server_command → 执行诊断命令
5. 修复配置文件
6. ax_reload_module → 重载模块
7. 验证功能恢复
```

---

### 工作流 E：模块代码变更部署（提交 → CI 构建 → 云端同步）

**适用场景**：当修改了模块的 Java 代码（非 UI yml 热重载能解决的变更，如新增命令、Service 逻辑变更、Model 字段调整等），需要重新编译加密模块并通过云端分发到运行中的服务端。

> **与工作流 B 的区别**：工作流 B 适用于纯 UI yml 文件修改，可通过 `ax_reload_ui` 热重载即时生效；工作流 E 适用于 Java 代码变更，必须重新编译 `.axb` 加密模块包后通过 `axs sync` 更新。

**前提条件**：
- ArcartXSuite 仓库已托管在 GitHub（`https://github.com/xuanmomo233/ArcartXSuite`）
- GitHub Actions 工作流 `build-encrypted.yml` 已配置（push 到 main 自动触发）
- GitHub Secrets 已配置：`AXS_SEED_PARTS_B64`、`AXB_SIGN_SEED_B64`、`AXB_RESP_SIGN_SEED_B64`、`CLOUD_CI_TOKEN`
- 服务端已安装 ArcartXSuite 主插件且可访问云端（`cloud.021209.xyz`）

**步骤**：

1. **本地验证编译**
   ```bash
   cd ArcartXSuite
   gradlew.bat :modules:battlepass:compileJava --no-daemon
   ```
   确保编译通过，无语法错误。

2. **提交变更到 GitHub**
   ```bash
   git add modules/battlepass/src/main/java/... modules/battlepass/src/main/resources/messages.yml
   git commit -m "feat(battlepass): 新增 addexp/setexp/removeexp/setlevel 管理指令"
   git push origin main
   ```

3. **等待 GitHub CI 构建完成**
   - push 到 main 后自动触发 `build-encrypted.yml` 工作流
   - `detect-changes` 作业检测到 `modules/battlepass/` 路径变更 → `module_changed=true`
   - `build-modules-encrypted` 作业执行：
     - `gradle :modules:battlepass:encryptModuleAxb -Paxs.protectModules -PskipNativeCheck=true`
     - 逐类 AES-GCM/ChaCha20 加密生成 `.axb` 模块包
     - 上传到云端（主云 `cloud.021209.xyz` + FlareCloud 镜像），version 格式 `<BASEVER>-ci.<短sha>`，`setCurrent=true`
   - 可在 GitHub Actions 页面查看构建进度：`https://github.com/xuanmomo233/ArcartXSuite/actions`
   - 构建通常需要 5-15 分钟，取决于队列和模块数量

4. **在服务端同步更新模块**
   ```
   通过调试桥执行服务端命令：
   工具: ax_run_server_command
   参数: { "command": "axs sync battlepass" }
   
   或通过 Herald 模拟玩家执行：
   Herald Action: chat_command
   参数: { "command": "axs sync battlepass" }
   ```
   `axs sync battlepass` 会从云端拉取最新版本的 `battlepass.axb`，替换服务端模块目录中的旧版本，并自动重载模块。

5. **验证模块更新成功**
   ```
   工具: ax_list_modules
   参数: {}
   返回: 确认 battlepass 模块 ready=true 且版本号已更新
   ```

6. **测试新功能**
   ```
   工具: ax_run_server_command
   参数: { "command": "axs battlepass help" }
   返回: 确认新增的 addexp/setexp/removeexp/setlevel 子命令出现在帮助列表中
   ```

**注意事项**：
- **只改 UI yml 不需要走此流程**：UI 文件修改可直接编辑服务端 `plugins/ArcartX-Suite/ui/` 下的文件 + `ax_reload_ui` 热重载，无需提交 GitHub。
- **本体(core)变更与模块变更独立**：如果只改了 `modules/battlepass/` 下的文件，CI 只会构建并上传 battlepass 模块的 `.axb`，不会重建本体。
- **密钥一致性**：模块加密使用与本体相同的 canonical root_seed（通过 `AXS_SEED_PARTS_B64` Secret），已部署的本体才能解密新模块。如果 Secret 未配置或变更过，新模块将无法被解密加载。
- **CI 构建失败时检查**：密钥扫描（硬编码 QQ/密码/JWT）、编译错误、ProGuard 混淆规则冲突都可能导致失败，在 Actions 日志中定位具体原因。
- **多模块同时变更**：如果一次提交改了多个模块，CI 会为每个变更的模块分别构建并上传独立的 `.axb`，需要分别 `axs sync <模块名>`。

---

## 注意事项

1. **停止服务端时不要用 `Stop-Process -Name java`**：这会杀死所有 Java 进程（包括客户端）。应使用 `stop` 命令或按 PID 精确停止。

2. **UI ID 命名空间**：模块注册的 UI ID 格式为 `模块名:ui_id`（如 `AXS:lottery_case`），关闭时需用完整 ID。打开时可用简短 ID。

3. **`layer_render` 事件量大**：每帧触发，仅在需要时监听。常规调试用 `screen_open`/`screen_close`/`layer_open`/`layer_close`。

4. **`ax_capture_packets` vs `packetBridge.sendPacket`**：Herald 的 `ax_capture_packets` 监听的是 ArcartX 客户端的 `CustomPacketEvent`，而服务端模块通过 `packetBridge.sendPacket` 发送的 UI 通讯包走不同通道，不会被捕获。要测试 packetHandler，用调试桥的 `ui.send_packet` RPC 方法。

5. **debug 模块不参与生产构建**：已从 `buildAll`/`buildModules`/`encryptAllModuleAxb`/`buildDev` 排除，仅通过 `:modules:debug:jar` 单独编译。

6. **Herald 端口动态分配**：范围 8888-8898，从 `<gameDir>/.herald/client-port` 读取。

7. **workspace.path 必须指向服务端根目录**：MCP Server 会自动检测 `plugins/ArcartX-Suite/ui/` 或 `plugins/ArcartXSuite/ui/`。
