from unittest.mock import patch

from hermes_cli import update_restart


def test_no_gateway_is_a_noop():
    with patch("gateway.status.get_running_pid", return_value=None):
        assert update_restart.request_post_update_restart() == "no gateway running"


def test_supervised_posix_gateway_gets_drain_signal(monkeypatch, tmp_path):
    unit = tmp_path / "hermes-gateway.service"
    unit.write_text("unit")
    monkeypatch.setattr(update_restart.sys, "platform", "linux")
    with (
        patch("gateway.status.get_running_pid", return_value=123),
        patch("hermes_cli.gateway.supports_systemd_services", return_value=True),
        patch("hermes_cli.gateway.get_systemd_unit_path", return_value=unit),
        patch("hermes_cli.gateway.is_macos", return_value=False),
        patch("hermes_cli.gateway.is_container", return_value=False),
        patch("os.kill") as kill,
    ):
        result = update_restart.request_post_update_restart()

    kill.assert_called_once_with(123, update_restart.signal.SIGUSR1)
    assert "drain and restart" in result


def test_foreground_gateway_is_not_killed(monkeypatch, tmp_path):
    monkeypatch.setattr(update_restart.sys, "platform", "linux")
    with (
        patch("gateway.status.get_running_pid", return_value=123),
        patch("hermes_cli.gateway.supports_systemd_services", return_value=False),
        patch("hermes_cli.gateway.get_systemd_unit_path", return_value=tmp_path / "missing"),
        patch("hermes_cli.gateway.is_macos", return_value=False),
        patch("hermes_cli.gateway.is_container", return_value=False),
        patch("os.kill") as kill,
    ):
        result = update_restart.request_post_update_restart()

    kill.assert_not_called()
    assert "foreground terminal" in result


def test_windows_uses_canonical_restart(monkeypatch):
    monkeypatch.setattr(update_restart.sys, "platform", "win32")
    with (
        patch("gateway.status.get_running_pid", return_value=123),
        patch("hermes_cli.gateway_windows.restart") as restart,
    ):
        result = update_restart.request_post_update_restart()

    restart.assert_called_once_with()
    assert "Windows gateway" in result
