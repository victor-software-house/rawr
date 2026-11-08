#!/bin/sh
#MISE description="Fail when a mise lockfile no longer matches its config"
# pre-push runs this. `mise lock --dry-run` writes and installs nothing, and
# prints [] when every lock is current; `mise install --locked` would download
# a missing tool instead.
changes=$(mise lock --dry-run --json) || { echo "✘ mise lock --dry-run failed; see its error above"; exit 1; }
[ "$changes" = "[]" ] || { printf '%s\n' "$changes"; echo "✘ a mise lockfile is stale: run mise lock"; exit 1; }
