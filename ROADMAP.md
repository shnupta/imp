# Imp Roadmap

## Phase 1: Foundation Improvements (Current)

### Smart Memory & Retrieval
- Embeddings for conversation history and memory files (vector search)
- Semantic search over past sessions ("remember when we discussed X")
- Index extracted insights properly, not just raw daily logs
- Consider SQLite + vector extension (sqlite-vec) or a lightweight embedded store
- Goal: institutional knowledge that survives across sessions

### MCP Support (In Progress)
- Config-driven MCP server integration — TOML files in `~/.imp/mcp/`
- Stdio transport, JSON-RPC protocol, auto-discovery of tools at startup
- No code changes to add new MCP servers
- Enables: GitHub, Slack, databases, web browsing, anything with an MCP server
- Key unlock: stops Imp being "filesystem-only" and makes it workflow-integrated

### Workspace Awareness (In Progress)
- Git context in system prompt (branch, dirty/clean, last commit)
- Expanded project detection (language, key config files, README)
- Directory structure snapshot (tree-style, L2 on-demand)
- Goal: agent knows what you're working on without asking every time

### Crate Refactoring
- Pull reusable components out of `imp-cli` into separate crates:
  - `imp-core`: client, message types, tool system, compaction
  - `imp-memory`: memory management, embeddings, retrieval
  - `imp-tools`: tool registry, MCP client, built-in tools
- Foundation for daemon, web UI, and library use
- Should happen before daemon work to avoid massive refactor later

---

## Phase 2: Proactivity & Daemon

### Daemon Mode
- Long-running background process (`imp daemon`)
- Watches for events: file changes, git pushes, CI results, cron triggers
- Can initiate conversations or actions without being invoked
- IPC for the CLI to communicate with the running daemon
- Depends on: crate refactoring (core logic must be reusable)

### Background Tasks & Watchers
- File watchers: detect changes in project, trigger analysis
- Git hooks: post-commit, post-push — auto-review, run checks
- CI integration: watch pipeline status, alert on failures
- Calendar/reminder integration (beyond basic cron)
- Goal: Imp goes from "tool you invoke" to "teammate who's paying attention"

### Channel Integrations
- Telegram, Slack, Discord — chat with Imp from anywhere
- Each channel is an input/output surface connected to the daemon
- Supports notifications (CI failed, reminder, proactive insight)
- Could leverage MCP for some integrations, or native adapters
- Depends on: daemon

### Web UI
- Browser-based chat interface (alternative to terminal)
- Real-time streaming responses
- Session management, project switching
- Depends on: daemon (serves the web UI), crate refactoring

---

## Phase 3: Self-Learning & Evolution

### Automatic Pattern Extraction
- Move beyond manual note-taking in reflect
- Detect behavioral patterns: "Casey always wants tests with new code"
- Extract project conventions: "this codebase uses this error handling pattern"
- Learn from mistakes: "last time we tried X it didn't work because Y"
- Store patterns as structured data, not just prose in MEMORY.md

### Adaptive Behavior
- Patterns should actually change agent behavior, not just be notes
- Example: if the agent learns Casey prefers concise responses, the system prompt should adapt
- Project-specific learned conventions should influence code generation
- Feedback loop: agent tries something → outcome → update pattern → try differently next time

### Improved Reflect
- Batch multiple days of reflection
- Detect sparse/empty daily notes and skip
- Auto-trigger after long chat sessions (not just cron)
- Cross-reference with project history (git log) for richer context
- Confidence scoring: distinguish firm knowledge from tentative observations

### Sub-Agent Improvements
- Shared context: parent can pass relevant file contents to sub-agents (don't re-read)
- Specialized sub-agents: code reviewer, test writer, documentation updater
- Sub-agent chaining: output of one feeds into another
- Better budget estimation based on task complexity
- Sub-agent learning: track which tasks sub-agents succeed/fail at

---

## Principles
- **Config over code**: new integrations (MCP, tools) should never require code changes
- **Lean system prompts**: every token in L1 costs money on every turn. Be aggressive about what's always-loaded vs on-demand.
- **Graceful degradation**: missing tools, failed MCP servers, no git — should never crash, just skip
- **Memory is everything**: an agent without memory is just a chatbot. Invest heavily here.
- **Presence over sessions**: the long-term goal is continuous awareness, not isolated conversations
