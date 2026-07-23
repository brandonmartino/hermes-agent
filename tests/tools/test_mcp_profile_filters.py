"""Profile-local MCP server cold-load filters."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

import pytest


SERVERS = {
    "fast": {"command": "fast-mcp"},
    "slow": {"command": "slow-mcp"},
    "missing": {"command": "missing-mcp"},
}


def test_filter_defaults_preserve_all_servers():
    from hermes_cli.mcp_server_filters import filter_mcp_servers_for_profile

    assert filter_mcp_servers_for_profile(SERVERS, {"mcp_servers": SERVERS}) == SERVERS


@pytest.mark.parametrize(
    ("mcp_config", "expected"),
    [
        ({"servers": {"include": ["fast"]}}, {"fast"}),
        ({"servers": {"include": "fast, missing"}}, {"fast", "missing"}),
        ({"servers": {"exclude": ["slow"]}}, {"fast", "missing"}),
        # include wins so operators can keep a denylist around without it
        # unexpectedly subtracting from a deliberate cold-load allowlist.
        ({"servers": {"include": ["fast"], "exclude": ["fast", "slow"]}}, {"fast"}),
        ({"server_allowlist": ["slow"]}, {"slow"}),
        ({"server_denylist": "missing"}, {"fast", "slow"}),
    ],
)
def test_filter_applies_include_exclude_aliases(mcp_config, expected):
    from hermes_cli.mcp_server_filters import filter_mcp_servers_for_profile

    result = filter_mcp_servers_for_profile(SERVERS, {"mcp": mcp_config})

    assert set(result) == expected


def test_load_mcp_config_filters_before_env_interpolation(monkeypatch):
    """Denied servers are absent before connect/register can spawn them."""
    from tools.mcp_tool import _load_mcp_config

    config = {
        "mcp": {"servers": {"include": ["fast"]}},
        "mcp_servers": {
            "fast": {"command": "${FAST_CMD}"},
            "slow": {"command": "${SLOW_CMD}"},
        },
    }
    monkeypatch.setenv("FAST_CMD", "fast-mcp")
    monkeypatch.setenv("SLOW_CMD", "slow-mcp")
    monkeypatch.setattr("hermes_cli.config.load_config", lambda: config)

    result = _load_mcp_config()

    assert result == {"fast": {"command": "fast-mcp"}}


def test_discover_mcp_tools_ignores_missing_allowlist_entry(monkeypatch):
    """An allowlist entry with no matching mcp_servers row is a no-op."""
    import tools.mcp_tool as mcp_tool

    monkeypatch.setattr(mcp_tool, "_MCP_AVAILABLE", True)
    monkeypatch.setattr(
        mcp_tool,
        "_load_mcp_config",
        lambda: {},
    )
    called = []
    monkeypatch.setattr(mcp_tool, "register_mcp_servers", lambda servers: called.append(servers))

    assert mcp_tool.discover_mcp_tools() == []
    assert called == []


@pytest.mark.asyncio
async def test_register_only_connects_filtered_servers(monkeypatch):
    """Skipped MCP servers spawn/connect zero times and add no tools."""
    import tools.mcp_tool as mcp_tool

    connected: list[str] = []
    registered: list[str] = []

    async def fake_connect(name, config):
        connected.append(name)
        return SimpleNamespace(
            name=name,
            session=object(),
            _tools=[],
            _config=config,
            _registered_tool_names=[],
        )

    def fake_register(name, server, config):
        registered.append(name)
        return [f"mcp_{name}_tool"]

    monkeypatch.setattr(mcp_tool, "_MCP_AVAILABLE", True)
    monkeypatch.setattr(mcp_tool, "_connect_server", fake_connect)
    monkeypatch.setattr(mcp_tool, "_register_server_tools", fake_register)
    monkeypatch.setattr(mcp_tool, "_filter_suspicious_mcp_servers", lambda servers: dict(servers))
    monkeypatch.setattr(mcp_tool, "_connect_cooldown_active", lambda _name: False)

    with mcp_tool._lock:
        saved_servers = dict(mcp_tool._servers)
        saved_connecting = set(mcp_tool._server_connecting)
        saved_errors = dict(mcp_tool._server_connect_errors)
        mcp_tool._servers.clear()
        mcp_tool._server_connecting.clear()
        mcp_tool._server_connect_errors.clear()
    try:
        allowed = {"fast": SERVERS["fast"]}
        assert mcp_tool.register_mcp_servers(allowed) == ["mcp_fast_tool"]
    finally:
        with mcp_tool._lock:
            mcp_tool._servers.clear()
            mcp_tool._servers.update(saved_servers)
            mcp_tool._server_connecting.clear()
            mcp_tool._server_connecting.update(saved_connecting)
            mcp_tool._server_connect_errors.clear()
            mcp_tool._server_connect_errors.update(saved_errors)
        mcp_tool._stop_mcp_loop()

    assert connected == ["fast"]
    assert registered == ["fast"]


def _write_config(home: Path, text: str) -> None:
    home.mkdir(parents=True, exist_ok=True)
    (home / "config.yaml").write_text(text, encoding="utf-8")


def test_temp_hermes_home_profiles_resolve_independent_filters(tmp_path, monkeypatch):
    """Different active profile homes resolve different allowed MCP sets."""
    from tools.mcp_tool import _load_mcp_config

    default_home = tmp_path / ".hermes"
    coder_home = default_home / "profiles" / "coder"
    _write_config(
        default_home,
        """
mcp:
  servers:
    include: [fast]
mcp_servers:
  fast:
    command: fast-mcp
  slow:
    command: slow-mcp
""".strip(),
    )
    _write_config(
        coder_home,
        """
mcp:
  servers:
    exclude: [fast]
mcp_servers:
  fast:
    command: fast-mcp
  slow:
    command: slow-mcp
""".strip(),
    )

    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    monkeypatch.setenv("HERMES_HOME", str(default_home))
    assert set(_load_mcp_config()) == {"fast"}

    monkeypatch.setenv("HERMES_HOME", str(coder_home))
    assert set(_load_mcp_config()) == {"slow"}


def test_background_probe_skips_thread_when_filters_remove_every_server(monkeypatch):
    from hermes_cli import mcp_startup

    monkeypatch.setattr(
        "hermes_cli.config.read_raw_config",
        lambda: {
            "mcp": {"servers": {"include": ["missing"]}},
            "mcp_servers": {"slow": {"command": "slow-mcp"}},
        },
    )

    assert mcp_startup._has_configured_mcp_servers() is False
