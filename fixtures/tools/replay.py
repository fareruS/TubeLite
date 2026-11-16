#!/usr/bin/env python3
"""Phase 0 replay tool: re-send a saved InnerTube request with fresh auth.

Reads a saved request file (JSON: method, url, headers, body), takes the
user's browser cookies from a local file, computes a fresh SAPISIDHASH for
the current time, and sends the request with Cookie + Authorization headers.

Secrets policy:
- The cookie file and the request file's Cookie/Authorization headers are
  NEVER printed, logged, or written to the output. --dry-run prints header
  NAMES only, never values.
- The computed SAPISIDHASH is itself a usable auth token for its window:
  it is never printed either, only that it was set.
- The response is saved verbatim to --out (which must be under fixtures/raw/,
  a gitignored directory). Redact it with redact.py before it goes anywhere
  else.

Request file format (example):
    {
      "method": "POST",
      "url": "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false",
      "headers": {
        "Content-Type": "application/json",
        "Origin": "https://www.youtube.com",
        "User-Agent": "Mozilla/5.0 ..."
      },
      "body": {"context": {"client": {"clientName": "WEB", ...}}, "browseId": "..."}
    }
Cookie / Authorization headers in the request file, if any, are IGNORED and
replaced with freshly computed ones.

Usage:
    python3 replay.py --request fixtures/raw/home.request.json \
        --cookies fixtures/raw/cookies.txt \
        --out fixtures/raw/home.response.json
    python3 replay.py --request ... --cookies ... --dry-run
"""

import argparse
import hashlib
import json
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from urllib.parse import urlparse

ORIGIN = "https://www.youtube.com"
SENSITIVE_HEADERS = {"cookie", "authorization"}
# Headers that must never be re-sent from a capture: HTTP/2 pseudo-headers
# (":authority", ":method", ...) and ones urllib manages or that belong to
# the original response/transport, not the request. accept-encoding is
# dropped too, so the server answers plain JSON instead of Brotli.
DROP_HEADERS = {
    "content-length",
    "content-encoding",
    "accept-encoding",
    "transfer-encoding",
    "connection",
    "host",
    # x-goog-authuser names the signed-in account index from the capture.
    # If the cookies came from a different account, a stale index points
    # the server at the wrong account; the cookies alone identify it.
    "x-goog-authuser",
}


def sapisid_hash(sapisid: str, origin: str, timestamp_unix: int) -> str:
    """Return "<ts>_<sha1_hex('<ts> <sapisid> <origin>')>" (InnerTube auth)."""
    payload = f"{timestamp_unix} {sapisid} {origin}"
    digest = hashlib.sha1(payload.encode("utf-8")).hexdigest()
    return f"{timestamp_unix}_{digest}"


def extract_sapisid(cookie_header: str) -> str:
    """Extract the SAPISID cookie value from a Cookie header string."""
    match = re.search(r"(?:^|;\s*)SAPISID=([^;]+)", cookie_header)
    if not match:
        raise ValueError("no SAPISID cookie found in cookie file")
    return match.group(1).strip()


def load_cookie_header(path: Path, host: str = "") -> str:
    """Read cookies from a file, as a Cookie header string.

    Accepts either a raw "name=value; name2=value2" header, or a Netscape
    cookies.txt export (the "Get cookies.txt LOCALLY" extension format):
    tab-separated lines "domain flag path secure expiry name value".
    For Netscape files only cookies that apply to `host` are kept, so an
    export covering every site still sends just this site's cookies.
    """
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        raise ValueError(f"cookie file is empty: {path}")
    if text.startswith("# Netscape"):
        pairs = []
        seen_names = set()
        for line in text.splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            fields = line.split("\t")
            if len(fields) < 7:
                continue
            domain, _flag, _path, _secure, _expiry, name, value = fields[:7]
            base = domain.lstrip(".")
            if host and not (host == base or host.endswith("." + base)):
                continue
            # Exports can list the same cookie several times (multiple
            # stores/profiles) with different values — a browser never
            # sends that, and conflicting values break session binding,
            # so keep the first occurrence of each name only.
            if name in seen_names:
                continue
            seen_names.add(name)
            pairs.append(f"{name}={value}")
        if not pairs:
            raise ValueError(f"no cookies for {host or 'this host'} in Netscape file")
        return "; ".join(pairs)
    # Tolerate a pasted "Cookie: ..." line.
    if text.lower().startswith("cookie:"):
        text = text[len("cookie:"):].strip()
    # Tolerate a multi-line paste: join continuation lines.
    return " ".join(text.splitlines()).strip()


def build_headers(request_headers: dict, cookie_header: str, auth_value: str) -> dict:
    """Request headers with any stored Cookie/Authorization replaced."""
    headers = {
        k: v for k, v in request_headers.items()
        if k.lower() not in SENSITIVE_HEADERS
        and k.lower() not in DROP_HEADERS
        and not k.startswith(":")
    }
    headers["Cookie"] = cookie_header
    headers["Authorization"] = f"SAPISIDHASH {auth_value}"
    return headers


def response_summary(raw: bytes) -> dict:
    """Small, secret-free summary of a response body."""
    summary = {"bytes": len(raw)}
    try:
        parsed = json.loads(raw.decode("utf-8"))
    except (ValueError, UnicodeDecodeError):
        summary["json"] = False
        return summary
    summary["json"] = True
    if isinstance(parsed, dict):
        summary["top_level_keys"] = sorted(parsed.keys())
    elif isinstance(parsed, list):
        summary["top_level"] = f"list[{len(parsed)}]"
    return summary


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", required=True, help="saved request JSON file")
    parser.add_argument("--cookies", required=True, help="cookie header file (gitignored)")
    parser.add_argument("--out", help="where to save the raw response body")
    parser.add_argument("--timestamp", type=int, default=None,
                        help="override epoch seconds (tests only)")
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--dry-run", action="store_true",
                        help="print what would be sent (header names only), send nothing")
    args = parser.parse_args(argv)

    try:
        request = json.loads(Path(args.request).read_text(encoding="utf-8"))
        host = urlparse(request["url"]).hostname or ""
        cookie_header = load_cookie_header(Path(args.cookies), host)
        sapisid = extract_sapisid(cookie_header)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"replay: error: {exc}", file=sys.stderr)
        return 2

    timestamp = args.timestamp if args.timestamp is not None else int(time.time())
    auth_value = sapisid_hash(sapisid, ORIGIN, timestamp)

    method = request.get("method", "POST").upper()
    url = request["url"]
    headers = build_headers(request.get("headers", {}), cookie_header, auth_value)
    body = request.get("body")
    data = json.dumps(body).encode("utf-8") if body is not None else None

    if args.dry_run:
        print(f"method: {method}")
        print(f"url: {url}")
        print(f"headers: {sorted(headers.keys())} (values redacted)")
        print("auth: SAPISIDHASH set (value redacted)")
        if body is not None and isinstance(body, dict):
            print(f"body top-level keys: {sorted(body.keys())}")
        return 0

    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=args.timeout) as resp:
            raw = resp.read()
            status = resp.status
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        status = exc.code
        print(f"replay: HTTP {status} (body saved, if --out given)", file=sys.stderr)
    except (urllib.error.URLError, TimeoutError) as exc:
        print(f"replay: network error: {exc}", file=sys.stderr)
        return 1

    if args.out:
        out_path = Path(args.out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(raw)

    print(f"status: {status}")
    print(f"summary: {json.dumps(response_summary(raw))}")
    if args.out:
        print(f"saved: {args.out} (raw — redact before sharing)")
    return 0 if status and 200 <= status < 300 else 1


if __name__ == "__main__":
    raise SystemExit(main())
