#!/usr/bin/env bash
#MISE description="Markdown lint"
#MISE tools = { rumdl = "0.3.5" }
# With files as arguments, it lints only those.
# The exclude of .rumdl.toml still applies to them.
# rumdl lints any file it is given, so the caller passes Markdown files only.
rumdl check "${@:-.}"
