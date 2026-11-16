#!/bin/sh
# Fake yt-dlp that fails: prints an error to stderr, exits 1.
echo "ERROR: video unavailable" >&2
exit 1
