#!/usr/bin/env bash
#MISE description="Lint the message of each pushed commit and scan those commits for credentials"
#MISE raw=true
# pre-push runs this with git's pre-push lines on stdin, one
# `<local ref> <local sha> <remote ref> <remote sha>` per ref; raw connects
# that stdin to the task. No linter reads those lines itself: committed and
# cocogitto take one revision range, and gitleaks takes git log options, so
# this loop turns the lines into commits
# and hands them to the commits and hooks:secrets:history tasks. Their stdin is
# closed, so they cannot read the lines this loop reads.
#
# A delete (all-zero local sha) pushes nothing. Excluding every commit any
# remote-tracking ref already holds skips the default branch's history,
# GitHub squash merges included, and commits pushed before, also on a push to
# a URL or to a remote with no tracking refs yet; the remote sha covers a
# remote tip that no tracking ref names.
set -euo pipefail
zero=0000000000000000000000000000000000000000
while read -r _ lsha _ rsha; do
  [ "$lsha" = "$zero" ] && continue
  known=""
  if [ "$rsha" != "$zero" ] && git cat-file -e "$rsha^{commit}" 2>/dev/null; then
    known=$rsha
  fi
  commits=$(git rev-list "$lsha" --not --remotes $known)
  [ -n "$commits" ] || continue
  for commit in $commits; do
    mise run commits "$commit" </dev/null
  done
  # gitleaks splits --log-opts on spaces, so an empty $known must leave no
  # trailing space: git reads that as an empty revision and gitleaks scans no
  # commit.
  mise run hooks:secrets:history --log-opts="$lsha --not --remotes${known:+ $known}" </dev/null
done
