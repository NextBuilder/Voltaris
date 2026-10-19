"""Tests for the settings dialog's platform-specific KiCad discovery."""

import importlib.util
import os
from pathlib import Path
import sys
import types
import unittest
from unittest import mock


PLUGIN_DIR = Path(__file__).resolve().parents[1]


def load_settings_dialog():
    wx = types.ModuleType("wx")
    wx.Dialog = object
    sys.modules.setdefault("wx", wx)
    spec = importlib.util.spec_from_file_location(
        "settings_dialog_detection_under_test", PLUGIN_DIR / "settings_dialog.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeRegistryKey:
    def __init__(self, values=None, children=None):
        self.values = values or {}
        self.children = children or {}

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False


def fake_winreg(install_dir):
    child = FakeRegistryKey({
        "DisplayName": "KiCad 10.0 (current user)",
        "DisplayVersion": "10.0.5",
        "InstallLocation": install_dir,
    })
    uninstall = FakeRegistryKey(children={"KiCad 10.0": child})
    module = types.ModuleType("winreg")
    module.HKEY_CURRENT_USER = "HKCU"
    module.HKEY_LOCAL_MACHINE = "HKLM"
    module.KEY_READ = 1
    module.KEY_WOW64_64KEY = 2
    module.KEY_WOW64_32KEY = 4
    module.OpenKey = lambda parent, name, *_args: (
        uninstall if parent == "HKCU" else parent.children[name])
    module.QueryInfoKey = lambda key: (len(key.children), 0, 0)
    module.EnumKey = lambda key, index: list(key.children)[index]
    module.QueryValueEx = lambda key, name: (key.values[name], 1)
    return module


class KiCadDetectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dialog = load_settings_dialog()

    def test_current_user_uninstall_record_is_discovered(self):
        install_dir = r"C:\Users\casey\AppData\Local\Programs\KiCad\10.0"
        registry = fake_winreg(install_dir)
        expected = os.path.join(install_dir, "bin", "kicad-cli.exe")

        with mock.patch.object(self.dialog.sys, "platform", "win32"), \
                mock.patch.dict(sys.modules, {"winreg": registry}), \
                mock.patch.object(self.dialog.os.path, "isfile",
                                  side_effect=lambda path: path == expected):
            self.assertEqual(self.dialog.detect_kicad_cli(), expected)

    def test_local_app_data_is_a_fallback_without_registry(self):
        local_app_data = r"C:\Users\casey\AppData\Local"
        expected = os.path.join(
            local_app_data, "Programs", "KiCad", "10.0", "bin", "kicad-cli.exe")

        with mock.patch.object(self.dialog.sys, "platform", "win32"), \
                mock.patch.dict(self.dialog.os.environ,
                                {"LOCALAPPDATA": local_app_data}, clear=True), \
                mock.patch.dict(sys.modules, {"winreg": None}), \
                mock.patch.object(self.dialog.os.path, "isfile",
                                  side_effect=lambda path: path == expected):
            self.assertEqual(self.dialog.detect_kicad_cli(), expected)


if __name__ == "__main__":
    unittest.main()
