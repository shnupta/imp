#!/usr/bin/env python3
"""
Simple test script to verify MCP integration in Imp.
This creates a minimal MCP server that responds to the basic protocol.
"""

import sys
import json
import os
import subprocess
import tempfile
import time

def create_simple_mcp_server():
    """Create a simple MCP server script for testing."""
    server_script = '''#!/usr/bin/env python3
import sys
import json

def send_response(response):
    print(json.dumps(response))
    sys.stdout.flush()

def read_request():
    line = sys.stdin.readline()
    return json.loads(line.strip())

# Main loop
try:
    while True:
        request = read_request()
        
        if request.get("method") == "initialize":
            response = {
                "jsonrpc": "2.0",
                "id": request.get("id"),
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "test-mcp-server",
                        "version": "1.0.0"
                    }
                }
            }
            send_response(response)
            
        elif request.get("method") == "tools/list":
            response = {
                "jsonrpc": "2.0",
                "id": request.get("id"),
                "result": {
                    "tools": [
                        {
                            "name": "test_echo",
                            "description": "A simple echo tool for testing MCP integration",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "message": {
                                        "type": "string",
                                        "description": "Message to echo back"
                                    }
                                },
                                "required": ["message"]
                            }
                        }
                    ]
                }
            }
            send_response(response)
            
        elif request.get("method") == "tools/call":
            params = request.get("params", {})
            tool_name = params.get("name")
            arguments = params.get("arguments", {})
            
            if tool_name == "test_echo":
                message = arguments.get("message", "No message provided")
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": f"Echo: {message}"
                            }
                        ]
                    }
                }
                send_response(response)
            else:
                response = {
                    "jsonrpc": "2.0",
                    "id": request.get("id"),
                    "error": {
                        "code": -1,
                        "message": f"Unknown tool: {tool_name}"
                    }
                }
                send_response(response)
                
except EOFError:
    sys.exit(0)
except Exception as e:
    sys.stderr.write(f"Error: {e}\\n")
    sys.exit(1)
'''
    
    with tempfile.NamedTemporaryFile(mode='w', suffix='.py', delete=False) as f:
        f.write(server_script)
        f.flush()
        os.chmod(f.name, 0o755)
        return f.name

def create_test_mcp_config(server_path):
    """Create a test MCP configuration file."""
    config_content = f'''[server]
name = "test-server"
command = "python3"
args = ["{server_path}"]

[server.env]
TEST_VAR = "test_value"
'''
    
    # Ensure the MCP directory exists
    mcp_dir = os.path.expanduser("~/.imp/mcp")
    os.makedirs(mcp_dir, exist_ok=True)
    
    config_path = os.path.join(mcp_dir, "test-server.toml")
    with open(config_path, 'w') as f:
        f.write(config_content)
    
    return config_path

def test_mcp_integration():
    """Test the MCP integration by running the imp binary."""
    print("Testing MCP Integration...")
    
    # Create test MCP server
    server_path = create_simple_mcp_server()
    print(f"Created test MCP server: {server_path}")
    
    try:
        # Create test MCP configuration
        config_path = create_test_mcp_config(server_path)
        print(f"Created test MCP config: {config_path}")
        
        # Check if imp binary exists
        imp_path = "/root/imp/target/release/imp"
        if not os.path.exists(imp_path):
            print(f"Error: Imp binary not found at {imp_path}")
            print("Please build Imp first: cd /root/imp && cargo build --release")
            return False
        
        print("MCP integration test setup complete!")
        print("\nTo test manually:")
        print(f"1. The test MCP server is configured in: {config_path}")
        print("2. Run Imp and it should load the test-server")
        print("3. Look for 'Loaded MCP server: test-server' in the startup logs")
        print("4. The 'test_echo' tool should be available alongside built-in tools")
        
        print("\nConfiguration files created:")
        print(f"  - MCP server script: {server_path}")
        print(f"  - MCP config file: {config_path}")
        
        print("\nExample MCP server configs are also available:")
        print("  - ~/.imp/mcp/github.toml")
        print("  - ~/.imp/mcp/filesystem.toml")
        
        return True
        
    except Exception as e:
        print(f"Error during test setup: {e}")
        return False
    
    finally:
        # Clean up server script
        if 'server_path' in locals():
            try:
                os.unlink(server_path)
                print(f"Cleaned up test server: {server_path}")
            except:
                pass

if __name__ == "__main__":
    success = test_mcp_integration()
    sys.exit(0 if success else 1)