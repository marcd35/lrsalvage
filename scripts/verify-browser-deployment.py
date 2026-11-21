"""Check a deployed Pages site against the staged app and isolation headers."""

import argparse
import json
from pathlib import Path
from urllib.parse import quote, urlparse
from urllib.request import urlopen


ROUTES = {"index.html": "", "recovery.html": "recovery?sample=working"}
JS_TYPES = {"text/javascript", "application/javascript"}


def verify(root, static):
    origin = urlparse(root)
    if origin.scheme != "https" or not origin.hostname or origin.path or origin.query or origin.fragment:
        raise SystemExit("provide the deployment's HTTPS origin")
    expected = json.loads((static / "build.json").read_bytes())
    if expected["dirty"]:
        raise SystemExit("the deployment must use a clean build")

    for staged in sorted(path for path in static.rglob("*") if path.is_file()):
        relative = staged.relative_to(static).as_posix()
        if relative == "_headers":
            continue
        route = ROUTES.get(relative, quote(relative))
        with urlopen(f"{root}/{route}", timeout=20) as response:
            final = urlparse(response.url)
            if (final.scheme, final.netloc) != (origin.scheme, origin.netloc):
                raise SystemExit(f"{route} redirected away from the deployment")
            for key, value in (
                ("Cross-Origin-Opener-Policy", "same-origin"),
                ("Cross-Origin-Embedder-Policy", "require-corp"),
            ):
                if response.headers.get(key) != value:
                    raise SystemExit(f"{route or '/'} must return {key}: {value}")

            mime = response.headers.get_content_type()
            if staged.suffix in (".js", ".mjs") and mime not in JS_TYPES:
                raise SystemExit(f"{relative} must be served as JavaScript, got {mime}")
            if staged.suffix == ".wasm" and mime != "application/wasm":
                raise SystemExit(f"{relative} must be served as application/wasm, got {mime}")
            if response.read() != staged.read_bytes():
                raise SystemExit(f"{relative} differs from the staged artifact")

    print(f"{root} · app {expected['app']} · core {expected['core']} · {expected['commit']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url")
    parser.add_argument("static", type=Path, help="staged static directory")
    options = parser.parse_args()
    verify(options.url.rstrip("/"), options.static)


if __name__ == "__main__":
    main()
