#!/bin/sh
#MISE description="GitHub Actions workflow lint"
#MISE tools = { jactionlint = "2.6.0", "aqua:koalaman/shellcheck" = "0.11.0" }
# jactionlint runs shellcheck on every run: script, from the search path. The
# task pins it, so each machine and CI check the scripts with the same version.
# GitHub runs only the workflows at the repository root.
# The task lints those from the root, even in a render below it, such as cli/.
#
# jactionlint exits 3 on a workflow folder with no file (jactionlint 2.5.0).
# The task therefore lints the tracked workflows only.
# With none, as in a fresh Bun render, nothing runs and the check passes.
#
# :(glob) keeps * in .github/workflows itself, the files GitHub runs. A failing
# git ls-files fails the task. -z hands xargs each path unquoted, whatever it
# contains.
cd "$(git rev-parse --show-toplevel)" || exit 1
files=$(git ls-files ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml') || exit 1
[ -n "$files" ] || exit 0
git ls-files -z ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml' |
  xargs -0 jactionlint
