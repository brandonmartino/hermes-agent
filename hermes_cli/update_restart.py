"""Post-update gateway restart bridge.

Uses the canonical gateway identity and supervisor detection paths; the Rust
updater invokes this module from the newly activated slot after a flip.
"""

from __future__ import annotations

import os
import signal
import sys


def request_post_update_restart() -> str:
    from gateway.status import get_running_pid

    pid = get_running_pid()
    if pid is None:
        return "no gateway running"

    if sys.platform == "win32":
        from hermes_cli import gateway_windows

        gateway_windows.restart()
        return f"restarted Windows gateway {pid}"

    from hermes_cli.gateway import (
        get_launchd_plist_path,
        get_systemd_unit_path,
        is_container,
        is_macos,
        supports_systemd_services,
    )

    supervised = (
        (supports_systemd_services() and (
            get_systemd_unit_path(system=False).exists()
            or get_systemd_unit_path(system=True).exists()
        ))
        or (is_macos() and get_launchd_plist_path().exists())
        or is_container()
    )
    if not supervised:
        return f"gateway {pid} is running in a foreground terminal; restart it manually"
    if not hasattr(signal, "SIGUSR1"):
        return f"gateway {pid} cannot be signaled on this platform; restart it manually"

    os.kill(pid, signal.SIGUSR1)
    return f"gateway {pid} signaled to drain and restart"


def main() -> int:
    print(request_post_update_restart())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
