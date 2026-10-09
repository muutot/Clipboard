"""Exercise the production upload orchestration with synthetic HTTP responses.

Compile the production functions from their AST so this boundary regression needs
only the standard library. Requests session setup/retry configuration is outside
this test; no release API, token, or real upload is used.
"""
import ast
import contextlib
import io
import os
from pathlib import Path
import tempfile
import types
import unittest
from urllib.parse import urlsplit
from unittest.mock import Mock


def load_upload_functions(http):
    source = Path(__file__).with_name("sync_release.py")
    tree = ast.parse(source.read_text(encoding="utf-8-sig"), filename=str(source))
    names = {"upload_asset_to_gitcode_by_tag", "upload_headers_for", "url_origin"}
    functions = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    namespace = {
        "os": os, "urlsplit": urlsplit, "http": http,
        "GITCODE_API_BASE": "https://gitcode.example/api/v5",
        "GITCODE_OWNER": "fixture", "GITCODE_REPO": "fixture",
        "GITCODE_TOKEN": "synthetic-api-credential",
    }
    exec(compile(ast.Module(body=functions, type_ignores=[]), str(source), "exec"), namespace)
    return namespace["upload_asset_to_gitcode_by_tag"]


class UploadBoundaryTests(unittest.TestCase):
    def upload(self, info):
        http = Mock()
        http.get.return_value = types.SimpleNamespace(status_code=200, json=lambda: info)
        http.put.return_value = types.SimpleNamespace(status_code=200)
        upload = load_upload_functions(http)
        with tempfile.TemporaryDirectory(prefix="clipboard-upload-test-") as temporary:
            path = Path(temporary) / "artifact.zip"
            path.write_bytes(b"synthetic asset")
            with contextlib.redirect_stdout(io.StringIO()):
                result = upload("v1.2.3", str(path), path.name)
        return result, http

    def test_external_presigned_upload_does_not_receive_the_api_token(self):
        success, http = self.upload({
            "upload_url": "https://objects.example/asset?signature=fixture",
            "headers": {"Content-Type": "application/octet-stream", "X-Obs-Test": "fixture"},
        })
        self.assertTrue(success)
        headers = http.put.call_args.kwargs["headers"]
        self.assertFalse(any(key.lower() == "authorization" for key in headers))
        self.assertEqual(headers["X-Obs-Test"], "fixture")
        self.assertEqual(http.get.call_args.kwargs["headers"]["Authorization"], "Bearer synthetic-api-credential")

    def test_same_origin_api_upload_can_use_the_api_token(self):
        success, http = self.upload("https://GITCODE.example:443/api/v5/upload")
        self.assertTrue(success)
        self.assertEqual(http.put.call_args.kwargs["headers"]["Authorization"], "Bearer synthetic-api-credential")

    def test_external_explicit_upload_authorization_is_preserved(self):
        success, http = self.upload({
            "upload_url": "https://objects.example/asset",
            "headers": {"authorization": "OBS fixture-signed-authorization"},
        })
        self.assertTrue(success)
        self.assertEqual(http.put.call_args.kwargs["headers"], {"authorization": "OBS fixture-signed-authorization"})

    def test_same_origin_explicit_authorization_is_preserved(self):
        success, http = self.upload({
            "url": "https://gitcode.example/upload",
            "headers": {"authorization": "OBS fixture"},
        })
        self.assertTrue(success)
        self.assertEqual(http.put.call_args.kwargs["headers"], {"authorization": "OBS fixture"})

    def test_different_port_does_not_receive_api_credentials(self):
        success, http = self.upload("https://gitcode.example:444/upload")
        self.assertTrue(success)
        self.assertEqual(http.put.call_args.kwargs["headers"], {})

    def test_invalid_upload_destinations_are_rejected_before_put(self):
        for url in ("http://objects.example/asset", "/upload", "https://user@objects.example/a", "https://objects.example:invalid/a"):
            with self.subTest(url=url):
                success, http = self.upload(url)
                self.assertFalse(success)
                http.put.assert_not_called()


if __name__ == "__main__":
    unittest.main()
