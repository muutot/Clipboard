"""Exercise production release download/retry logic against disposable HTTP servers.

The production downloader and session factory are compiled from their AST; no
environment credential or external release API is loaded. Requests and tqdm are
the same dependencies as scripts/requirements-sync.txt.
"""
import ast
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import os
from pathlib import Path
import re
import tempfile
import threading
import types
import unittest

import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry
from tqdm import tqdm


def load_downloader():
    source = Path(__file__).with_name("sync_release.py")
    tree = ast.parse(source.read_text(encoding="utf-8-sig"), filename=str(source))
    names = {"download_file_with_progress_and_resume", "get_robust_session"}
    functions = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    namespace = {
        "os": os, "re": re, "requests": requests, "HTTPAdapter": HTTPAdapter,
        "Retry": Retry, "PROXIES": None, "tqdm": tqdm, "NO_PROGRESS": True,
        "GITHUB_TOKEN": "", "time": types.SimpleNamespace(sleep=lambda _: None),
    }
    exec(compile(ast.Module(body=functions, type_ignores=[]), str(source), "exec"), namespace)
    namespace["http"] = namespace["get_robust_session"]()
    return namespace["download_file_with_progress_and_resume"], namespace["http"]


class DownloadIdentityTests(unittest.TestCase):
    def response_sequence(self, responses, initial=b"", max_attempts=2):
        calls = []

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                calls.append(dict(self.headers.items()))
                status, headers, body = responses[min(len(calls) - 1, len(responses) - 1)]
                self.send_response(status)
                for key, value in headers.items():
                    self.send_header(key, value)
                self.end_headers()
                try:
                    self.wfile.write(body)
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                self.close_connection = True

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        download, session = load_downloader()
        try:
            with tempfile.TemporaryDirectory(prefix="clipboard-download-ut-") as temporary:
                path = Path(temporary) / "artifact.bin"
                if initial:
                    path.write_bytes(initial)
                with contextlib.redirect_stdout(io.StringIO()):
                    result = download(f"http://127.0.0.1:{server.server_port}/artifact", str(path), path.name, max_attempts=max_attempts)
                return result, path.read_bytes() if path.exists() else b"", calls
        finally:
            session.close()
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            self.assertFalse(thread.is_alive())

    def download_changed_asset(self, strong_etag):
        chunk = 1024 * 1024
        old = b"a" * (2 * chunk)
        current = b"b" * (2 * chunk)
        calls = []

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                calls.append((self.headers.get("Range"), self.headers.get("If-Range")))
                if len(calls) == 1:
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(old)))
                    if strong_etag:
                        self.send_header("ETag", '"old"')
                    self.end_headers()
                    self.wfile.write(old[:chunk])
                    self.wfile.flush()
                    self.close_connection = True
                elif self.headers.get("Range") and not self.headers.get("If-Range"):
                    # The browser download URL now names a replacement asset.
                    self.send_response(206)
                    self.send_header("Content-Length", str(chunk))
                    self.send_header("Content-Range", f"bytes {chunk}-{len(current)-1}/{len(current)}")
                    if strong_etag:
                        self.send_header("ETag", '"new"')
                    self.end_headers()
                    self.wfile.write(current[chunk:])
                else:
                    # Correct If-Range handling: the changed entity is sent in full.
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(current)))
                    self.send_header("ETag", '"new"')
                    self.end_headers()
                    self.wfile.write(current)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        download, session = load_downloader()
        try:
            with tempfile.TemporaryDirectory(prefix="clipboard-download-ut-") as temporary:
                path = Path(temporary) / "artifact.bin"
                with contextlib.redirect_stdout(io.StringIO()):
                    result = download(f"http://127.0.0.1:{server.server_port}/artifact", str(path), path.name, max_attempts=2)
                self.assertTrue(result)
                written = path.read_bytes()
                self.assertEqual(len(written), len(current))
                self.assertEqual(hashlib.sha256(written).hexdigest(), hashlib.sha256(current).hexdigest(), "a successful download must contain one complete asset version")
            return calls
        finally:
            session.close()
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            self.assertFalse(thread.is_alive())

    def test_changed_asset_with_strong_etag_never_combines_versions(self):
        calls = self.download_changed_asset(True)
        self.assertEqual(calls, [(None, None), ("bytes=1048576-", '"old"')])

    def test_changed_asset_without_validator_restarts_instead_of_combining_versions(self):
        calls = self.download_changed_asset(False)
        self.assertEqual(calls, [(None, None), (None, None)])

    def test_unchanged_entity_resumes_the_verified_prefix(self):
        chunk = 1024 * 1024
        body = b"a" * (2 * chunk)
        result, written, calls = self.response_sequence([
            (200, {"Content-Length": str(len(body)), "ETag": '"stable"'}, body[:chunk]),
            (206, {"Content-Length": str(chunk), "ETag": '"stable"', "Content-Range": f"bytes {chunk}-{len(body)-1}/{len(body)}"}, body[chunk:]),
        ])
        self.assertTrue(result)
        self.assertEqual(hashlib.sha256(written).hexdigest(), hashlib.sha256(body).hexdigest())
        self.assertEqual(calls[1].get("Range"), "bytes=1048576-")
        self.assertEqual(calls[1].get("If-Range"), '"stable"')
        self.assertEqual(calls[0].get("Accept-Encoding"), "identity")

    def test_invalid_range_is_rejected_before_appending_unrelated_bytes(self):
        chunk = 1024 * 1024
        for range_value, etag, length in [
            (f"bytes {chunk+1}-{2*chunk-1}/{2*chunk}", '"stable"', str(chunk)),
            (f"bytes {chunk}-{2*chunk-1}/{2*chunk}", '"changed"', str(chunk)),
            (f"bytes {chunk}-{2*chunk-1}/{2*chunk}", '"stable"', str(chunk - 1)),
            (f"bytes {chunk}-{2*chunk-1}/{2*chunk+1}", '"stable"', str(chunk)),
        ]:
            with self.subTest(range_value=range_value, etag=etag, length=length):
                result, written, _ = self.response_sequence([
                    (200, {"Content-Length": str(2 * chunk), "ETag": '"stable"'}, b"a" * chunk),
                    (206, {"Content-Length": length, "ETag": etag, "Content-Range": range_value}, b"b" * chunk),
                ])
                self.assertFalse(result)
                self.assertEqual(len(written), chunk)
                self.assertEqual(hashlib.sha256(written).hexdigest(), hashlib.sha256(b"a" * chunk).hexdigest())

    def test_unsatisfied_range_restarts_instead_of_certifying_a_partial_file(self):
        chunk = 1024 * 1024
        result, written, calls = self.response_sequence([
            (200, {"Content-Length": str(2 * chunk), "ETag": '"old"'}, b"a" * chunk),
            (416, {"Content-Range": "bytes */6", "Content-Length": "0"}, b""),
            (200, {"Content-Length": "6", "ETag": '"new"'}, b"newest"),
        ], max_attempts=3)
        self.assertTrue(result)
        self.assertEqual(written, b"newest")
        self.assertNotIn("Range", calls[2])

    def test_weak_etag_and_unverified_existing_file_do_not_authorize_resume(self):
        chunk = 1024 * 1024
        result, written, calls = self.response_sequence([
            (200, {"Content-Length": str(2 * chunk), "ETag": 'W/"weak"'}, b"a" * chunk),
            (200, {"Content-Length": "6"}, b"newest"),
        ], initial=b"unverified older file")
        self.assertTrue(result)
        self.assertEqual(written, b"newest")
        self.assertTrue(all("Range" not in headers for headers in calls))

    def test_unexpected_partial_and_compressed_representations_are_not_successes(self):
        import gzip
        compressed = gzip.compress(b"plain" * 100)
        for response in [
            (206, {"Content-Range": "bytes 0-5/6", "ETag": '"new"', "Content-Length": "6"}, b"newest"),
            (200, {"Content-Length": str(len(compressed)), "Content-Encoding": "gzip"}, compressed),
            (204, {"Content-Length": "0"}, b""),
        ]:
            with self.subTest(status=response[0], headers=response[1]):
                result, written, _ = self.response_sequence([response], max_attempts=1)
                self.assertFalse(result)
                self.assertEqual(written, b"")


if __name__ == "__main__":
    unittest.main()
