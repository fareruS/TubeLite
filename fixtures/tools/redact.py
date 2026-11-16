#!/usr/bin/env python3
"""Phase 0 redaction: strip personal data from a saved InnerTube response.

Reads a raw response (or saved request) JSON file from fixtures/raw/ and
writes a redacted copy to fixtures/redacted/. Never modifies the input.

What is stripped (recursive over the whole JSON value):
- Exact keys (case-insensitive): cookie, authorization, sapisid,
  visitordata, sessiontoken, password, signaturecipher, cipher. Their
  values become "REDACTED".
- Email addresses in any string value -> "REDACTED@example.invalid".
- Signed stream URLs: any URL whose host contains googlevideo.com, or any
  URL with a "signature=" query parameter, has its query string removed
  (signatures live in the query). Host + path are kept so itag/mime
  structure stays visible. A percent-encoded signed URL (e.g. the `url=`
  inside a `signatureCipher`, where the whole query is `%3F…%3D…`) is
  redacted wholesale — decoding it only to check would still leave a live
  signed URL in the output.

Verification: after redacting, the output is re-scanned for every extra
string, any email address, and any "SAPISID=" / "Cookie:" / "Authorization:"
literal, plus any surviving "expire=" / "expire%3D" (a signed stream URL
that slipped through). If anything is found, the script exits non-zero and
does NOT write the output file.
- Extra sensitive strings listed one per line in --extra (a gitignored file,
  e.g. fixtures/raw/redact-strings.txt holding your own channel ID, account
  name, etc.): every occurrence is replaced with "REDACTED". This is how
  personal channel IDs are removed — the script cannot guess which public
  channel IDs are yours.

Verification: after redacting, the output is re-scanned for every extra
string, any email address, and any "SAPISID=" / "Cookie:" / "Authorization:"
literal. If anything is found, the script exits non-zero and does NOT write
the output file.

Usage:
    python3 redact.py --in fixtures/raw/home.response.json \
        --out fixtures/redacted/home.json
    python3 redact.py --in fixtures/raw/home.response.json \
        --out fixtures/redacted/home.json --extra fixtures/raw/redact-strings.txt
"""

import argparse
import json
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit, urlunsplit

SENSITIVE_KEYS = {
    "cookie",
    "authorization",
    "sapisid",
    "visitordata",
    "x-goog-visitor-id",
    "sessiontoken",
    "password",
    "accountname",
    "accountemail",
    "email",
    "accountphoto",
    # Client-context fingerprinting / session data in request bodies.
    "remotehost",
    "configinfo",
    "deviceexperimentid",
    "rollouttoken",
    "x-client-data",
    # Cipher blobs are percent-encoded signed stream URLs (the `s=` decipher
    # signature plus a signed `url=`). The itag/mime structure lives in
    # sibling fields, so the whole value is redacted, never kept.
    "signaturecipher",
    "cipher",
}

EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
# Backstop: any surviving signed stream URL. A plain `expire=` cannot
# survive query-stripping, and an encoded one only lives inside a cipher
# that the key rule above must have caught.
FORBIDDEN_LITERALS = ("SAPISID=", "Cookie:", "Authorization:", "expire=", "expire%3D")


def is_percent_encoded_signed_url(value: str) -> bool:
    """True when the value is a percent-encoded signed stream URL.

    Decoding it reveals a googlevideo host carrying an expiry or signature
    parameter, and there is no literal `?` for the plain query-strip to cut
    at. Such a value is a live signed URL even though no plain `?` or
    `googlevideo.com` appears in it, so it must be redacted wholesale.
    A plain signed URL *with* a literal `?` is left to `strip_signed_url`,
    which removes its query and keeps the harmless host + path.
    """
    if "%" not in value or "?" in value:
        return False
    decoded = unquote(value).lower()
    return "googlevideo.com" in decoded and (
        "expire=" in decoded or "signature=" in decoded or "sig=" in decoded
    )


def strip_signed_url(value: str) -> str:
    """Drop the query string of signed/stream URLs, keep host + path."""
    lowered = value.lower()
    if "googlevideo.com" in lowered or "signature=" in lowered:
        parts = urlsplit(value)
        if parts.query:
            return urlunsplit((parts.scheme, parts.netloc, parts.path, "", ""))
    return value


def redact_value(value, extra_strings, stats):
    if isinstance(value, dict):
        return {
            key: (
                "REDACTED"
                if key.lower() in SENSITIVE_KEYS
                else redact_value(val, extra_strings, stats)
            )
            for key, val in value.items()
        }
    if isinstance(value, list):
        return [redact_value(item, extra_strings, stats) for item in value]
    if isinstance(value, str):
        # A percent-encoded signed URL is a live secret in disguise: redact
        # the whole value instead of trying to strip a query that isn't visible.
        if is_percent_encoded_signed_url(value):
            stats["signed_urls"] += 1
            return "REDACTED"
        original = value
        value = strip_signed_url(value)
        if value != original:
            stats["signed_urls"] += 1
        # Fast pre-filter: the email regex is quadratic on long token
        # strings without '@' (continuation tokens, hashes), so only run
        # it when '@' is present at all.
        if "@" in value and EMAIL_RE.search(value):
            value = EMAIL_RE.sub("REDACTED@example.invalid", value)
            stats["emails"] += 1
        for secret in extra_strings:
            if secret and secret in value:
                value = value.replace(secret, "REDACTED")
                stats["extra_strings"] += 1
        return value
    return value


def count_sensitive_keys(value) -> int:
    """Count exact sensitive keys (for the report) before redaction."""
    if isinstance(value, dict):
        return sum(
            (1 if key.lower() in SENSITIVE_KEYS else 0) + count_sensitive_keys(v)
            for key, v in value.items()
        )
    if isinstance(value, list):
        return sum(count_sensitive_keys(item) for item in value)
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--in", dest="input", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--extra", help="gitignored file, one sensitive string per line")
    args = parser.parse_args(argv)

    try:
        parsed = json.loads(Path(args.input).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"redact: error: {exc}", file=sys.stderr)
        return 2

    extra_strings = []
    if args.extra:
        extra_path = Path(args.extra)
        if extra_path.exists():
            extra_strings = sorted(
                (line.strip() for line in extra_path.read_text(encoding="utf-8").splitlines()
                 if line.strip()),
                key=len, reverse=True,
            )

    stats = {"sensitive_keys": count_sensitive_keys(parsed),
             "signed_urls": 0, "emails": 0, "extra_strings": 0}
    redacted = redact_value(parsed, extra_strings, stats)

    # Verification pass: nothing sensitive may survive in the output text.
    text = json.dumps(redacted, ensure_ascii=False)
    problems = []
    for secret in extra_strings:
        if secret and secret in text:
            problems.append("extra sensitive string still present")
            break
    # Email check, windowed around each '@' so a multi-MB dump stays fast.
    # The redaction placeholder itself is email-shaped, so exclude it —
    # otherwise a file is flagged for its own replacement.
    check_text = text.replace("REDACTED@example.invalid", "")
    if "@" in check_text:
        for at in re.finditer(r"@", check_text):
            window = check_text[max(0, at.start() - 64):at.end() + 64]
            if EMAIL_RE.search(window):
                problems.append("email address still present")
                break
    for literal in FORBIDDEN_LITERALS:
        if literal in text:
            problems.append(f"literal {literal!r} still present")
    if problems:
        print(f"redact: FAILED verification: {'; '.join(problems)}", file=sys.stderr)
        return 1

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(redacted, ensure_ascii=False, indent=2) + "\n",
                        encoding="utf-8")
    print(f"redacted: {args.input} -> {args.out}")
    print(f"stats: {json.dumps(stats)}")
    print("verification: passed (no extra strings, emails, or auth literals)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
