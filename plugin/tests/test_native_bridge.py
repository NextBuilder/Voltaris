"""Tests for the narrowly-scoped KiCad 10 native Specctra bridge."""

import importlib.util
import json
import os
import tempfile
import types
import unittest
import urllib.error
import urllib.request


_PLUGIN_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_SPEC = importlib.util.spec_from_file_location(
    "voltaris_native_bridge_under_test",
    os.path.join(_PLUGIN_DIR, "native_bridge.py"),
)
native_bridge = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(native_bridge)


class _Board:
    def __init__(self, path):
        self.path = path

    def GetFileName(self):
        return self.path


class _Pcbnew:
    def __init__(self, board_path):
        self.board = _Board(board_path)

    def GetBoard(self):
        return self.board

    @staticmethod
    def ExportSpecctraDSN(output):
        with open(output, "w", encoding="utf-8") as stream:
            stream.write("(pcb native-test)\n")
        return True


class _Wx:
    @staticmethod
    def CallAfter(callback):
        callback()


def _request(registration, path, body=None, token=None):
    headers = {"Authorization": f"Bearer {token or registration['token']}"}
    data = None
    method = "GET"
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
        method = "POST"
    request = urllib.request.Request(
        registration["address"] + path,
        data=data,
        headers=headers,
        method=method,
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        return response.status, json.load(response)


class NativeBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.board = os.path.join(self.temp.name, "board.kicad_pcb")
        with open(self.board, "w", encoding="utf-8") as stream:
            stream.write("(kicad_pcb)\n")
        self.bridge = native_bridge.NativeSpecctraBridge(
            _Pcbnew(self.board),
            _Wx(),
            root=os.path.join(self.temp.name, "bridge"),
        )
        self.assertTrue(self.bridge.start())
        self.addCleanup(self.bridge.stop)
        with open(self.bridge.registration_path, encoding="utf-8") as stream:
            self.registration = json.load(stream)

    def test_status_and_export_require_the_registration_token(self):
        status, payload = _request(self.registration, "/v1/status")
        self.assertEqual(status, 200)
        self.assertTrue(payload["native_specctra_export"])

        with self.assertRaises(urllib.error.HTTPError) as caught:
            _request(self.registration, "/v1/status", token="wrong")
        self.assertEqual(caught.exception.code, 401)

    def test_export_uses_a_bridge_owned_path_and_reports_the_active_board(self):
        status, payload = _request(
            self.registration,
            "/v1/export-specctra-dsn",
            {"expected_board": self.board},
        )
        self.assertEqual(status, 200)
        self.assertTrue(payload["success"])
        self.assertEqual(os.path.realpath(payload["board_path"]), os.path.realpath(self.board))
        self.assertTrue(os.path.isfile(payload["dsn_path"]))
        self.assertEqual(
            os.path.commonpath([payload["dsn_path"], self.bridge.session_dir]),
            self.bridge.session_dir,
        )
        self.assertGreater(payload["dsn_bytes"], 0)

    def test_export_refuses_a_different_board(self):
        with self.assertRaises(urllib.error.HTTPError) as caught:
            _request(
                self.registration,
                "/v1/export-specctra-dsn",
                {"expected_board": os.path.join(self.temp.name, "other.kicad_pcb")},
            )
        self.assertEqual(caught.exception.code, 409)

    def test_stop_removes_registration_and_session_artifacts(self):
        registration = self.bridge.registration_path
        session = self.bridge.session_dir
        self.bridge.stop()
        self.assertFalse(os.path.exists(registration))
        self.assertFalse(os.path.exists(session))


if __name__ == "__main__":
    unittest.main()
