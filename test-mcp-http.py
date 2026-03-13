#!/usr/bin/env python3
"""
Second Brain — MCP HTTP Integration Test Suite

Tests every MCP tool over the Streamable HTTP transport, plus protocol edge
cases, error handling, and injection resistance.

Requires the full stack to be running:
    docker compose -f docker-compose.prod.yml -p secondbrain --env-file .env.prod up -d

Usage:
    ./test-mcp-http.py                          # Run all tests
    ./test-mcp-http.py --base-url http://h:8080 # Custom endpoint
    ./test-mcp-http.py --quick                  # Skip slow tests (embed, semantic)
    ./test-mcp-http.py -k search                # Only tests matching "search"
    ./test-mcp-http.py -v                       # Verbose — show result text for every test
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from typing import Any

import requests

# ── Colours ───────────────────────────────────────────────────────────────────

RED = "\033[0;31m"
GREEN = "\033[0;32m"
YELLOW = "\033[1;33m"
CYAN = "\033[0;36m"
BOLD = "\033[1m"
DIM = "\033[2m"
NC = "\033[0m"


# ── MCP Client ────────────────────────────────────────────────────────────────


@dataclass
class ToolResult:
    """Parsed result from an MCP tools/call response."""

    raw: dict[str, Any] = field(default_factory=dict)
    text: str = ""
    is_error: bool = False


class McpClient:
    """Minimal MCP Streamable-HTTP client for testing."""

    def __init__(self, base_url: str, timeout: int = 60):
        self.mcp_url = f"{base_url.rstrip('/')}/mcp"
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.session_id: str | None = None
        self._req_id = 0

    def _next_id(self) -> int:
        self._req_id += 1
        return self._req_id

    @property
    def _headers(self) -> dict[str, str]:
        h = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session_id:
            h["Mcp-Session-Id"] = self.session_id
        return h

    # -- low-level --------------------------------------------------------

    def _post_sse(self, body: dict, timeout: int | None = None) -> dict:
        """POST to /mcp, parse SSE stream, return last JSON-RPC data frame."""
        r = requests.post(
            self.mcp_url,
            json=body,
            headers=self._headers,
            stream=True,
            timeout=timeout or self.timeout,
        )
        r.raise_for_status()

        result: dict = {}
        for line in r.iter_lines(decode_unicode=True):
            if line and line.startswith("data: {"):
                try:
                    result = json.loads(line[len("data: ") :])
                except json.JSONDecodeError:
                    pass
        return result

    def raw_post(
        self, path: str = "/mcp", headers: dict | None = None, body: Any = None
    ) -> requests.Response:
        """Send a raw HTTP request (for protocol-level tests)."""
        url = f"{self.base_url}{path}"
        return requests.post(
            url,
            json=body,
            headers=headers or {},
            timeout=10,
        )

    def raw_get(
        self, path: str = "/mcp", headers: dict | None = None
    ) -> requests.Response:
        url = f"{self.base_url}{path}"
        return requests.get(url, headers=headers or {}, timeout=10)

    def raw_delete(
        self, path: str = "/mcp", headers: dict | None = None
    ) -> requests.Response:
        url = f"{self.base_url}{path}"
        return requests.delete(url, headers=headers or {}, timeout=10)

    # -- MCP protocol -----------------------------------------------------

    def initialize(self) -> dict:
        """Run the initialize handshake and store the session id."""
        body = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"},
            },
        }
        r = requests.post(
            self.mcp_url,
            json=body,
            headers=self._headers,
            stream=True,
            timeout=15,
        )
        r.raise_for_status()

        self.session_id = r.headers.get("Mcp-Session-Id")

        result: dict = {}
        for line in r.iter_lines(decode_unicode=True):
            if line and line.startswith("data: {"):
                try:
                    result = json.loads(line[len("data: ") :])
                except json.JSONDecodeError:
                    pass

        # send initialized notification
        notif = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        requests.post(
            self.mcp_url,
            json=notif,
            headers=self._headers,
            timeout=5,
        )
        time.sleep(0.2)
        return result

    def list_tools(self) -> list[str]:
        body = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "tools/list",
        }
        resp = self._post_sse(body, timeout=15)
        tools = resp.get("result", {}).get("tools", [])
        return [t["name"] for t in tools]

    def call_tool(self, name: str, arguments: dict | None = None) -> ToolResult:
        """Call an MCP tool and return a parsed ToolResult."""
        body = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}},
        }
        try:
            resp = self._post_sse(body)
        except Exception as exc:
            return ToolResult(raw={}, text=str(exc), is_error=True)

        if not resp:
            return ToolResult(raw={}, text="<no response>", is_error=True)

        # JSON-RPC error (e.g. unknown tool)
        if "error" in resp:
            msg = resp["error"].get("message", str(resp["error"]))
            return ToolResult(raw=resp, text=msg, is_error=True)

        result = resp.get("result", {})
        content = result.get("content", [])
        text = content[0]["text"] if content else ""
        is_error = bool(result.get("isError", False))
        return ToolResult(raw=resp, text=text, is_error=is_error)

    def call_method(self, method: str, params: dict | None = None) -> dict:
        """Call an arbitrary JSON-RPC method."""
        body = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": method,
            "params": params or {},
        }
        return self._post_sse(body, timeout=15)


# ── Test Framework ────────────────────────────────────────────────────────────


@dataclass
class TestCase:
    name: str
    group: str
    func: Any  # callable
    slow: bool = False


_tests: list[TestCase] = []


def test(group: str, name: str, *, slow: bool = False):
    """Decorator to register a test function."""

    def decorator(fn):
        _tests.append(TestCase(name=name, group=group, func=fn, slow=slow))
        return fn

    return decorator


def run_tests(
    client: McpClient,
    *,
    quick: bool = False,
    filter_pattern: str | None = None,
    verbose: bool = False,
) -> bool:
    passed = 0
    failed = 0
    skipped = 0
    failures: list[tuple[str, str]] = []

    current_group = ""
    for tc in _tests:
        # filter
        if filter_pattern and filter_pattern.lower() not in tc.name.lower():
            continue

        # group header
        if tc.group != current_group:
            current_group = tc.group
            print(f"\n{BOLD}{current_group}{NC}")

        label = f"  {tc.name:<55s}"

        if quick and tc.slow:
            skipped += 1
            print(f"{label}{YELLOW}SKIP{NC}")
            continue

        try:
            result = tc.func(client)
            # test functions return None for pass, or a string reason on fail
            if result is None:
                passed += 1
                print(f"{label}{GREEN}PASS{NC}")
            else:
                failed += 1
                failures.append((tc.name, str(result)))
                print(f"{label}{RED}FAIL{NC}")
                print(f"    {DIM}{str(result)[:200]}{NC}")
        except Exception as exc:
            failed += 1
            failures.append((tc.name, str(exc)))
            print(f"{label}{RED}FAIL{NC}")
            print(f"    {DIM}{str(exc)[:200]}{NC}")

    total = passed + failed + skipped
    print(f"\n{BOLD}{CYAN}{'═' * 55}{NC}")
    print(
        f"  {GREEN}Passed: {passed}{NC}  "
        f"{RED}Failed: {failed}{NC}  "
        f"{YELLOW}Skipped: {skipped}{NC}  "
        f"Total: {total}"
    )
    print(f"{BOLD}{CYAN}{'═' * 55}{NC}")

    if failures:
        print(f"\n{RED}Failed tests:{NC}")
        for name, reason in failures:
            print(f"  - {name}: {reason[:120]}")
    print()

    return failed == 0


# ══════════════════════════════════════════════════════════════════════════════
#  TEST DEFINITIONS
# ══════════════════════════════════════════════════════════════════════════════

CREATED_NOTE = "/data/notes/_test_integration_note.md"

# ── Protocol & Transport ──────────────────────────────────────────────────────


@test("Protocol & Transport", "GET wrong path → 404")
def _(c: McpClient):
    r = c.raw_get("/wrong-path")
    if r.status_code != 404:
        return f"expected 404, got {r.status_code}"


@test("Protocol & Transport", "POST without Accept header → 406")
def _(c: McpClient):
    r = c.raw_post(
        headers={"Content-Type": "application/json"},
        body={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
    )
    if r.status_code != 406:
        return f"expected 406, got {r.status_code}"


@test("Protocol & Transport", "GET /mcp without session → rejected")
def _(c: McpClient):
    r = c.raw_get(headers={"Accept": "text/event-stream"})
    if r.status_code == 200:
        return f"expected non-200, got {r.status_code}"


@test("Protocol & Transport", "Bogus session ID → 404")
def _(c: McpClient):
    r = c.raw_post(
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "Mcp-Session-Id": "bogus-nonexistent-session",
        },
        body={"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
    )
    if r.status_code != 404:
        return f"expected 404, got {r.status_code}"


@test("Protocol & Transport", "Invalid JSON-RPC method → error")
def _(c: McpClient):
    resp = c.call_method("nonexistent/method")
    if "error" not in resp:
        return "expected JSON-RPC error"


@test("Protocol & Transport", "Initialize returns server info + capabilities")
def _(c: McpClient):
    # Use a fresh connection (don't reuse session)
    body = {
        "jsonrpc": "2.0",
        "id": 9999,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "init-check", "version": "0.1"},
        },
    }
    r = requests.post(
        c.mcp_url,
        json=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
        stream=True,
        timeout=10,
    )
    resp = {}
    for line in r.iter_lines(decode_unicode=True):
        if line and line.startswith("data: {"):
            resp = json.loads(line[len("data: ") :])

    info = resp.get("result", {}).get("serverInfo", {})
    if info.get("name") != "second-brain":
        return f"unexpected server name: {info.get('name')}"
    if "tools" not in resp.get("result", {}).get("capabilities", {}):
        return "missing tools capability"


@test("Protocol & Transport", "tools/list returns 15 tools")
def _(c: McpClient):
    tools = c.list_tools()
    if len(tools) != 16:
        return f"expected 15 tools, got {len(tools)}: {tools}"


@test("Protocol & Transport", "Concurrent sessions are independent")
def _(c: McpClient):
    # Create a second client with its own session
    c2 = McpClient(c.base_url)
    c2.initialize()
    if not c.session_id or not c2.session_id:
        return "missing session ID"
    if c.session_id == c2.session_id:
        return "sessions should have different IDs"


@test("Protocol & Transport", "DELETE session")
def _(c: McpClient):
    # Create a throwaway session to delete
    c2 = McpClient(c.base_url)
    c2.initialize()
    r = c2.raw_delete(headers={"Mcp-Session-Id": c2.session_id})
    if r.status_code not in (200, 202, 204, 405):
        return f"expected 200/202/204/405, got {r.status_code}"


# ── Ingestion ─────────────────────────────────────────────────────────────────


@test("Ingestion", "note_ingest: valid directory")
def _(c: McpClient):
    r = c.call_tool("note_ingest", {"path": "/data/notes", "embed": False})
    if r.is_error:
        return f"error: {r.text}"
    if "ingested" not in r.text and "skipped" not in r.text:
        return f"unexpected result: {r.text[:100]}"


@test("Ingestion", "note_ingest: nonexistent path (graceful)")
def _(c: McpClient):
    r = c.call_tool("note_ingest", {"path": "/nonexistent/dir", "embed": False})
    if not r.text:
        return "expected some response text"


# ── Full-Text Search ──────────────────────────────────────────────────────────


@test("Full-Text Search", "note_search: basic query")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": "test"})
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty result"


@test("Full-Text Search", "note_search: with limit")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": "test", "limit": 2})
    if r.is_error:
        return f"error: {r.text}"


@test("Full-Text Search", "note_search: empty query (graceful)")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": ""})
    if not r.text:
        return "expected some response"


@test("Full-Text Search", "note_search: SQL injection attempt")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": "'; DROP TABLE notes; --"})
    # Must not crash; table must still exist
    if not r.text:
        return "no response"
    r2 = c.call_tool("note_list", {"limit": 1})
    if r2.is_error:
        return "notes table may have been affected!"


@test("Full-Text Search", "note_search: XSS payload")
def _(c: McpClient):
    r = c.call_tool(
        "note_search", {"query": '<script>alert("xss")</script> & test'}
    )
    if not r.text:
        return "no response"


@test("Full-Text Search", "note_search: unicode query")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": "日本語テスト"})
    if not r.text:
        return "no response"


@test("Full-Text Search", "note_search: very long query (1000 words)")
def _(c: McpClient):
    long_query = " ".join(["test"] * 1000)
    r = c.call_tool("note_search", {"query": long_query})
    if not r.text:
        return "no response"


# ── Note Listing ──────────────────────────────────────────────────────────────


@test("Note Listing", "note_list: defaults")
def _(c: McpClient):
    r = c.call_tool("note_list", {})
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty"


@test("Note Listing", "note_list: pagination")
def _(c: McpClient):
    r = c.call_tool("note_list", {"limit": 5, "offset": 0})
    if r.is_error:
        return f"error: {r.text}"


@test("Note Listing", "note_list: lifecycle filter")
def _(c: McpClient):
    r = c.call_tool("note_list", {"lifecycle": "active", "limit": 3})
    if r.is_error:
        return f"error: {r.text}"


@test("Note Listing", "note_list: invalid lifecycle (graceful)")
def _(c: McpClient):
    r = c.call_tool("note_list", {"lifecycle": "nonexistent_state"})
    if not r.text:
        return "no response"


# ── Note Reading ──────────────────────────────────────────────────────────────


@test("Note Reading", "note_read: valid note")
def _(c: McpClient):
    r = c.call_tool("note_read", {"file_path": "/data/notes/TODO.md"})
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty content"


@test("Note Reading", "note_read: nonexistent → error")
def _(c: McpClient):
    r = c.call_tool(
        "note_read", {"file_path": "/data/notes/this_does_not_exist_xyz.md"}
    )
    if not r.is_error and "not found" not in r.text.lower():
        return f"expected error, got: {r.text[:80]}"


@test("Note Reading", "note_read: path traversal blocked")
def _(c: McpClient):
    r = c.call_tool("note_read", {"file_path": "../../etc/passwd"})
    if "root:" in r.text:
        return "SECURITY: path traversal leaked /etc/passwd!"


# ── Note CRUD ─────────────────────────────────────────────────────────────────


@test("Note CRUD", "note_create: new note")
def _(c: McpClient):
    content = (
        "# Integration Test Note\n\n"
        "Created by test-mcp-http.py.\n"
        "Link: [[TODO]]\n"
    )
    r = c.call_tool("note_create", {"file_path": CREATED_NOTE, "content": content})
    if r.is_error:
        return f"error: {r.text}"


@test("Note CRUD", "note_read: read created note back")
def _(c: McpClient):
    # Small delay: the background sync processor may race with note_create's
    # ingestion if a previous test run deleted this file (soft-delete propagation).
    time.sleep(1)
    r = c.call_tool("note_read", {"file_path": CREATED_NOTE})
    if r.is_error:
        return f"error: {r.text}"
    if "Integration Test" not in r.text:
        return f"content mismatch: {r.text[:80]}"


@test("Note CRUD", "note_update: modify created note")
def _(c: McpClient):
    content = (
        "# Updated Integration Test\n\n"
        "Updated by test suite.\n"
        "Links: [[TODO]] and [[emcheck_architecture]]\n"
    )
    r = c.call_tool("note_update", {"file_path": CREATED_NOTE, "content": content})
    if r.is_error:
        return f"error: {r.text}"


@test("Note CRUD", "note_update: nonexistent note (graceful)")
def _(c: McpClient):
    r = c.call_tool(
        "note_update",
        {"file_path": "/data/notes/no_such_note_xyz.md", "content": "# Nope"},
    )
    if not r.text:
        return "no response"


@test("Note CRUD", "note_classify: set lifecycle to volatile")
def _(c: McpClient):
    r = c.call_tool(
        "note_classify", {"file_path": CREATED_NOTE, "lifecycle": "volatile"}
    )
    if r.is_error:
        return f"error: {r.text}"


@test("Note CRUD", "note_classify: invalid lifecycle (graceful)")
def _(c: McpClient):
    r = c.call_tool(
        "note_classify",
        {"file_path": CREATED_NOTE, "lifecycle": "not_a_real_state"},
    )
    if not r.text:
        return "no response"


@test("Note CRUD", "note_stamp: tag human edit")
def _(c: McpClient):
    r = c.call_tool("note_stamp", {"file_path": CREATED_NOTE, "editor": "calebbarzee"})
    if r.is_error:
        return f"error: {r.text}"
    if "calebbarzee" not in r.text:
        return f"expected editor name in response: {r.text[:80]}"


@test("Note CRUD", "note_stamp: nonexistent file (graceful)")
def _(c: McpClient):
    r = c.call_tool(
        "note_stamp", {"file_path": "/data/notes/nope.md", "editor": "someone"}
    )
    if not r.is_error and "not found" not in r.text.lower():
        return f"expected error: {r.text[:80]}"


# ── Link Graph ────────────────────────────────────────────────────────────────


@test("Link Graph", "note_graph: note with links")
def _(c: McpClient):
    r = c.call_tool("note_graph", {"file_path": CREATED_NOTE})
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty graph"


@test("Link Graph", "note_graph: nonexistent note (graceful)")
def _(c: McpClient):
    r = c.call_tool("note_graph", {"file_path": "/data/notes/nope_nope.md"})
    if not r.text:
        return "no response"


# ── Embeddings & Semantic Search ──────────────────────────────────────────────


@test("Embeddings & Semantic Search", "embed_notes: process new", slow=True)
def _(c: McpClient):
    r = c.call_tool("embed_notes", {"only_new": True})
    if r.is_error:
        return f"error: {r.text}"


@test(
    "Embeddings & Semantic Search",
    "semantic_search: natural language query",
    slow=True,
)
def _(c: McpClient):
    r = c.call_tool(
        "semantic_search",
        {"query": "architecture and design patterns", "limit": 3},
    )
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty result"


@test("Embeddings & Semantic Search", "semantic_search: with filters", slow=True)
def _(c: McpClient):
    r = c.call_tool(
        "semantic_search",
        {"query": "testing", "lifecycle": "active", "limit": 3},
    )
    if r.is_error:
        return f"error: {r.text}"


@test("Embeddings & Semantic Search", "find_related: by file path", slow=True)
def _(c: McpClient):
    r = c.call_tool("find_related", {"file_path": "/data/notes/TODO.md", "limit": 3})
    if r.is_error:
        return f"error: {r.text}"


# ── File Search ───────────────────────────────────────────────────────────────


@test("File Search", "file_search: content mode")
def _(c: McpClient):
    r = c.call_tool("file_search", {"query": "test", "mode": "content", "limit": 5})
    if r.is_error:
        return f"error: {r.text}"
    if not r.text:
        return "empty"


@test("File Search", "file_search: filename mode")
def _(c: McpClient):
    r = c.call_tool("file_search", {"query": "TODO", "mode": "filename", "limit": 5})
    if r.is_error:
        return f"error: {r.text}"
    if "TODO" not in r.text:
        return f"expected TODO in results: {r.text[:80]}"


@test("File Search", "file_search: empty query (graceful)")
def _(c: McpClient):
    r = c.call_tool("file_search", {"query": "", "mode": "content"})
    if not r.text:
        return "no response"


# ── Projects ──────────────────────────────────────────────────────────────────


@test("Projects", "project_list")
def _(c: McpClient):
    r = c.call_tool("project_list", {})
    if r.is_error:
        return f"error: {r.text}"


@test("Projects", "project_context: nonexistent project (graceful)")
def _(c: McpClient):
    r = c.call_tool("project_context", {"project": "nonexistent_project_xyz"})
    if not r.text:
        return "no response"


# ── Skills ────────────────────────────────────────────────────────────────────


@test("Skills", "run_skill: summarize (dry run)")
def _(c: McpClient):
    r = c.call_tool(
        "run_skill", {"skill": "summarize", "period": "this-week", "dry_run": True}
    )
    if r.is_error:
        return f"error: {r.text}"


@test("Skills", "run_skill: nonexistent skill → error")
def _(c: McpClient):
    r = c.call_tool("run_skill", {"skill": "nonexistent_skill_xyz"})
    if not r.text:
        return "no response"


# ── Error Handling ────────────────────────────────────────────────────────────


@test("Error Handling", "Missing required argument (note_search without query)")
def _(c: McpClient):
    r = c.call_tool("note_search", {})
    if not r.is_error and "error" not in json.dumps(r.raw).lower():
        return "expected error for missing required arg"


@test("Error Handling", "Wrong argument type (query as integer)")
def _(c: McpClient):
    r = c.call_tool("note_search", {"query": 12345})
    if not r.text:
        return "no response"


@test("Error Handling", "Nonexistent tool → error")
def _(c: McpClient):
    r = c.call_tool("this_tool_does_not_exist", {"foo": "bar"})
    if not r.is_error and "error" not in json.dumps(r.raw).lower():
        return "expected error for unknown tool"


@test("Error Handling", "Null arguments (graceful)")
def _(c: McpClient):
    r = c.call_tool("note_list")  # None arguments → {}
    if not r.text:
        return "no response"


@test("Error Handling", "Deeply nested JSON argument")
def _(c: McpClient):
    nested = {"query": "test", "extra": {"a": {"b": {"c": {"d": "deep"}}}}}
    r = c.call_tool("note_search", nested)
    # Should not crash — extra fields are ignored
    if not r.text:
        return "no response"


# ══════════════════════════════════════════════════════════════════════════════
#  MAIN
# ══════════════════════════════════════════════════════════════════════════════


def cleanup(created_note: str):
    """Remove the test note from the container."""
    subprocess.run(
        ["docker", "exec", "secondbrain-app", "rm", "-f", created_note],
        capture_output=True,
    )


def main():
    parser = argparse.ArgumentParser(
        description="MCP HTTP integration tests for Second Brain"
    )
    parser.add_argument(
        "--base-url",
        default="http://localhost:8080",
        help="Base URL of the MCP server (default: http://localhost:8080)",
    )
    parser.add_argument(
        "--quick", action="store_true", help="Skip slow tests (embed, semantic)"
    )
    parser.add_argument(
        "-k", "--filter", default=None, help="Only run tests whose name matches FILTER"
    )
    parser.add_argument(
        "-v", "--verbose", action="store_true", help="Show result text for every test"
    )
    args = parser.parse_args()

    print()
    print(f"{BOLD}{CYAN}{'═' * 55}{NC}")
    print(f"{BOLD}{CYAN}  Second Brain — MCP HTTP Integration Tests{NC}")
    print(f"{BOLD}{CYAN}{'═' * 55}{NC}")
    print(f"  Endpoint: {args.base_url}/mcp")
    print()

    # Verify connectivity
    try:
        r = requests.get(f"{args.base_url}/", timeout=5)
    except requests.ConnectionError:
        print(f"{RED}Cannot reach {args.base_url} — is the stack running?{NC}")
        sys.exit(1)

    # Create session
    print(f"{BOLD}Setting up test session...{NC}")
    client = McpClient(args.base_url)
    try:
        client.initialize()
    except Exception as exc:
        print(f"{RED}Failed to initialize MCP session: {exc}{NC}")
        sys.exit(1)
    print(f"  Session: {DIM}{client.session_id}{NC}")

    # Run
    try:
        success = run_tests(
            client, quick=args.quick, filter_pattern=args.filter, verbose=args.verbose
        )
    finally:
        cleanup(CREATED_NOTE)

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
