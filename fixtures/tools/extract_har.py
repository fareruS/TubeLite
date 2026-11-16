#!/usr/bin/env python3
"""Extract youtubei requests/responses from a HAR file into fixtures/raw/.

Reads a HAR exported from DevTools Network ("Save all as HAR with content"),
picks the POST entries to youtubei/v1/* (skipping log_event telemetry
beacons), and writes per-entry request/response JSON files plus the Cookie
header (once) to cookies.txt for the replay tool.

Secrets policy:
- Cookie / Authorization header VALUES are never printed and never written
  into the request files. The Cookie value goes only to fixtures/raw/
  cookies.txt (gitignored).
- The summary prints endpoint, identifier, client name/version, status and
  byte counts only — never header values, never body content.
- Personal identifiers found under known account paths (e.g. the guide
  response's userProfile) are collected into fixtures/raw/redact-strings.txt
  (gitignored) WITHOUT being printed, so redact.py can strip them.

Usage:
    python3 extract_har.py --har fixtures/raw/homehar.har --map browse:FEwhat_to_watch:home
    python3 extract_har.py --har fixtures/raw/subcription.har \
        --map browse:UC8zZ084em5pohjs6kk2Bpaw:channel \
        --map player:vCsU3aB5Tqg:player --map navigation/resolve_url:*:resolve_url
    Unmapped entries are written as <har-stem>-<endpoint>-<n>.request.json.
"""

import argparse
import base64
import json
import sys
from pathlib import Path
from urllib.parse import urlparse

SENSITIVE_HEADERS = {"cookie", "authorization", "x-goog-visitor-id"}


def decode_content(content: dict) -> bytes:
    text = content.get("text", "")
    if content.get("encoding") == "base64":
        return base64.b64decode(text)
    return text.encode("utf-8")


def entry_ident(body: dict) -> str:
    if not isinstance(body, dict):
        return ""
    for key in ("browseId", "query", "videoId"):
        if body.get(key):
            return str(body[key])
    if "continuation" in body:
        return "continuation"
    return ""


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--har", required=True)
    parser.add_argument("--out-dir", default=None,
                        help="default: fixtures/raw next to this script")
    parser.add_argument("--map", action="append", default=[],
                        help="endpoint:ident:page (ident may be *)")
    args = parser.parse_args(argv)

    har_path = Path(args.har)
    out_dir = (Path(args.out_dir) if args.out_dir else
               Path(__file__).resolve().parent.parent / "raw")
    out_dir.mkdir(parents=True, exist_ok=True)

    mapping = {}
    for spec in args.map:
        try:
            endpoint, ident, page = spec.split(":", 2)
        except ValueError:
            print(f"extract: bad --map {spec!r} (want endpoint:ident:page)", file=sys.stderr)
            return 2
        mapping[(endpoint, ident)] = page

    har = json.loads(har_path.read_text(encoding="utf-8"))
    entries = har["log"]["entries"]
    stem = har_path.stem
    cookies_path = out_dir / "cookies.txt"
    strings_path = out_dir / "redact-strings.txt"
    extra_strings = set()
    if strings_path.exists():
        extra_strings.update(
            line.strip() for line in strings_path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        )
    header_names = set()
    picked = []
    auto_n = 0
    for entry in entries:
        req = entry["request"]
        url = urlparse(req["url"])
        if "youtubei/v1/" not in url.path:
            continue
        endpoint = url.path.split("/v1/", 1)[1]
        if endpoint == "log_event":
            continue
        if req.get("method", "").upper() != "POST":
            continue
        try:
            body = json.loads(req.get("postData", {}).get("text", "{}"))
        except (ValueError, TypeError):
            body = {}
        ident = entry_ident(body)
        page = mapping.get((endpoint, ident)) or mapping.get((endpoint, "*"))
        if page is None:
            auto_n += 1
            page = f"{stem}-{endpoint}-{auto_n}"
        headers = {}
        cookie_value = None
        # Chrome's HAR export puts cookies in a separate "cookies" array,
        # not in the headers list.
        for c in req.get("cookies", []):
            if c.get("name") == "SAPISID":
                cookie_value = "; ".join(
                    f"{x.get('name')}={x.get('value')}" for x in req["cookies"])
                break
        for h in req.get("headers", []):
            name = h.get("name", "")
            header_names.add(name)
            if name.lower() in SENSITIVE_HEADERS:
                continue
            headers[name] = h.get("value", "")
        if cookie_value and "SAPISID=" in cookie_value and not cookies_path.exists():
            cookies_path.write_text(cookie_value, encoding="utf-8")
        client = body.get("context", {}).get("client", {}) if isinstance(body, dict) else {}
        status = entry["response"]["status"]
        raw = decode_content(entry["response"].get("content", {}))
        try:
            response_json = json.loads(raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            print(f"extract: WARNING {page}: response is not JSON ({len(raw)} bytes), skipped",
                  file=sys.stderr)
            continue
        # Collect personal identifiers (values never printed).
        profile = response_json.get("userProfile") if isinstance(response_json, dict) else None
        if isinstance(profile, dict):
            for key in ("channelId", "accountName", "accountEmail", "email", "accountPhoto"):
                value = profile.get(key)
                # Only long, distinctive values: short names would corrupt
                # unrelated text if replaced globally.
                if value and len(str(value)) >= 12:
                    extra_strings.add(str(value))
        (out_dir / f"{page}.request.json").write_text(
            json.dumps({"method": req.get("method", "POST"), "url": req["url"],
                        "headers": headers, "body": body},
                       ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        (out_dir / f"{page}.response.json").write_text(
            json.dumps(response_json, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        resp_keys = sorted(response_json.keys()) if isinstance(response_json, dict) else []
        picked.append((page, endpoint, ident, client.get("clientName"),
                       client.get("clientVersion"), status, len(raw), resp_keys))
    if extra_strings:
        strings_path.write_text("\n".join(sorted(extra_strings)) + "\n", encoding="utf-8")
    for page, endpoint, ident, cn, cv, status, nbytes, rkeys in picked:
        print(f"picked: {page} endpoint={endpoint} id={ident!r} client={cn} {cv} "
              f"status={status} resp_bytes={nbytes} resp_keys={rkeys}")
    print(f"header names seen: {sorted(header_names)}")
    print(f"cookies file: {'written' if cookies_path.exists() else 'not written (no SAPISID cookie seen)'}")
    print(f"personal strings collected for redaction: {len(extra_strings)} (values not shown)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
