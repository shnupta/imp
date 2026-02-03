# Imp

A personal AI agent CLI built in Rust. Imp lives on your machine, remembers your projects, learns your preferences, and works alongside you in the terminal.

## What is Imp?

Imp is an AI agent that acts as a **coding partner** and **personal assistant**. It:

- Maintains persistent memory across sessions (daily notes + long-term knowledge)
- Uses tools to interact with your codebase — read, write, edit, search, run commands
- Spawns sub-agents for parallel background tasks
- Supports MCP servers for extensible tool integration (GitHub, databases, etc.)
- Understands your project context — git status, language, structure, conventions
- Reflects on interactions to learn and improve over time
- Calls Claude (Anthropic) for intelligent responses and task execution

## Installation

### Prerequisites

- **Rust** — install from [rustup.rs](https://rustup.rs/)
- **Authentication** — install [Claude Code CLI](https://claude.ai/code) and run `claude setup-token`
- **Optional**: `ripgrep` for faster code search

### Build from Source

```bash
git clone https://github.com/shnupta/imp.git
cd imp
cargo build --release
cargo install --path crates/imp-cli
```

## Quick Start

```bash
# First-time setup — creates your agent's identity, personality, and config
imp bootstrap

# Start an interactive chat
imp chat

# Ask a one-shot question
imp ask "What files are in this project?"

# Resume a previous session
imp chat --resume

# Continue the last session
imp chat --continue
```

## Authentication

Imp uses Anthropic tokens. Run `claude setup-token` to get yours.

```bash
imp bootstrap  # Configure during first-time setup
imp login      # Update authentication later
```

**Token types** (auto-detected from prefix):
- `sk-ant-oat*` — OAuth (Claude Pro/Max subscription)
- `sk-ant-api*` — API key (pay-per-token)

## Core Features

### Interactive Chat

```bash
imp chat
```

Full-featured terminal chat with:
- **Markdown rendering** via termimad
- **Input queue** — type while the agent is working, inputs are queued and processed in order
- **Session management** — resume previous sessions, session picker with titles
- **Multiline input** — backslash continuation (`line \`)
- **Commands**: `/help`, `/quit`, `/clear`, `/compact`, `/session`, `/agents`, `/queue`, `/cancel`

### Sub-Agents

Imp can spawn background sub-agents for parallel work. The agent decides when to delegate:

```
You: Refactor the auth module and update the tests

Imp: I'll handle the refactoring directly and spawn a sub-agent for the tests.
🚀 Sub-agent #1 spawned
[continues working on refactoring while tests are written in parallel]
```

Sub-agents get their own conversation context, tools, and token budget. Results are automatically summarized when they complete.

### Memory System

Imp maintains two layers of memory:

- **Daily notes** (`~/.imp/memory/YYYY-MM-DD.md`) — raw interaction logs
- **Long-term memory** (`~/.imp/MEMORY.md`) — curated knowledge, preferences, lessons

Run `imp reflect` to distill daily notes into long-term memory. This can also update `USER.md` (what Imp knows about you) and `SOUL.md` (the agent's evolving identity).

### Workspace Awareness

When you run Imp inside a project, it automatically detects:
- **Git context** — current branch, dirty/clean status, last commit
- **Language** — from Cargo.toml, package.json, go.mod, etc.
- **Config files** — Dockerfile, CI configs, linters, test frameworks
- **Project description** — from README
- **Directory structure** — available on-demand (depth 3, excludes noise)

### Built-in Tools

| Tool | Description |
|------|-------------|
| `exec` | Run shell commands |
| `file_read` | Read files with line numbers, optional offset/limit for large files |
| `file_edit` | Find-and-replace with exact match (rejects ambiguous multi-match edits) |
| `file_write` | Create or overwrite files (auto-creates parent dirs) |
| `search_code` | Search all files with ripgrep (optional file type filter) |
| `list_files` | List directory contents |
| `spawn_agent` | Spawn a background sub-agent for parallel work |
| `check_agents` | Check status of running sub-agents |

### MCP Support

Extend Imp with any [MCP server](https://modelcontextprotocol.io/) — no code changes needed. Drop a TOML file in `~/.imp/mcp/`:

```toml
# ~/.imp/mcp/github.toml
[server]
name = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[server.env]
GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}"
```

MCP tools appear alongside built-in tools seamlessly. Environment variables support `${VAR}` expansion.

### Custom Tools

Add shell-based tools via TOML files in `~/.imp/tools/`:

```toml
[tool]
name = "git_status"
description = "Get the current git status"

[handler]
kind = "shell"
command = "git status --porcelain"
```

## Context System

Imp uses a tiered context system to stay lean:

**L1 — Always in system prompt** (kept small):
- `SOUL.md` — agent identity and personality
- `USER.md` — about you (preferences, work context)
- Project summary (language, git status, config files)

**L2 — Available on demand** (agent loads when relevant):
- `MEMORY.md` — long-term memory
- `memory/YYYY-MM-DD.md` — daily notes
- Project context, patterns, history files
- Directory structure snapshot
- Git log and diff info

**L3 — Cold storage**:
- SQLite database with full conversation history

## Configuration

Config lives at `~/.imp/config.toml`:

```toml
[llm]
provider = "anthropic"
model = "claude-opus-4-5-20251101"
max_tokens = 16384

[auth]
method = "oauth"  # or "api_key"

[auth.oauth]
access_token = "sk-ant-oat..."

[thinking]
enabled = false  # Extended thinking (Sonnet 4+ only)
```

### Key Directories

```
~/.imp/
├── config.toml          # Main configuration
├── SOUL.md              # Agent identity & personality
├── USER.md              # About you
├── MEMORY.md            # Long-term memory
├── memory/              # Daily notes (YYYY-MM-DD.md)
├── mcp/                 # MCP server configs (*.toml)
├── tools/               # Custom tool definitions (*.toml)
├── projects/            # Per-project context
│   └── <project-name>/
│       ├── CONTEXT.md
│       ├── PATTERNS.md
│       ├── HISTORY.md
│       └── memory/
└── imp.db               # SQLite conversation history
```

## Commands

| Command | Description |
|---------|-------------|
| `imp bootstrap` | First-time setup wizard |
| `imp chat` | Interactive chat session |
| `imp chat --resume` | Pick a previous session to resume |
| `imp chat --continue` | Continue the last session |
| `imp chat --session <id>` | Resume a specific session |
| `imp ask "<question>"` | One-shot question |
| `imp reflect [--date YYYY-MM-DD]` | Reflect on a day's interactions |
| `imp login` | Update authentication |
| `imp project list` | List registered projects |
| `imp learn` | Interactive learning session |

## Token Usage & Cost

Imp tracks token usage per session with model-aware pricing:
- Displays per-request and session totals
- Tracks prompt caching (cache read/write tokens)
- Supports pricing for Opus 4.5, Opus 4, Sonnet, and Haiku

## Roadmap

See [ROADMAP.md](ROADMAP.md) for planned features including:
- Smart memory with embeddings and vector search
- Daemon mode with background tasks and proactivity
- Channel integrations (Telegram, Slack)
- Web UI
- Automatic pattern extraction and adaptive behavior

## License

MIT
