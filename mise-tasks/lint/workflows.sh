#!/bin/sh
#MISE description="GitHub Actions workflow lint"
#MISE tools = { actionlint = "1.7.12" }
# GitHub runs only the repository root's workflows, so the task lints those
# from the root, also in a render below it, such as cli/. actionlint fails on
# a workflow folder with no file (actionlint 1.7.12, linter.go LintDir), so it
# lints the tracked workflows only: with none, as in a fresh Bun render,
# nothing runs and the check passes. :(glob) keeps * in .github/workflows
# itself, the files GitHub runs, and a failing git ls-files fails the task. -z
# hands xargs each path unquoted, whatever it holds.
cd "$(git rev-parse --show-toplevel)" || exit 1
files=$(git ls-files ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml') || exit 1
[ -z "$files" ] || git ls-files -z ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml' | xargs -0 actionlint
