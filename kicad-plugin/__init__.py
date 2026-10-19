"""
Voltaris -- Action Plugin launcher.

This thin Python plugin registers a menu item in KiCad's Tools menu.
Clicking it opens a settings dialog where users can configure paths
and start/stop the compiled Rust MCP server binary.

Installation (KiCad PCM):
  The plugin appears in KiCad's Plugin and Content Manager.
  After installation the binary is placed in the plugin directory.
  KiCad loads this __init__.py automatically.
"""

import atexit
import os
import shutil
import subprocess
import sys
import threading

import pcbnew  # Available inside KiCad

# Import the settings dialog (same directory)
_plugin_dir = os.path.dirname(os.path.abspath(__file__))
if _plugin_dir not in sys.path:
    sys.path.insert(0, _plugin_dir)

from settings_dialog import VoltarisSettingsDialog, load_settings
from native_bridge import NativeSpecctraBridge
import wx

PLUGIN_DIR = _plugin_dir
BINARY_NAME = "voltaris.exe" if sys.platform == "win32" else "voltaris"
BINARY_PATH = os.path.join(PLUGIN_DIR, "bin", BINARY_NAME)
SETTINGS_PATH = os.path.join(PLUGIN_DIR, "settings.json")

_server_process = None
_server_thread = None

# Windows GUI parents (pcbnew) attach a new console to their subprocesses.
# That console makes stdin look like a TTY, which triggers voltaris's
# double-click install wizard instead of MCP server mode. CREATE_NO_WINDOW
# suppresses the console; stdin=PIPE stays intact.
_POPEN_FLAGS = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0

_CACHE_DIR = os.path.join(
    os.environ.get("LOCALAPPDATA", os.path.expanduser("~/.cache")),
    "voltaris", "cache",
)
_PID_FILE = os.path.join(_CACHE_DIR, "server.pid")
_native_bridge = NativeSpecctraBridge(pcbnew, wx)


def _sync_native_bridge():
    """Apply the persisted KiCad-10 bridge setting without exposing internals."""
    enabled = bool(load_settings(SETTINGS_PATH).get("native_specctra_bridge", False))
    if enabled:
        try:
            _native_bridge.start()
        except Exception as error:
            print(f"[Voltaris] Native Specctra bridge failed to start: {error}", file=sys.stderr)
    else:
        _native_bridge.stop()


def _stop_native_bridge():
    _native_bridge.stop()


def _stage(source_path):
    """Copy file to LOCALAPPDATA cache, return cached path.

    OneDrive-redirected Documents (common in enterprise Windows) tag both
    the plugin's voltaris.exe AND settings.json with IO_REPARSE_TAG_CLOUD,
    which trips ERROR_ACCESS_DENIED on execute and on serde config read.
    Staging to %LOCALAPPDATA% (never OneDrive-synced) sidesteps it.
    """
    if sys.platform != "win32":
        return source_path
    os.makedirs(_CACHE_DIR, exist_ok=True)
    dst = os.path.join(_CACHE_DIR, os.path.basename(source_path))
    shutil.copy2(source_path, dst)
    return dst


def _write_pid_file(pid):
    """Record the PID of the server this session just spawned."""
    os.makedirs(_CACHE_DIR, exist_ok=True)
    with open(_PID_FILE, "w") as f:
        f.write(str(pid))


def _clear_pid_file(pid):
    """Remove the PID file only while it still records ``pid``.

    Every KiCad session writes the same file. Removing it unconditionally
    lets the session whose server exits last delete a record written by a
    session whose server is still running: the survivor becomes untracked,
    so no later start_server preflight can ever reap it.
    """
    try:
        with open(_PID_FILE) as f:
            recorded = int(f.read().strip())
    except (OSError, ValueError):
        return
    if recorded != pid:
        return
    try:
        os.remove(_PID_FILE)
    except OSError:
        pass


def _terminate_owned_server():
    """Kill the server this session spawned, on KiCad exit.

    _run_server sits on a daemon thread, so interpreter shutdown drops the
    thread without touching the child. The server then outlives KiCad,
    holding the HTTP port and serving from a possibly pre-update binary --
    an http-transport server never reads stdin, so parent death is invisible
    to it and nothing else ends it.

    Best-effort by nature: a hard kill of KiCad runs no atexit handler, which
    is why the preflight reap stays.
    """
    process = _server_process
    if process is None or process.poll() is not None:
        return
    try:
        process.terminate()
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
    except OSError:
        return
    _clear_pid_file(process.pid)


atexit.register(_terminate_owned_server)
atexit.register(_stop_native_bridge)


def _kill_tracked():
    """Terminate the PID recorded in the PID file (best-effort), clear the file.

    os.kill(pid, 0) is unsafe on Windows -- it invokes TerminateProcess for any
    signal value, so it would actually kill the process instead of probing.
    Use taskkill/kill by PID; if the PID is stale, the kill is a harmless no-op.
    """
    try:
        with open(_PID_FILE) as f:
            pid = int(f.read().strip())
    except (OSError, ValueError):
        return
    try:
        if sys.platform == "win32":
            subprocess.run(
                ["taskkill", "/F", "/PID", str(pid)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=_POPEN_FLAGS,
            )
        else:
            os.kill(pid, 9)
    except OSError:
        pass
    try:
        os.remove(_PID_FILE)
    except OSError:
        pass


def _run_server():
    """Run the MCP server subprocess. Exit means exit -- no retry loop.

    A retry loop respawns voltaris after Stop Server kills it (wait() returns
    normally on external kill, so nothing distinguishes "crashed" from
    "stopped"). If voltaris crashes on startup, the user clicks Start again.
    """
    global _server_process
    pid = None
    try:
        args = [_stage(BINARY_PATH)]
        if os.path.exists(SETTINGS_PATH):
            args += ["--config", _stage(SETTINGS_PATH)]
        # pcbnew has no valid stderr; inheriting gives voltaris a broken
        # handle and tracing_subscriber errors on init. DEVNULL is a
        # valid sink.
        _server_process = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            creationflags=_POPEN_FLAGS,
        )
        pid = _server_process.pid
        _write_pid_file(pid)
        _server_process.wait()
    except Exception as e:
        print(f"[Voltaris] Server error: {e}", file=sys.stderr)
    finally:
        _server_process = None
        if pid is not None:
            _clear_pid_file(pid)


def start_server():
    """Start the MCP server in a background thread."""
    global _server_thread

    if not os.path.exists(BINARY_PATH):
        wx.MessageBox(
            f"Voltaris binary not found at:\n{BINARY_PATH}\n\n"
            "Please reinstall the plugin.",
            "Voltaris",
            wx.OK | wx.ICON_ERROR,
        )
        return False

    # Orphan from a previous KiCad session may still be holding the port.
    _kill_tracked()

    if _server_thread and _server_thread.is_alive():
        return True

    _server_thread = threading.Thread(target=_run_server, daemon=True)
    _server_thread.start()
    return True


def stop_server():
    """Stop the MCP server subprocess."""
    _kill_tracked()


def is_server_running():
    """Report whether we've launched a server we haven't stopped.

    PID file existence as a proxy -- survives module re-imports. If the
    server crashed on its own, the next start_server preflight cleans up.
    """
    return os.path.exists(_PID_FILE)


# ─── KiCad Action Plugin entry point ─────────────────────────────────────────

class VoltarisPlugin(pcbnew.ActionPlugin):
    def defaults(self):
        self.name = "Voltaris"
        self.category = "AI Tools"
        self.description = (
            "Configure and control the Voltaris -- enables AI assistants "
            "like Claude to design PCBs and schematics via the Model Context Protocol."
        )
        self.show_toolbar_button = True
        self.icon_file_name = os.path.join(PLUGIN_DIR, "resources", "icon.png")

    def Run(self):
        """Open the settings dialog. Server start/stop is handled via dialog buttons."""
        # Get the KiCad main window as parent for the dialog
        parent = wx.GetTopLevelWindows()[0] if wx.GetTopLevelWindows() else None

        dlg = VoltarisSettingsDialog(
            parent=parent,
            plugin_dir=PLUGIN_DIR,
            binary_path=BINARY_PATH,
            server_running=is_server_running(),
            native_bridge_available=_native_bridge.available(),
            native_bridge_running=_native_bridge.running(),
        )

        result = dlg.ShowModal()

        if result == wx.ID_YES:
            # User clicked "Start Server"
            start_server()
        elif result == wx.ID_NO:
            # User clicked "Stop Server"
            stop_server()

        dlg.Destroy()
        _sync_native_bridge()


# Register the plugin with KiCad
_sync_native_bridge()
VoltarisPlugin().register()
