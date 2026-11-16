#!/bin/sh
# Fake yt-dlp for tests: prints the extractor fixture, ignores args.
# Usage: fake-ytdlp.sh <any args>  (exit 0)
cat "$(dirname "$0")/extractor.json"
