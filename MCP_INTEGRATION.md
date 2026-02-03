# MCP (Model Context Protocol) Integration

Imp now supports MCP (Model Context Protocol) servers, allowing you to extend Imp's capabilities with external tools without modifying the codebase.

## Overview

MCP integration allows you to:
- Add external tools by simply dropping configuration files in `~/.imp/mcp/`
- Use tools from the MCP ecosystem (GitHub, filesystem, databases, etc.)
- No code changes required to add new MCP servers
- MCP tools work seamlessly alongside built-in tools
- Support for both main agents and sub-agents

## Configuration

### Directory Structure
```
~/.imp/
├── mcp/
│   ├── github.toml
│   ├── filesystem.toml
│   └── postgres.toml
└── tools/
    └── custom_tools.toml
```

### MCP Server Configuration Format

Each MCP server needs its own TOML file in `~/.imp/mcp/`:

```toml
[server]
name = "github"                           # Unique server name
command = "npx"                          # Command to run the server
args = ["-y", "@modelcontextprotocol/server-github"]  # Optional arguments

[server.env]                            # Optional environment variables
GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}"     # Supports ${VAR} expansion
API_KEY = "hardcoded-value"             # Or hardcoded values
```

## Example Configurations

### GitHub MCP Server
**File:** `~/.imp/mcp/github.toml`
```toml
[server]
name = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[server.env]
GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}"
```

### Filesystem MCP Server
**File:** `~/.imp/mcp/filesystem.toml`
```toml
[server]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

# No environment variables needed
```

### PostgreSQL MCP Server
**File:** `~/.imp/mcp/postgres.toml`
```toml
[server]
name = "postgres"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres"]

[server.env]
DATABASE_URL = "${DATABASE_URL}"
```

### Custom Python MCP Server
**File:** `~/.imp/mcp/custom.toml`
```toml
[server]
name = "custom"
command = "python3"
args = ["/path/to/my_mcp_server.py"]

[server.env]
CUSTOM_CONFIG = "${HOME}/.config/custom.json"
LOG_LEVEL = "INFO"
```

## How It Works

1. **Startup**: Imp scans `~/.imp/mcp/` for TOML configuration files
2. **Discovery**: Each MCP server is spawned as needed and queried for available tools
3. **Registration**: MCP tools are registered alongside built-in tools in the tool registry
4. **Execution**: When an MCP tool is called, Imp routes the request to the correct server
5. **Communication**: Uses JSON-RPC over stdin/stdout to communicate with MCP servers

## MCP Protocol Support

Imp implements the MCP protocol with these features:
- **Transport**: stdio (subprocess communication)
- **Protocol Version**: 2024-11-05
- **Methods Supported**:
  - `initialize` - Protocol handshake
  - `tools/list` - Discover available tools
  - `tools/call` - Execute tools

## Environment Variable Expansion

MCP configurations support `${VARIABLE}` syntax for environment variables:
- `"${GITHUB_TOKEN}"` - Expands to the value of `GITHUB_TOKEN` env var
- `"${HOME}/.config"` - Expands to your home directory path
- `"prefix_${VAR}_suffix"` - Supports interpolation within strings
- Missing variables expand to empty strings

## Tool Schema Conversion

MCP tool schemas are automatically converted to Anthropic's format:
- **MCP Format**: `{ name, description, inputSchema }`  
- **Anthropic Format**: `{ name, description, input_schema }`
- JSON Schema properties are preserved exactly
- Required parameters are maintained

## Sub-Agent Support

Sub-agents also have access to MCP tools:
- MCP servers are loaded when sub-agents start
- Same configuration directory (`~/.imp/mcp/`) is used
- Each sub-agent gets its own connection to MCP servers
- Tools work the same way as in the main agent

## Error Handling

The implementation handles various error scenarios gracefully:
- **Server fails to start**: Warning logged, other servers continue loading
- **Tool call fails**: Error returned to agent, execution continues
- **Invalid configuration**: Warning logged, file skipped
- **Missing dependencies**: Server marked as unavailable

## Performance Considerations

- **Lazy loading**: MCP servers are started only when first tool is called
- **Persistent connections**: Servers stay alive for the session duration
- **Concurrent access**: Thread-safe implementation allows parallel tool calls
- **Resource cleanup**: Servers are terminated when Imp exits

## Troubleshooting

### Common Issues

1. **MCP server not found**
   ```
   Warning: Failed to connect to MCP server 'github': Failed to spawn MCP server 'github': No such file or directory
   ```
   - Install the MCP server: `npm install -g @modelcontextprotocol/server-github`
   - Check the command path in your configuration

2. **Environment variable not set**
   ```
   MCP server error: Authentication required
   ```
   - Set required environment variables: `export GITHUB_TOKEN=your_token`
   - Check variable names in your configuration

3. **Tool not available**
   ```
   Unknown tool: github_search
   ```
   - Verify the MCP server is running: check startup logs
   - Confirm the tool name matches what the server provides

### Debug Tips

- Check Imp's startup logs for MCP server loading messages
- Verify your MCP server works standalone: `npx @modelcontextprotocol/server-github`
- Test environment variable expansion: `echo $GITHUB_TOKEN`
- Ensure TOML syntax is valid: use a TOML validator

## Available MCP Servers

Popular MCP servers from the ecosystem:
- **@modelcontextprotocol/server-github** - GitHub API integration
- **@modelcontextprotocol/server-filesystem** - File system operations
- **@modelcontextprotocol/server-postgres** - PostgreSQL database access
- **@modelcontextprotocol/server-sqlite** - SQLite database operations
- **@modelcontextprotocol/server-fetch** - HTTP request capabilities
- **@modelcontextprotocol/server-brave-search** - Brave Search API

## Example Usage

Once configured, MCP tools work transparently:

```
User: Search for recent issues in the imp repository
Agent: I'll search the GitHub repository for recent issues.
[Uses github_search_issues tool from MCP server]

User: Read the contents of /tmp/data.json
Agent: I'll read that file for you.
[Uses read_file tool from filesystem MCP server]
```

## Future Enhancements

Planned improvements:
- Support for other MCP transports (HTTP, WebSocket)
- Tool usage analytics and performance monitoring
- Configuration validation and better error messages
- Hot-reloading of MCP server configurations
- Built-in MCP server management commands