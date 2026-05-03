# ccswitch — Claude Code Provider Switcher

A lightweight shell-based provider switcher for Claude Code, inspired by [cc-switch](https://github.com/farion1231/cc-switch).

Supports switching between Kimi, GLM-5, MiniMax, Zhongzhuan relay, and native Claude — with changes persisted to `~/.claude/settings.json` so they survive terminal restarts.

---

## Quick Start

```bash
ccswitch list          # list all providers
ccswitch use kimi      # switch to Kimi k2.6
ccswitch use glm       # switch to GLM-5
ccswitch use minimax   # switch to MiniMax M2.7
ccswitch use zz        # switch to Zhongzhuan relay
ccswitch use cc        # switch back to native Claude
ccswitch status        # show current provider (shell env + settings.json)
```

Short aliases also work:

```bash
kimi      # use_kimi
glm       # use_glm
minimax   # use_minimax
zz        # use_zhongzhuan
cc        # use_claude_code (native)
model     # show_model (brief status)
```

---

## How It Works

When you switch providers, ccswitch does two things simultaneously:

1. **Exports shell environment variables** — takes effect immediately in the current terminal session.
2. **Writes to `~/.claude/settings.json` `.env` block** — persists across sessions; Claude Code reads this on startup.

This mirrors how the cc-switch desktop app works, without needing a GUI.

### Environment variables managed

| Variable | Purpose |
|---|---|
| `ANTHROPIC_BASE_URL` | API endpoint for the provider |
| `ANTHROPIC_AUTH_TOKEN` | Provider API key |
| `ANTHROPIC_MODEL` | Model name/ID |
| `API_TIMEOUT_MS` | Request timeout |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Reduces extra traffic for relay providers |

### Native Claude mode (`cc`)

Clears all of the above from both shell env and `settings.json`, restoring Claude Code's default OAuth/API key behavior.

---

## Installation

1. Clone or copy this repo to `~/workspace/ccswitch/`
2. Add one line to your `~/.bashrc`:

```bash
source ~/workspace/ccswitch/ccswitch.sh
```

3. Reload:

```bash
source ~/.bashrc
```

**Requires:** `jq` for settings.json persistence (`sudo apt install jq`). Without it, shell env switching still works; settings.json is just not updated.

---

## Providers

| Alias | Provider | Endpoint |
|---|---|---|
| `kimi` | Kimi k2.6 | `api.kimi.com/coding/` |
| `glm` | GLM-5 | `open.bigmodel.cn/api/anthropic` |
| `minimax` | MiniMax M2.7 | `api.minimax.io/anthropic` |
| `zz` | Zhongzhuan relay | `cc1.zhihuiapi.top` |
| `cc` | Native Claude | claude.ai / OAuth |

---

## Adding a New Provider

Edit `ccswitch.sh` and add a function following the same pattern:

```bash
use_myprovider() {
    export ANTHROPIC_BASE_URL="https://api.example.com/anthropic"
    export ANTHROPIC_AUTH_TOKEN="$MY_API_KEY"
    unset ANTHROPIC_API_KEY
    export ANTHROPIC_MODEL="my-model-name"
    export API_TIMEOUT_MS="60000"
    export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1"
    _ccswitch_write_settings \
        "ANTHROPIC_BASE_URL=https://api.example.com/anthropic" \
        "ANTHROPIC_AUTH_TOKEN=$MY_API_KEY" \
        "ANTHROPIC_MODEL=my-model-name" \
        "API_TIMEOUT_MS=60000" \
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"
    echo "✅ Switch to MyProvider"
}
alias myprovider='use_myprovider'
```

Then add it to the `ccswitch use` and `ccswitch list` cases.

---

---

# ccswitch — Claude Code 模型提供商切换工具（中文说明）

轻量级 Shell 脚本，用于在多个 Claude Code 兼容提供商之间快速切换，参考 [cc-switch](https://github.com/farion1231/cc-switch) 桌面版的持久化策略实现。

---

## 快速上手

```bash
ccswitch list          # 列出所有可用提供商
ccswitch use kimi      # 切换到 Kimi k2.6
ccswitch use glm       # 切换到 GLM-5
ccswitch use minimax   # 切换到 MiniMax M2.7
ccswitch use zz        # 切换到中转节点
ccswitch use cc        # 切换回原生 Claude
ccswitch status        # 查看当前配置（shell 环境变量 + settings.json）
```

也可以直接用短别名：

```bash
kimi      # 切换 Kimi
glm       # 切换 GLM-5
minimax   # 切换 MiniMax
zz        # 切换中转
cc        # 切换回原生 Claude
model     # 查看当前模型（简略）
```

---

## 工作原理

切换提供商时，脚本同时执行两件事：

1. **导出 Shell 环境变量** — 立即在当前终端生效，无需重启。
2. **写入 `~/.claude/settings.json` 的 `.env` 块** — 持久化配置，新开终端或重启后 Claude Code 启动时自动读取。

这与 cc-switch 桌面版的工作方式一致，无需 GUI。

### 管理的环境变量

| 变量 | 作用 |
|---|---|
| `ANTHROPIC_BASE_URL` | 提供商 API 地址 |
| `ANTHROPIC_AUTH_TOKEN` | 提供商 API Key |
| `ANTHROPIC_MODEL` | 模型名称 |
| `API_TIMEOUT_MS` | 请求超时时间（毫秒）|
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | 中转节点场景下减少多余请求 |

### 原生 Claude 模式（`cc`）

会清除上述所有变量，同时从 `settings.json` 中删除对应字段，恢复 Claude Code 的默认 OAuth / API Key 行为。

---

## 安装方式

1. 将本仓库克隆或复制到 `~/workspace/ccswitch/`
2. 在 `~/.bashrc` 中加入一行：

```bash
source ~/workspace/ccswitch/ccswitch.sh
```

3. 重新加载：

```bash
source ~/.bashrc
```

**依赖：** `jq`（用于写入 settings.json）。未安装时 shell 环境变量切换仍正常工作，只是不会更新 settings.json。安装方法：`sudo apt install jq`

---

## 提供商列表

| 别名 | 提供商 | API 地址 |
|---|---|---|
| `kimi` | Kimi k2.6 | `api.kimi.com/coding/` |
| `glm` | GLM-5 | `open.bigmodel.cn/api/anthropic` |
| `minimax` | MiniMax M2.7 | `api.minimax.io/anthropic` |
| `zz` | 中转节点 | `cc1.zhihuiapi.top` |
| `cc` | 原生 Claude | claude.ai / OAuth |

---

## 添加新提供商

编辑 `ccswitch.sh`，按以下模板添加函数：

```bash
export MY_API_KEY="your-key-here"

use_myprovider() {
    export ANTHROPIC_BASE_URL="https://api.example.com/anthropic"
    export ANTHROPIC_AUTH_TOKEN="$MY_API_KEY"
    unset ANTHROPIC_API_KEY
    export ANTHROPIC_MODEL="my-model-name"
    export API_TIMEOUT_MS="60000"
    export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1"
    _ccswitch_write_settings \
        "ANTHROPIC_BASE_URL=https://api.example.com/anthropic" \
        "ANTHROPIC_AUTH_TOKEN=$MY_API_KEY" \
        "ANTHROPIC_MODEL=my-model-name" \
        "API_TIMEOUT_MS=60000" \
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1"
    echo "✅ Switch to MyProvider"
}
alias myprovider='use_myprovider'
```

然后在 `ccswitch()` 函数的 `use` 和 `list` 分支中补充对应条目即可。
