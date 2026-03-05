#!/usr/bin/env bash
# End-to-end test for Phase 3: Sync Engine, links, new tools
set -euo pipefail

BINARY="./target/release/second-brain"
NOTES_DIR="./test-notes"
NEW_NOTE="./test-notes/e2e-created-note.md"

# Clean up any previous test artifacts
rm -f "$NEW_NOTE"

send_and_recv() {
    local msg="$1"
    local wait="${2:-2}"
    echo "$msg"
    sleep "$wait"
}

{
    # Initialize
    send_and_recv '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    send_and_recv '{"jsonrpc":"2.0","method":"notifications/initialized"}'

    # Ingest test notes (includes link-bearing notes)
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"note_ingest\",\"arguments\":{\"path\":\"$NOTES_DIR\"}}}" 10

    # Query link graph for project-overview.md (should have outbound links)
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"note_graph\",\"arguments\":{\"file_path\":\"$NOTES_DIR/project-overview.md\"}}}" 2

    # Create a new note via MCP tool
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"note_create\",\"arguments\":{\"file_path\":\"$NEW_NOTE\",\"content\":\"# E2E Test Note\\n\\nThis note was created via the note_create MCP tool.\\nSee also: [[project-overview]]\\n\"}}}" 5

    # Get link graph for the newly created note
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"note_graph\",\"arguments\":{\"file_path\":\"$NEW_NOTE\"}}}" 2

    # Update the note
    send_and_recv "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"note_update\",\"arguments\":{\"file_path\":\"$NEW_NOTE\",\"content\":\"# E2E Test Note (Updated)\\n\\nThis note was updated via note_update.\\nLinks: [[daily-log]] and [[mcp-architecture]]\\n\"}}}" 5

    # List tools to verify all 10 are registered
    send_and_recv '{"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}' 2

} | timeout 60 "$BINARY" 2>/dev/null | while IFS= read -r line; do
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
        print(text[:800])
        print()
    elif 'tools' in result:
        names = [t['name'] for t in result['tools']]
        print(f'--- Response {rid}: Tools ({len(names)}) ---')
        print(', '.join(sorted(names)))
        print()
    elif 'serverInfo' in result:
        print(f'--- Response {rid}: Init OK ---')
        print(f'Server: {result[\"serverInfo\"][\"name\"]} v{result[\"serverInfo\"][\"version\"]}')
        print()
except:
    pass
" 2>/dev/null
done

# Clean up
rm -f "$NEW_NOTE"
echo "--- Test complete ---"
