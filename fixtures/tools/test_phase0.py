#!/usr/bin/env python3
"""Offline tests for Phase 0 tools (replay.py, redact.py). No network beyond
a local 127.0.0.1 mock server. Run: python3 test_phase0.py"""

import hashlib
import http.server
import json
import re
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))

import redact as redact_mod
import replay as replay_mod


class TestSapisidHash(unittest.TestCase):
    def test_known_vector_matches_rust_test(self):
        # Same vector as crates/yt-source/src/innertube.rs
        # (Python hashlib.sha1("1700000000 test_sapisid https://www.youtube.com")).
        self.assertEqual(
            replay_mod.sapisid_hash("test_sapisid", "https://www.youtube.com", 1_700_000_000),
            "1700000000_34562bc6921a7e4218ebf0fdb6bd88a734cd8ae8",
        )

    def test_extract_sapisid(self):
        self.assertEqual(
            replay_mod.extract_sapisid("VISITOR=abc; SAPISID=sekrit123; SID=xyz"),
            "sekrit123",
        )
        with self.assertRaises(ValueError):
            replay_mod.extract_sapisid("VISITOR=abc; SID=xyz")


class TestRedact(unittest.TestCase):
    def test_dirty_fixture_is_cleaned(self):
        dirty = {
            "contents": {"items": [{"title": "Hello"}]},
            "responseContext": {"visitorData": "Cgtz1234SECRET"},
            "headers": {"Cookie": "SAPISID=sekrit; SID=xyz",
                        "Authorization": "SAPISIDHASH 1_abc"},
            "contact": "owner@example.com",
            "stream": "https://rr1---sn.googlevideo.com/videoplayback?expire=1&signature=ABCDEF",
            "myChannel": "UC_OWN_CHANNEL_ID_123",
        }
        stats = {"signed_urls": 0, "emails": 0, "extra_strings": 0}
        clean = redact_mod.redact_value(dirty, ["UC_OWN_CHANNEL_ID_123"], stats)
        text = json.dumps(clean)
        self.assertNotIn("sekrit", text)
        self.assertNotIn("Cgtz1234SECRET", text)
        self.assertNotIn("owner@example.com", text)
        self.assertNotIn("signature=", text)
        self.assertNotIn("UC_OWN_CHANNEL_ID_123", text)
        self.assertNotIn("SAPISID=", text)
        self.assertEqual(clean["contents"]["items"][0]["title"], "Hello")
        self.assertEqual(clean["headers"]["Cookie"], "REDACTED")
        self.assertEqual(clean["responseContext"]["visitorData"], "REDACTED")
        self.assertTrue(clean["stream"].startswith("https://rr1---sn.googlevideo.com/videoplayback"))
        self.assertGreater(stats["signed_urls"], 0)
        self.assertGreater(stats["emails"], 0)

    def test_cli_verification_fails_on_surviving_secret(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "dirty.json"
            src.write_text(json.dumps({"note": "SAPISID=sekrit123"}), encoding="utf-8")
            out = Path(tmp) / "clean.json"
            result = subprocess.run(
                [sys.executable, str(TOOLS / "redact.py"),
                 "--in", str(src), "--out", str(out)],
                capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(out.exists())


class TestReplayAgainstMock(unittest.TestCase):
    def test_replay_sends_fresh_auth_and_body(self):
        seen = {}

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                seen["body"] = self.rfile.read(length)
                seen["auth"] = self.headers.get("Authorization")
                seen["cookie"] = self.headers.get("Cookie")
                seen["path"] = self.path
                payload = json.dumps({"contents": {"ok": True}}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *args):
                pass

        with socketserver.TCPServer(("127.0.0.1", 0), Handler) as httpd:
            port = httpd.server_address[1]
            thread = threading.Thread(target=httpd.handle_request, daemon=True)
            thread.start()
            with tempfile.TemporaryDirectory() as tmp:
                req = Path(tmp) / "req.json"
                req.write_text(json.dumps({
                    "method": "POST",
                    "url": f"http://127.0.0.1:{port}/youtubei/v1/browse?prettyPrint=false",
                    "headers": {"Content-Type": "application/json",
                                "Origin": "https://www.youtube.com",
                                "Cookie": "STALE=should-be-replaced",
                                "Authorization": "SAPISIDHASH stale"},
                    "body": {"context": {"client": {"clientName": "WEB"}},
                             "browseId": "FEwhatsnew"},
                }), encoding="utf-8")
                cookies = Path(tmp) / "cookies.txt"
                cookies.write_text("SAPISID=test_sapisid; SID=x", encoding="utf-8")
                out = Path(tmp) / "resp.json"
                rc = replay_mod.main(["--request", str(req), "--cookies", str(cookies),
                                      "--out", str(out), "--timestamp", "1700000000"])
                thread.join(timeout=5)
                saved_response = out.read_text()
        self.assertEqual(rc, 0)
        expected = replay_mod.sapisid_hash(
            "test_sapisid", "https://www.youtube.com", 1_700_000_000)
        self.assertEqual(seen["auth"], f"SAPISIDHASH {expected}")
        self.assertEqual(seen["cookie"], "SAPISID=test_sapisid; SID=x")
        self.assertNotIn("stale", seen["auth"])
        self.assertEqual(json.loads(seen["body"])["browseId"], "FEwhatsnew")
        self.assertEqual(json.loads(saved_response)["contents"], {"ok": True})

    def test_dry_run_prints_no_secrets(self):
        with tempfile.TemporaryDirectory() as tmp:
            req = Path(tmp) / "req.json"
            req.write_text(json.dumps({
                "method": "POST", "url": "https://www.youtube.com/youtubei/v1/browse",
                "headers": {"Content-Type": "application/json"},
                "body": {"browseId": "FEwhatsnew"},
            }), encoding="utf-8")
            cookies = Path(tmp) / "cookies.txt"
            cookies.write_text("SAPISID=sekrit123; SID=x", encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(TOOLS / "replay.py"),
                 "--request", str(req), "--cookies", str(cookies), "--dry-run"],
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0)
            self.assertNotIn("sekrit123", result.stdout)
            self.assertNotIn("SAPISIDHASH 1", result.stdout)
            self.assertIn("headers:", result.stdout)


if __name__ == "__main__":
    unittest.main()
