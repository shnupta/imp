import sqlite3
import requests
import json

# This would normally be done through the agent, but let's test the store_chunk functionality
# by directly calling the Rust binary with some test knowledge

print("Testing knowledge graph functionality...")

# Test that the binary works
import subprocess
result = subprocess.run(['./target/debug/imp', 'knowledge', 'stats'], 
                       capture_output=True, text=True, cwd='/root/imp')
print("Stats output:", result.stdout)
