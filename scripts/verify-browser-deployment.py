"""Check the identity and isolation headers of a deployed Pages artifact."""

import argparse
import json
from urllib.parse import urlparse
from urllib.request import urlopen


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url")
    parser.add_argument("build", type=argparse.FileType("r"))
    options = parser.parse_args()
    root = options.url.rstrip("/")
    origin = urlparse(root)
    if origin.scheme != "https" or not origin.hostname or origin.path:
        parser.error("provide the deployment's HTTPS origin")
    expected = json.load(options.build)

    def fetch(path):
        with urlopen(f"{root}/{path}", timeout=20) as response:
            final = urlparse(response.url)
            if (final.scheme, final.netloc) != (origin.scheme, origin.netloc):
                raise SystemExit(f"{path} redirected away from the deployment")
            for key, value in (
                ("Cross-Origin-Opener-Policy", "same-origin"),
                ("Cross-Origin-Embedder-Policy", "require-corp"),
            ):
                if response.headers.get(key) != value:
                    raise SystemExit(f"{path or '/'} must return {key}: {value}")
            return response.headers.get_content_type(), response.read()

    _, identity = fetch("build.json")
    if json.loads(identity) != expected or expected["dirty"]:
        raise SystemExit("the deployment did not serve the expected clean build identity")
    _, landing = fetch("")
    _, recovery = fetch("recovery?sample=working")
    if b"Try a sample" not in landing or b'id="scan"' not in recovery:
        raise SystemExit("the deployment did not serve both the guide and recovery page")
    fetch("worker.js")
    mime, wasm = fetch("pkg/lrsalvage_app_bg.wasm")
    if mime != "application/wasm" or not wasm.startswith(b"\x00asm"):
        raise SystemExit("the deployment did not serve WebAssembly with application/wasm")
    for name in json.loads(fetch("practice/samples.json")[1]):
        if not fetch(f"practice/{name}.zip")[1].startswith(b"PK\x03\x04"):
            raise SystemExit(f"the deployment did not serve the archive for {name}")
    print(f"{root} · app {expected['app']} · core {expected['core']} · {expected['commit']}")


if __name__ == "__main__":
    main()
