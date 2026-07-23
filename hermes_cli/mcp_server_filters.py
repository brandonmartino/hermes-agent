"""Helpers for resolving profile-local MCP server cold-load filters.

Kept separate from ``tools.mcp_tool`` so startup probes can decide whether MCP
is configured without importing the optional MCP SDK or spawning transports.
"""

from __future__ import annotations

import logging
from typing import Any, Mapping

logger = logging.getLogger(__name__)

_MCP_FILTER_INCLUDE_KEYS = ("include", "allow", "allowlist", "enabled")
_MCP_FILTER_EXCLUDE_KEYS = ("exclude", "deny", "denylist", "disabled")


def _normalize_server_filter(value: Any, path: str) -> set[str]:
    """Normalize an MCP server-name filter to a set of names.

    Accepts a YAML list/tuple/set or a comma-separated string. Invalid values
    fail closed to an empty filter so existing configurations keep working.
    """
    if value is None or value is False:
        return set()
    if value is True:
        logger.warning(
            "Ignoring boolean true for %s; expected a list of MCP server names",
            path,
        )
        return set()
    if isinstance(value, str):
        items = [part.strip() for part in value.split(",")]
    elif isinstance(value, (list, tuple, set, frozenset)):
        items = [str(item).strip() for item in value]
    else:
        logger.warning(
            "Ignoring invalid MCP server filter at %s; expected list or comma-separated string",
            path,
        )
        return set()
    return {item for item in items if item}


def _first_filter_value(config: Mapping[str, Any], keys: tuple[str, ...]) -> Any:
    for key in keys:
        if key in config:
            return config.get(key)
    return None


def resolve_mcp_server_filters(config: Mapping[str, Any] | None) -> tuple[set[str], set[str]]:
    """Return ``(include, exclude)`` server filters from a loaded config.

    Canonical shape::

        mcp:
          servers:
            include: [github, filesystem]
            exclude: [expensive]

    ``include`` takes precedence over ``exclude``. Short aliases are accepted
    for operator ergonomics and compatibility with existing allow/deny wording.
    """
    if not isinstance(config, Mapping):
        return set(), set()
    mcp_cfg = config.get("mcp") or {}
    if not isinstance(mcp_cfg, Mapping):
        return set(), set()

    servers_cfg = mcp_cfg.get("servers") or {}
    if not isinstance(servers_cfg, Mapping):
        servers_cfg = {}

    include_raw = _first_filter_value(servers_cfg, _MCP_FILTER_INCLUDE_KEYS)
    exclude_raw = _first_filter_value(servers_cfg, _MCP_FILTER_EXCLUDE_KEYS)

    # Backward/ergonomic aliases under mcp: keep these out of DEFAULT_CONFIG so
    # unset config preserves the current behavior byte-for-byte.
    if include_raw is None:
        include_raw = mcp_cfg.get("server_include", mcp_cfg.get("server_allowlist"))
    if exclude_raw is None:
        exclude_raw = mcp_cfg.get("server_exclude", mcp_cfg.get("server_denylist"))

    include = _normalize_server_filter(include_raw, "mcp.servers.include")
    exclude = _normalize_server_filter(exclude_raw, "mcp.servers.exclude")
    return include, exclude


def filter_mcp_servers_for_profile(
    servers: Mapping[str, Any] | None,
    config: Mapping[str, Any] | None,
) -> dict[str, Any]:
    """Apply active-profile MCP server filters to ``mcp_servers``.

    The profile boundary is the active ``config.yaml``/``HERMES_HOME``. Callers
    pass the already-loaded config for that profile; this helper only decides
    which configured server definitions are allowed to cold-connect in this
    process. When no filters are set, the returned mapping is unchanged except
    for a shallow ``dict`` copy.
    """
    if not isinstance(servers, Mapping) or not servers:
        return {}
    include, exclude = resolve_mcp_server_filters(config)
    if include:
        return {name: cfg for name, cfg in servers.items() if name in include}
    if exclude:
        return {name: cfg for name, cfg in servers.items() if name not in exclude}
    return dict(servers)
