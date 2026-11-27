# Ashira

用 Rust 写的编程助手。可以在终端（TUI）里用，也可以启动 HTTP API 后用网页或 App 操作。

## 功能

- 支持 Anthropic Messages、OpenAI Chat Completions 和 Responses，可切换模型和思考级别
- 读写文件、搜索、执行命令；Shell 命令和 MCP 工具可按会话选择审批模式
- 显示模型的思考过程，长的自动折叠
- 模型可以向你提问（单选、多选、填空），回答后继续
- 自动保存会话，可恢复、重命名、归档；上下文快满时自动压缩
- 待办清单、后台命令和子代理、插件、MCP、全局记忆。后台命令显示输出，子代理可打开独立对话。
- 开发服务器和文件监听器可作为常驻任务启动，不受普通后台命令的一小时上限，也不会收到空闲提醒。让 AI 通过 `bash` 的 `persistent: true` 启动，用 `task_status` 查看日志位置、用 `task_cancel` 停止；退出 Ashira 或清理所属会话时服务也会停止。
- 网页和 App：多会话、文件浏览与编辑、发送图片和长文本文件、用量统计。超过 1 MiB 的 UTF-8 文本按段只读预览。

## 安装

Windows（PowerShell）：

```powershell
irm https://raw.githubusercontent.com/Night-stars-1/Ashira/master/install.ps1 | iex
```

Linux x64 / macOS（Apple Silicon）：

```sh
curl -fsSL https://raw.githubusercontent.com/Night-stars-1/Ashira/master/install.sh | sh
```

更新到最新版本：

```sh
ashira update            # 或 ashira update v0.2.0 安装指定版本
```

网页和 App（Windows、Android）在 [Release](https://github.com/Night-stars-1/Ashira/releases) 页面下载。

## 构建

Core：

```sh
cargo build --release
```

Web：

```sh
cd web && bun install && bun run build
```

App：

```sh
cd app
flutter build apk --release
flutter build windows --release
```

## 配置

配置放在 `~/.ashira`（可用 `ASHIRA_HOME` 修改）。

```yaml
# ~/.ashira/models.yml
providers:
  anthropic:
    api: anthropic-messages
    baseUrl: https://api.anthropic.com
    apiKey: ANTHROPIC_API_KEY   # 环境变量名
    discovery:
      type: openai-models-list
```

```yaml
# ~/.ashira/config.yml
modelRoles:
  default: anthropic/claude-sonnet-4-5
```

## 使用

```sh
ashira                    # 交互界面
ashira "检查这个项目"       # 启动并发送任务
ashira -p "总结 src 目录"   # 非交互，只输出结果
ashira --resume           # 恢复最近的会话
ashira --serve            # 启动 HTTP API（127.0.0.1:8787）
```

网页开发模式：先 `ashira --serve`，再 `cd web && bun dev`。

在局域网开放 API 时要设置 Token，网页和 App 首次连接会要求输入：

```sh
ASHIRA_API_TOKEN=<随机字符串> ashira --listen 0.0.0.0:8787
```

在 Web 和 App 的聊天输入区切换审批模式，或在终端使用 `/approval [manual|auto|full]`。切换只影响当前会话。新会话的起始模式在「设置 → 通用 → 审批」中设置，保存在 `config.yml` 的 `approval.default`；关闭「在输入框显示审批按钮」（`approval.hidden: true`）后，聊天输入区不再显示切换按钮，所有会话都按默认模式运行。

| 模式 | 行为 |
|---|---|
| 手动审批（默认） | 检测到危险 Shell 命令或调用未经授权的 MCP 工具时询问。 |
| 自动审批 | 放行普通命令及已授权的 MCP 工具；自动拒绝检测到的危险命令和未经授权的 MCP 工具。 |
| 完整权限 | 跳过上述确认，但不提权，也不解除工具的角色限制。`--yes` 以此模式启动。 |

### 常用按键（TUI）

| 按键 | 作用 |
|---|---|
| Enter | 发送；回合进行中则排队 |
| Ctrl+Enter | 回合进行中把消息插入当前回合 |
| Shift+Enter / Ctrl+J | 换行 |
| Alt+↑ | 取回最后一条排队消息 |
| Esc | 中断当前回合 |
| Shift+Tab | 切换思考级别 |

### 常用命令（TUI）

| 命令 | 作用 |
|---|---|
| `/models` | 选择模型 |
| `/thinking` | 设置思考级别 |
| `/approval [manual\|auto\|full]` | 查看或切换当前会话的审批模式 |
| `/compact` | 压缩上下文 |
| `/clear` | 开始新会话 |
| `/usage` | 查看用量 |
| `/mcp` `/memory` `/plugins` | 管理 MCP、记忆、插件 |
| `/help` | 查看全部命令 |

## 致谢

感谢 [oh-my-pi](https://github.com/can1357/oh-my-pi) 为终端交互和工具扩展设计提供参考。
