# Measurement protocol (Windows 11). See scripts/measure.sh for the Linux twin.
# Samples the sum of Private Bytes of the app process + every child process
# (including all msedgewebview2.exe instances) every 5 s.
# Scenarios: idle 2 min, play 1080p 5 min, open a 500-row playlist and scroll,
# hide to tray 2 min, mini player 2 min, 30-minute soak.
# Writes CSV + a summary table to docs/measurements/<date>-<commit>.md
# Not yet run — no Windows release build exists in Phase 1.
param([string]$Out = "docs/measurements/$(Get-Date -Format yyyy-MM-dd)-unknown.csv")
Write-Host "No release build exists yet (Phase 1). Run after Phase 2 playback lands."
