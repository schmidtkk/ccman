# ccswitch -- Claude Code Provider Switcher

A fast Rust-based provider switcher for Claude Code, inspired by [cc-switch](https://github.com/farion1231/cc-switch).

Supports switching between multiple API providers (Kimi, GLM-5, MiniMax, Zhongzhuan relay, native Claude, and custom) with changes persisted to `~/.claude/settings.json` so they survive terminal restarts. Includes a terminal UI, API key rotation, health checks, and usage tracking with cost calculation.

---

## Quick Start

```bash
ccswitch list          # list all providers
ccswitch use kimi      # switch to Kimi k2.6
ccswitch use glm       # switch to GLM-5
ccswitch use minimax   # switch to MiniMax M2.7
ccswitch use zz        # switch to Zhongzhuan relay
ccswitch use cc        # switch back to native Claude
ccswitch status        # show current provider
ccswitch tui           # interactive terminal UI
```

Short aliases (from the shell wrapper):

```bash
kimi      # ccswitch use kimi
glm       # ccswitch use glm
minimax   # ccswitch use minimax
zz        # ccswitch use zz
cc        # ccswitch use cc
model     # ccswitch status
```

---

## How It Works

When you switch providers, ccswitch does two things simultaneously:

1. **Exports shell environment variables** -- takes effect immediately in the current terminal session.
2. **Writes to `~/.claude/settings.json` `.env` block** -- persists across sessions; Claude Code reads this on startup.

### Environment variables managed

| Variable | Purpose |
|---|---|
| `ANTHROPIC_BASE_URL` | API endpoint for the provider |
| `ANTHROPIC_AUTH_TOKEN` | Provider API key |
| `ANTHROPIC_MODEL` | Model name/ID |
| `API_TIMEOUT_MS` | Request timeout (ms) |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Reduces extra traffic for relay providers |

### Native Claude mode (`cc`)

Clears all managed env vars from both the shell and `settings.json`, restoring Claude Code's default OAuth/API key behavior.

---

## Installation

### Prerequisites

- Rust toolchain (for building the binary)
- Bash (the shell wrapper is bash-specific)

### Build & Install

```bash
cd ~/workspace/ccswitch
cargo build --release
```

Then add this line to your `~/.bashrc`:

```bash
source ~/workspace/ccswitch/ccswitch.sh
```

### API Keys

Copy `.env.example` to `.env` and fill in your API keys:

```bash
cp .env.example .env
# Edit .env with your keys
```

The `.env` file is gitignored and never committed.

---

## Commands

### Provider switching

```bash
ccswitch list              # list all configured providers
ccswitch use <provider>    # switch to a provider
ccswitch status            # show current provider and env
```

### API key management

```bash
ccswitch key list <provider>                                  # list keys for a provider
ccswitch key add <provider> <key> --label "my-key" --priority 1
ccswitch key remove <key-id>                                  # remove a key by ID
```

### Usage tracking

```bash
ccswitch usage today        # today's usage summary
ccswitch usage month        # this month's usage
ccswitch usage total        # all-time usage
ccswitch usage logs         # recent usage logs
```

### Health checks

```bash
ccswitch health             # check all providers
ccswitch health <provider>  # check a specific provider
```

### Provider management (dynamic CRUD)

```bash
ccswitch provider add <name> <display-name> <base-url> [--model ...] [--auth-header ...]
ccswitch provider edit <name> [--display-name ...] [--base-url ...]
ccswitch provider remove <name>
```

### TUI

```bash
ccswitch tui                # interactive terminal UI with 4 tabs
```

The TUI has tabs for Providers, Keys, Usage, and Health. Navigate with Tab/vi-style keys (j/k/h/l), mouse clicks, or Enter.

---

## Providers

| Alias | Provider | Endpoint |
|---|---|---|
| `kimi` | Kimi k2.6 | `api.kimi.com/coding/` |
| `glm` | GLM-5 | `open.bigmodel.cn/api/anthropic` |
| `minimax` | MiniMax M2.7 | `api.minimax.io/anthropic` |
| `zz` | Zhongzhuan relay | `cc1.zhihuiapi.top` |
| `cc` | Native Claude | claude.ai / OAuth |

Providers are stored in a local SQLite database and can be added/edited/removed dynamically via `ccswitch provider` commands or the TUI.

---

## Architecture

```
ccswitch/           # Binary crate: CLI (clap) + TUI (ratatui)
ccswitch-core/      # Core library: provider switching, settings, health, usage
ccswitch-db/        # Database library: SQLite models, repositories, migrations
ccswitch.sh         # Shell wrapper: loads .env, delegates to binary, eval's exports
migrations/         # SQL migration files
```
