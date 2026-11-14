#!/usr/bin/env bash
#MISE description="Fail a push of commits the checkout does not hold, which the file checks read"
#MISE raw=true
# The pre-push file checks lint lefthook's {push_files} as the working tree
# holds them, so they check what the push sends only when the checkout holds
# the pushed commit. pre-push runs this with git's pre-push lines on stdin,
# one `<local ref> <local sha> <remote ref> <remote sha>` per ref. raw
# connects that stdin to the task.
#
# A ref whose commits a remote-tracking ref or the remote sha already holds,
# such as a tag on a merged commit, sends nothing new and is skipped, as in
# hooks:pushed. A delete, an all-zero local sha, sends nothing either. Every
# other ref must point at HEAD, and no tracked file may differ from HEAD in
# the working tree. An untracked file is in no commit and is not checked.
# The loop also reads a last line with no newline, as a caller may send.
set -euo pipefail
head=$(git rev-parse HEAD)
sends=""
while read -r lref lsha _ rsha || [ -n "${lref:-}" ]; do
  case $lsha in *[!0]*) ;; *) continue ;; esac
  known=""
  if [[ $rsha == *[!0]* ]] && git cat-file -e "$rsha^{commit}" 2>/dev/null; then
    known=$rsha
  fi
  # An assignment, so set -e stops the push when git rev-list fails.
  new=$(git rev-list -n 1 "$lsha" --not --remotes $known)
  [ -n "$new" ] || continue
  if [ "$(git rev-parse "$lsha^{commit}")" != "$head" ]; then
    echo "$lref is not the checked-out commit, and the pre-push file checks read the checkout." >&2
    echo "Check out $lref, then push it." >&2
    exit 1
  fi
  sends=1
done
[ -n "$sends" ] || exit 0
changed=$(git diff --name-only HEAD)
if [ -n "$changed" ]; then
  echo "These tracked files differ from the pushed commit, which the file checks do not read:" >&2
  printf '%s\n' "$changed" >&2
  echo "Commit or restore them, then push again." >&2
  exit 1
fi
