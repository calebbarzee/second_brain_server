#!/usr/bin/env bash
# End-to-end test for the MCP server with semantic search
set -euo pipefail

BINARY="./target/release/second-brain"
NOTES_DIR="./test-notes"

send_and_recv() {
    local msg="$1"
    local wait="${2:-2}"
    echo "$msg"
    sleep "$wait"
}

{
    send_and_recv '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    send_and_recv '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"note_ingest\",\"arguments\":{\"path\":\"$NOTES_DIR\"}}}" 8
    send_and_recv '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"semantic_search","arguments":{"query":"how does memory management work in programming languages"}}}' 3
    send_and_recv '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"semantic_search","arguments":{"query":"cooking food recipes"}}}' 3
    send_and_recv '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"tools/list","arguments":{}}}' 2
} | timeout 40 "$BINARY" 2>/dev/null | while IFS= read -r line; do
    echo "$line" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    rid = d.get('id', '?')
    result = d.get('result', {})
    content = result.get('content', [])
    if content:
        text = content[0].get('text', '')
        print(f'--- Response {rid} ---')
        print(text[:500])
        print()
    elif 'tools' in result:
        names = [t['name'] for t in result['tools']]
        print(f'--- Response {rid}: Tools ---')
        print(', '.join(names))
        print()
    elif 'serverInfo' in result:
        print(f'--- Response {rid}: Init OK ---')
        print(f'Server: {result[\"serverInfo\"][\"name\"]} v{result[\"serverInfo\"][\"version\"]}')
        print()
except:
    pass
" 2>/dev/null
done
