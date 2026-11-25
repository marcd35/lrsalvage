"""Exercise the app packager and deployment verifier without hosted credentials."""

import argparse
from email.message import Message
from io import BytesIO
import json
from pathlib import Path
import runpy
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.parse import unquote, urlparse
from zipfile import ZipFile


SCRIPTS = Path(__file__).resolve().parent
package_web = runpy.run_path(str(SCRIPTS / "build-browser-app.py"))["package_web"]
deployment = runpy.run_path(str(SCRIPTS / "verify-browser-deployment.py"))
ORIGIN = "https://preview.example"
CASES = {"working", "mixed", "unreadable-catalog", "unrecognized-layout", "cache-only", "legacy"}


class PackageTests(unittest.TestCase):
    def setUp(self):
        scratch = self.enterContext(TemporaryDirectory())
        root = Path(scratch).resolve()
        app = root / "app"
        self.web = app / "web"
        self.web.mkdir(parents=True)
        self.static = root / "static"
        self.git("init", "-q")
        (root / ".gitignore").write_text("app/web/.env\napp/web/pkg/\n", encoding="utf-8")
        for name in ("launch.html", "index.html", "worker.js", "test.js"):
            (self.web / name).write_text(name, encoding="utf-8")
        self.git("add", ".gitignore", "app")
        self.enterContext(patch.dict(package_web.__globals__, ROOT=root, APP=app))

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.web.parent.parent), *args], check=True)

    def test_stages_tracked_runtime_assets_only(self):
        (self.web / ".env").write_text("ignored fixture", encoding="utf-8")
        (self.web / "untracked.txt").write_text("untracked fixture", encoding="utf-8")
        (self.web / "pkg").mkdir()
        (self.web / "pkg/stale.wasm").write_bytes(b"stale build")
        self.git("check-ignore", "-q", "app/web/.env")
        package_web(self.static)
        self.assertEqual(
            {p.name for p in self.static.iterdir()},
            {"index.html", "recovery.html", "worker.js"},
        )
        self.assertEqual((self.static / "index.html").read_text(), "launch.html")
        self.assertEqual((self.static / "recovery.html").read_text(), "index.html")

    def test_rejects_tracked_symlink(self):
        link = self.web / "private.txt"
        try:
            link.symlink_to(self.web.parent.parent / ".gitignore")
        except OSError as error:
            if sys.platform == "win32" and error.winerror == 1314:
                self.skipTest("creating symlinks requires Windows Developer Mode or elevation")
            raise
        self.git("add", "app/web/private.txt")
        with self.assertRaisesRegex(SystemExit, "expected a tracked regular web file"):
            package_web(self.static)


class DeploymentTests(unittest.TestCase):
    static: Path

    def test_production_artifact(self):
        self.assertTrue((self.static / "pkg/lrsalvage_app_bg.wasm").read_bytes().startswith(b"\0asm"))
        self.assertFalse(json.loads((self.static / "build.json").read_bytes())["dirty"])
        self.assertTrue((self.static / "_headers").is_file())
        practice = self.static / "practice"
        manifest = json.loads((practice / "samples.json").read_bytes())
        self.assertEqual(set(manifest), CASES)
        legacy = manifest["legacy"]["files"]
        self.assertEqual(len(legacy), 4)
        self.assertTrue(all(file.endswith(".lrprev") for file in legacy))
        for name, case in manifest.items():
            with self.subTest(name=name), ZipFile(practice / f"{name}.zip") as archive:
                self.assertTrue(case["files"])
                self.assertEqual(set(archive.namelist()), {
                    f"{name}/{file}" for file in [*case["files"], "PHOTO-CREDITS.txt"]
                })
                for file in case["files"]:
                    self.assertEqual(archive.read(f"{name}/{file}"), (practice / name / file).read_bytes())

    def verify(self, replacements=None):
        replacements = replacements or {}

        def serve(request, *, timeout):
            del timeout
            self.assertEqual(
                request.get_header("User-agent"), "lrsalvage-deployment-verifier/1.0"
            )
            url = request.full_url
            route = unquote(urlparse(url).path).removeprefix("/")
            relative = {"": "index.html", "recovery": "recovery.html"}.get(route, route)
            file = self.static / relative
            replacement = replacements.get(relative)
            if replacement == 404:
                raise HTTPError(url, 404, "Not found", {}, None)
            mime = {".js": "text/javascript", ".wasm": "application/wasm"}.get(
                file.suffix, "application/octet-stream"
            )
            body, mime = replacement or (file.read_bytes(), mime)
            response = BytesIO(body)
            response.url = url
            response.headers = Message()
            response.headers["Content-Type"] = mime
            response.headers["Cross-Origin-Opener-Policy"] = "same-origin"
            response.headers["Cross-Origin-Embedder-Policy"] = "require-corp"
            return response

        verify = deployment["verify"]
        with patch.dict(verify.__globals__, urlopen=serve):
            verify(ORIGIN, self.static)

    def test_matches_every_staged_response(self):
        self.verify()

    def test_rejects_wrong_or_missing_assets(self):
        cases = [
            ("worker.js", ((self.static / "index.html").read_bytes(), "text/html"), "JavaScript"),
            ("practice/samples.json", (b"{}", "application/json"), "differs from the staged artifact"),
            ("practice/working.zip", 404, "404"),
            ("pkg/lrsalvage_app_bg.wasm",
             ((self.static / "pkg/lrsalvage_app_bg.wasm").read_bytes(), "text/plain"),
             "application/wasm"),
        ]
        for file, replacement, message in cases:
            with self.subTest(file=file):
                with self.assertRaisesRegex((HTTPError, SystemExit), message) as failure:
                    self.verify({file: replacement})
                if isinstance(failure.exception, HTTPError):
                    failure.exception.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("static", type=Path)
    DeploymentTests.static = parser.parse_args().static.resolve()
    unittest.main(argv=[sys.argv[0]], verbosity=2)
