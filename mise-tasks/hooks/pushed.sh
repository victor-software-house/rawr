#!/usr/bin/env bash
#MISE description="Lint the message of each pushed commit and scan those commits for credentials"
#MISE raw=true
# pre-push runs this with git's pre-push lines on stdin, one
# `<local ref> <local sha> <remote ref> <remote sha>` per ref.
# raw connects that stdin to the task.
#
# No linter reads those lines itself, so this task turns them into the
# commits each ref pushes.
# It runs committed and gitleaks itself, both from mise.dev.toml.
# It starts no other mise process: a mise run for each commit cost about
# 60 ms against 3.5 ms for committed alone (templates#230).
#
# The gitleaks flags match hooks:secrets:history, which CI and a hand check run.
# COMMITTED_CONFIG points to a committed.toml kept outside the repository root.
#
# A pushed commit is one that no remote-tracking ref contains yet.
# That skips the history of the default branch, GitHub squash merges included,
# and commits pushed before.
# It also works on a push to a URL, or to a remote with no tracking refs yet.
# The remote sha covers a remote tip that no tracking ref points to.
#
# Each ref is scanned and linted even when an earlier step failed, and the
# task fails at the end. So a failing lint never hides a leaked secret, on
# the same ref or on a later one. The script keeps to bash 3.2, the bash
# macOS ships.
set -euo pipefail

# Prints the remote sha when this repository has that commit.
known_tip() {
  if [[ $1 == *[!0]* ]] && git cat-file -e "$1^{commit}" 2>/dev/null; then
    echo "$1"
  fi
}

# Prints the commits a ref pushes, newest first.
pushed_commits() {
  git rev-list --topo-order "$1" --not --remotes ${2:+"$2"}
}

# committed takes one revision range, so this prints the fewest revisions that
# cover the pushed commits, one a line:
#   - one range, when the pushed commits are exactly those after the oldest
#     one's parent. That is a fast-forward, or a new branch off a pushed
#     commit;
#   - the root commit and the range after it, on a first push from the root;
#   - each commit otherwise.
revisions() {
  local tip=$1 commits=$2 oldest count base
  oldest=$(tail -n 1 <<<"$commits")
  count=$(grep -c . <<<"$commits")
  if base=$(git rev-parse -q --verify "$oldest^"); then
    if (($(git rev-list --count "$base..$tip") == count)); then
      echo "$base..$tip"
      return
    fi
  elif (($(git rev-list --count "$oldest..$tip") == count - 1)); then
    printf '%s\n' "$oldest" "$oldest..$tip"
    return
  fi
  echo "$commits"
}

# xargs runs committed on each revision and fails after the last one when any
# failed, so one push reports every bad commit.
lint_pushed() {
  revisions "$1" "$2" | xargs -n 1 committed ${COMMITTED_CONFIG:+--config "$COMMITTED_CONFIG"}
}

# gitleaks splits --log-opts on spaces.
# An empty remote sha must leave no trailing space, because git reads it as an
# empty revision.
scan_pushed() {
  gitleaks git --redact --no-banner --log-opts="$1 --not --remotes${2:+ $2}" </dev/null
}

status=0
while read -r _ tip _ remote; do
  # A delete pushes an all-zero sha, 40 zeros under SHA-1 and 64 under SHA-256.
  [[ $tip == *[!0]* ]] || continue
  known=$(known_tip "$remote")
  commits=$(pushed_commits "$tip" "$known")
  [[ -n $commits ]] || continue
  scan_pushed "$tip" "$known" || status=1
  lint_pushed "$tip" "$commits" || status=1
done
exit "$status"
