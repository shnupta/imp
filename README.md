# Imp

A Rust-based AI agent CLI for engineering teams. Each team member gets their own agent with a unique identity, personalized context, and shared team knowledge.

## What is Imp?

Imp is an AI agent that acts as both a **coding partner** and **work organizer**. It:

- Loads context from markdown files to understand your project and team
- Uses tools to interact with your codebase (read/write files, run commands, search code)
- Calls Claude (Anthropic) for intelligent responses and task execution
- Provides both one-shot commands and interactive chat sessions

## Installation

### Prerequisites

- Rust (install from [rustup.rs](https://rustup.rs/))
- **Authentication**:
  - Install Claude Code CLI from [claude.ai/code](https://claude.ai/code)
  - Run `claude setup-token` to get your authentication token
- Optional: `ripgrep` for better code search (install via your package manager or [GitHub](https://github.com/BurntSushi/ripgrep))

**System Requirements:**
- Unix-like OS (Linux, macOS) or Windows with WSL
- Git (for project detection)
- Standard shell utilities (ls, grep, etc.)

### Build and Install from source

```bash
git clone <your-repo-url>
cd imp
cargo build --release

# Install to your PATH for system-wide access
cargo install --path .
```

After installation, `imp` will be available from anywhere in your terminal.

## Authentication

Imp uses Anthropic tokens for authentication. The setup is simple:

### Get Your Token
1. **Install Claude Code CLI**: Visit [claude.ai/code](https://claude.ai/code) and install the CLI
2. **Get your token**: Run `claude setup-token` in your terminal
3. **Copy the token**: The command will output a token starting with `sk-ant-`

### Configure Imp
```bash
imp bootstrap  # Paste your token during setup
# or
imp login      # Update authentication later
```

### Token Types (Auto-detected)
- **OAuth tokens** (`sk-ant-oat*`): Use your Claude Pro/Max subscription
- **API keys** (`sk-ant-api*`): Pay-per-token usage

Imp automatically detects which type you have and configures the appropriate authentication headers.

## First-time Setup

Run the bootstrap wizard to configure your agent:

```bash
imp bootstrap
```

This will:
1. **Choose authentication method**: OAuth (Claude Pro/Max) or API Key
2. **Agent identity**: Name your agent and set its personality  
3. **User information**: Tell your agent about yourself and work style
4. **Context setup**: Create template files for project understanding
5. **Optional engineering context**: Tech stack, principles, and architecture files

After bootstrap, you can switch authentication methods anytime with `imp login`.

## Usage

### Quick commands

```bash
# Ask a one-time question
imp ask "What files are in this directory?"

# Start an interactive chat session
imp chat

# Switch to OAuth authentication
imp login
```

### Context Files

Imp uses markdown files in the `context/` directory to understand your project:

- **`context/IDENTITY.md`** — Your agent's name and personality (created by `imp bootstrap`)
- **`context/STACK.md`** — Technology stack and tools your team uses
- **`context/PRINCIPLES.md`** — Coding standards and team practices
- **`context/ARCHITECTURE.md`** — System architecture and design decisions

Edit these files to customize how your agent behaves and what it knows about your project.

### Built-in Tools

Imp ships with these tools that Claude can use:

- **`exec`** — Run shell commands
- **`file_read`** — Read file contents
- **`file_write`** — Create or overwrite files
- **`file_edit`** — Find and replace text in files
- **`search_code`** — Search code using ripgrep
- **`list_files`** — List directory contents

### Adding Custom Tools

Create TOML files in the `tools/` directory to add new capabilities:

```toml
# tools/git-status.toml
[tool]
name = "git_status"
description = "Get the current git status"

[tool.parameters]
# No parameters needed

[handler]
kind = "shell"
command = "git status --porcelain"
```

## Configuration

Config is stored at `~/.imp/config.toml`. The format depends on your token type:

### OAuth Configuration (Claude Pro/Max)
```toml
[llm]
provider = "anthropic"
model = "claude-opus-4-5-20251101"

[auth]
method = "oauth"

[auth.oauth]
access_token = "sk-ant-oat..."  # Your setup-token
refresh_token = ""
expires_at = 1234567890
```

### API Key Configuration (Pay-per-token)
```toml
[llm]
provider = "anthropic" 
model = "claude-opus-4-5-20251101"

[auth]
method = "api_key"

[auth.api_key]
key = "sk-ant-api..."  # Your setup-token
```

**Note**: 
- Setup-tokens from `claude setup-token` are long-lived and don't need refresh
- Token type is automatically detected from the `sk-ant-` prefix
- Both use the same `claude setup-token` command - the type depends on your Claude account

## Examples

```bash
# One-shot tasks
imp ask "Create a README for this project"
imp ask "What's wrong with this error?" # (paste error in follow-up)

# Interactive session
imp chat
> What does this function do? # (then show file contents)
> How can I optimize this code?
> Write tests for the user authentication module
> quit
```

## Contributing

This is Phase 1 — a working foundation. Planned features:

- Web UI for chat interface
- Cron jobs and scheduling
- Team integration (Slack, GitHub)
- Semantic code search
- Advanced context management

## License

[Your license here]