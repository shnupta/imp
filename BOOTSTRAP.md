# Imp - Getting Started

Imp is your AI coding assistant that learns about your project through context files and helps you with development tasks.

## What You Need

1. **Rust toolchain** - Install from https://rustup.rs/
2. **Anthropic API key** - Get one at https://console.anthropic.com/

## Build Imp

```bash
# Clone this repository
git clone <your-repo-url>
cd imp

# Build the binary
cargo build --release

# The binary will be at target/release/imp
# Install it to your PATH for easy access:
# cargo install --path .
```

## First-Time Setup

Run the setup wizard:

```bash
imp init
```

This interactive wizard will:

1. **Name your agent** - Give it a personality (e.g., "Ada", "Rust", "Helper")
2. **API key** - Paste your Anthropic API key (keeps it secure in `~/.imp/config.toml`)
3. **Model selection** - Choose between Claude models
4. **Workspace** - Tell it where your code repositories live

After setup, you'll have:
- `~/.imp/config.toml` - Your personal config (never committed to git)
- `context/IDENTITY.md` - Your agent's personality
- `context/STACK.md` - Template for your tech stack info
- `context/PRINCIPLES.md` - Template for coding standards
- `context/ARCHITECTURE.md` - Template for system architecture

## Quick Start

```bash
# Ask a quick question
imp ask "What programming languages are used in this project?"

# Start an interactive chat
imp chat
```

## Customize Your Agent

Edit the files in `context/` to teach your agent about your project:

### `context/IDENTITY.md`
Your agent's personality and role. Already created by `imp init`.

### `context/STACK.md`
```markdown
# Technology Stack

## Languages
- Rust - Primary language
- TypeScript - Frontend
- Python - Scripts and data processing

## Frameworks
- Axum - Web server
- React - Frontend
- SQLite - Database

## Tools
- Cargo - Package manager
- Jest - Testing
- GitHub Actions - CI/CD
```

### `context/PRINCIPLES.md`
```markdown
# Coding Principles

## Code Style
- Use `rustfmt` for formatting
- Prefer explicit error handling with `Result<T, E>`
- Write doc comments for public APIs

## Testing
- Unit tests for all business logic
- Integration tests for API endpoints
- Use descriptive test names

## Pull Requests
- Squash commits before merging
- Require one reviewer
- All tests must pass
```

### `context/ARCHITECTURE.md`
```markdown
# Architecture Overview

## System Overview
Web API built with Rust/Axum, React frontend, SQLite database.

## Services
- `api/` - REST API server
- `web/` - React frontend
- `shared/` - Common types and utilities

## Database Schema
- `users` - User accounts
- `projects` - User projects
- `tasks` - Project tasks
```

## What Your Agent Can Do

**Built-in tools:**
- Execute shell commands
- Read/write files
- Search code with ripgrep
- List directory contents
- Edit files (find/replace)

**Example interactions:**
- "Show me the main function in src/main.rs"
- "What dependencies does this project use?"
- "Create a new Rust module for user authentication"
- "Run the tests and show me any failures"
- "Find all TODO comments in the codebase"

## Tips

1. **Be specific** - "Fix the login function" vs "There's a bug in the authentication"
2. **Provide context** - Show error messages, file contents, or current state
3. **Use tools** - Your agent can read files, run commands, and search code
4. **Iterate** - Have a conversation, build up understanding over multiple turns

## Common Commands

```bash
# One-shot tasks
imp ask "Create a .gitignore file for this Rust project"
imp ask "What's the current git status?"
imp ask "Show me the main dependencies in Cargo.toml"

# Interactive mode
imp chat
> Can you explain this error? [paste error]
> How should I structure a new module?
> Write a test for the validate_email function
> clear  # Clear conversation history
> reload # Reload context files
> quit   # Exit
```

## Troubleshooting

**"Config file not found"**
- Run `imp init` first

**"API error"**
- Check your API key in `~/.imp/config.toml`
- Verify you have credits in your Anthropic account

**"Tool not found"**
- Make sure `rg` (ripgrep) is installed for code search
- Other tools use standard Unix utilities

**Agent seems confused about project**
- Update your context files in `context/`
- Run `reload` in chat mode to pick up changes

You're ready to start! Your agent will learn about your project as you work together.