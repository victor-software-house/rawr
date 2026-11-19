#!/usr/bin/env bash
#MISE description="TOML formatting check"
#MISE tools = { taplo = "0.10.0" }
# taplo walks every directory below the root and follows links, so a tracked
# link to the repository root, such as a benchmark package's path dependency,
# makes it walk the repository again inside the link until the path is too
# long. The task hands it the tracked TOML files that are regular files, and
# .taplo.toml's exclude still applies to them. The list goes through a file
# so a git failure stops the task. With files as arguments, the task reads
# those instead of the tracked ones, and the exclude applies to them too.
#
# taplo reads each argument as a glob, so x[ab].toml would match xa.toml and
# miss itself. Each glob character is wrapped in brackets, which matches it
# literally.
set -euo pipefail
tracked=$(mktemp)
trap 'rm -f "$tracked"' EXIT
if [ "$#" -gt 0 ]; then
  printf '%s\0' "$@" >"$tracked"
else
  git ls-files -z ':(glob)**/*.toml' >"$tracked"
fi
files=()
while IFS= read -r -d '' file; do
  [ -f "$file" ] || continue
  literal=""
  for ((i = 0; i < ${#file}; i++)); do
    char=${file:i:1}
    case $char in
      '[' | ']' | '*' | '?' | '{' | '}') literal+="[$char]" ;;
      *) literal+=$char ;;
    esac
  done
  files+=("$literal")
done <"$tracked"
if [ "${#files[@]}" -gt 0 ]; then
  taplo fmt --check "${files[@]}"
fi
