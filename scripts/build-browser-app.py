"""Package the release page, WebAssembly and practice vaults for a static host."""

import argparse
from hashlib import sha256
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
from zipfile import ZIP_STORED, ZipFile, ZipInfo


ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "crates/lrsalvage-app"
TARGET = "wasm32-unknown-unknown"


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def read(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def write_json(path, document):
    path.write_bytes((json.dumps(document, indent=2, sort_keys=True) + "\n").encode())


def add_zip_file(archive, path, data):
    # Fixed time, mode and file order keep the archives independent of the host.
    info = ZipInfo(path)
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    archive.writestr(info, data)


def package_samples(static):
    practice = static / "practice"
    run("cargo", "run", "--quiet", "--locked", "-p", "lrsalvage-app", "--bin",
        "vault-practice", "--", str(practice))
    credits = (APP / "practice/credits.txt").read_bytes().replace(b"\r\n", b"\n")
    (practice / "credits.txt").write_bytes(credits)
    manifest = {}
    for folder in sorted(child for child in practice.iterdir() if child.is_dir()):
        paths = sorted(path for path in folder.rglob("*") if path.is_file())
        files = [path.relative_to(folder).as_posix() for path in paths]
        revision = sha256()
        with ZipFile(practice / f"{folder.name}.zip", "w", ZIP_STORED) as archive:
            for relative, path in zip(files, paths, strict=True):
                data = path.read_bytes()
                revision.update(relative.encode() + b"\0" + sha256(data).digest())
                add_zip_file(archive, f"{folder.name}/{relative}", data)
            add_zip_file(archive, f"{folder.name}/PHOTO-CREDITS.txt", credits)
        manifest[folder.name] = {"files": files, "revision": revision.hexdigest()}
    write_json(practice / "samples.json", manifest)
    shutil.copyfile(APP / "practice/photos/canyon.jpg", practice / "preview.jpg")


def package_web(static):
    web = APP / "web"
    tracked = subprocess.check_output(
        ["git", "ls-files", "--cached", "-z", "--", web.relative_to(ROOT).as_posix()],
        cwd=ROOT,
    ).decode("utf-8").split("\0")
    for path in filter(None, tracked):
        source = ROOT / path
        relative = source.relative_to(web)
        if relative.parts[0] == "pkg" or relative.name in ("test.html", "test.js"):
            continue
        # A tracked symlink could point to a local file outside the build inputs.
        if source.resolve() != source or not source.is_file():
            raise SystemExit(f"expected a tracked regular web file: {path}")
        target = static / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    (static / "index.html").rename(static / "recovery.html")
    (static / "launch.html").rename(static / "index.html")


def identity(metadata, tag):
    versions = {package["name"]: package["version"] for package in metadata["packages"]}
    commit = read("git", "rev-parse", "HEAD")
    dirty = bool(read("git", "status", "--porcelain"))
    app = versions["lrsalvage-app"]
    if tag and (tag != f"app-{app}" or dirty
                or read("git", "rev-parse", f"refs/tags/{tag}^{{commit}}") != commit):
        raise SystemExit(f"release requires a clean checkout of app-{app}")
    return {"app": app, "core": versions["lrsalvage-core"],
            "commit": commit, "dirty": dirty, "tag": tag}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="new output directory")
    parser.add_argument("--tag", help="verify and record the app release tag at HEAD")
    options = parser.parse_args()
    out = options.out.resolve()
    if out.exists():
        parser.error(f"output directory already exists: {out}")

    metadata = json.loads(read("cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"))
    release = identity(metadata, options.tag)
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    bindgen = next(package["version"] for package in lock["package"] if package["name"] == "wasm-bindgen")
    if read("wasm-bindgen", "--version") != f"wasm-bindgen {bindgen}":
        raise SystemExit(f"install wasm-bindgen-cli {bindgen} to match Cargo.lock")

    run("cargo", "build", "--locked", "--release", "--lib", "--target", TARGET, "-p", "lrsalvage-app")
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="app-static-", dir=out.parent) as scratch:
        static = Path(scratch) / "site"
        package_web(static)
        package_samples(static)
        run("wasm-bindgen", "--target", "web", "--no-typescript",
            "--out-dir", str(static / "pkg"), "--out-name", "lrsalvage_app",
            str(Path(metadata["target_directory"]) / TARGET / "release/lrsalvage_app.wasm"))
        write_json(static / "build.json", release)
        for licence in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(ROOT / licence, static / licence)
        static.rename(out)
    print(f"App {release['app']} / Core {release['core']} / {release['commit']}")
    print(out)


if __name__ == "__main__":
    main()
