#!/bin/sh
#MISE description="Fail when a mise lockfile no longer matches its config"
#MISE raw=true
# pre-push runs this with git's pre-push lines on stdin, one
# `<local ref> <local sha> <remote ref> <remote sha>` per ref; raw connects
# that stdin to the task. For each pushed commit that changes a lock input
# (mise.toml, mise.*.toml, a mise lock, .miserc.toml, or mise-tasks/, whose
# task tools the locks record), and each pushed merge commit, it runs
# `mise lock` for the lockfile_platforms entries in a detached worktree of
# that commit, and fails when a lockfile then differs from the commit, so the
# pusher reviews and commits the change. A commit that is not a merge and
# changes no lock input reuses its parent's result with no lock run of its
# own: it locks the same as that parent, which this push or an earlier one
# checked. The ref's tip is checked even then, since its parent may be on the
# remote already, pushed before this check or with --no-verify. A commit
# whose lock inputs match, file for file, those of a commit
# this push already checked reuses that result too, so a long first push
# locks once per distinct set of inputs. A delete (all-zero
# local sha, 40 zeros under SHA-1 and 64 under SHA-256) pushes nothing, and a ref whose commits a
# remote-tracking ref or the remote sha already holds, such as a tag on a
# pushed commit, was checked when those commits were pushed. The worktree
# is checked out with no hooks, so a checkout hook of the repository's own
# does not run in it. The pre-push wrapper passes the argument `pre-push`.
# git runs pre-push with no lines when a push sends nothing, as in
# "Everything up-to-date", and then there is nothing to check.
# Run by hand, with no argument and no pre-push lines,
# it locks the working tree in place and fails when a lockfile then differs
# from HEAD, which leaves the new locks to commit. `mise lock --dry-run`
# reports only version changes and misses a missing platform entry or
# download URL, which the real lock writes. The real lock also leaves out
# what no machine installs: a platform a tool's `os` option excludes, and one
# its release ships no build for. It
# runs against an empty global and system config directory, because a tool
# the pusher's own config pins has no entry in any project lock; the
# repository is trusted by path instead, since the pusher's
# trusted_config_paths is dropped with that config. A machine's own
# *.local.toml configs are ignored too, so the lock neither reads them nor
# writes a lock for them. mise matches the ignored paths against the
# canonical path, so the pattern starts from `pwd -P`.
#
# `mise lock` writes only the locks of the profiles MISE_ENV loads, so it
# runs once for each committed mise.<profile>.toml, which also locks
# mise.toml; a MISE_ENV the pusher's shell exports changes nothing. Under the
# hooks profile, mise would assign the pinned catalogs' task tools to that
# profile's lockfile, which nothing writes, so it is left out, and so is a
# machine's own mise.local.toml. Only tracked files count, so an untracked
# profile the push does not carry cannot fail it. hooks pieces always render
# mise.dev.toml.
caller=${1:-}
case $caller in
  "" | pre-push) ;;
  *)
    usage="the pre-push wrapper passes pre-push, and a run by hand passes none"
    echo "✘ unknown argument $caller; $usage"
    exit 2
    ;;
esac
scratch=$(mktemp -d) || { echo "✘ mktemp failed; see its error above"; exit 1; }
trap 'rm -rf "$scratch"; git worktree prune' EXIT
# sh runs the EXIT trap on a signal only when a trap for it exits.
trap 'exit 130' INT TERM HUP
mkdir "$scratch/config" "$scratch/seen" || { echo "✘ mkdir failed; see its error above"; exit 1; }

# inputs ARGS runs git ARGS on the files a lock depends on: the mise configs
# and locks, .miserc.toml, and mise-tasks/, whose task tools the locks record.
inputs() {
  git "$@" -- mise.toml 'mise.*.toml' mise.lock 'mise.*.lock' .miserc.toml mise-tasks
}

# check DIR locks each committed profile of the tree in DIR for the
# platforms lockfile_platforms lists in that tree under that profile, and
# prints the locks that then differ from that tree's commit. Errors go to
# stderr.
check() (
  cd "$1" || exit 1
  root=$(pwd -P) || { echo "✘ pwd -P failed; see its error above" >&2; exit 1; }
  # MISE_IGNORED_CONFIG_PATHS is a list split on ':', and each entry is a
  # glob that reads '\' as an escape, so a root holding either fails, and
  # the root's other glob characters are each put in a bracket class.
  case $root in
    *:*)
      echo "✘ $root holds ':', which MISE_IGNORED_CONFIG_PATHS splits on;" \
        "check out the repository at a path without it" >&2
      exit 1
      ;;
    *\\*)
      printf '✘ %s holds a backslash, which the ignore glob reads as an escape; ' "$root" >&2
      echo "check out the repository at a path without it" >&2
      exit 1
      ;;
  esac
  pattern="$(printf '%s' "$root" | sed 's/[[*?{}]/[&]/g')/**/*.local.toml"
  isolated() {
    env MISE_CONFIG_DIR="$scratch/config" MISE_SYSTEM_CONFIG_DIR="$scratch/config" \
      MISE_TRUSTED_CONFIG_PATHS="$root" MISE_IGNORED_CONFIG_PATHS="$pattern" "$@"
  }
  configs=$(git ls-files -- 'mise.*.toml') ||
    { echo "✘ git ls-files failed; see its error above" >&2; exit 1; }
  # The locks of the profiles locked below; no other lock file can fail the push.
  locked=mise.lock
  for config in $configs; do
    profile=${config#mise.}
    profile=${profile%.toml}
    case $profile in
      hooks | local | *.local) continue ;;
    esac
    if ! platforms=$(isolated MISE_ENV="$profile" mise settings get lockfile_platforms); then
      echo "✘ mise settings get lockfile_platforms failed in the $profile profile;" \
        "see its error above" >&2
      exit 1
    fi
    platforms=$(printf '%s' "$platforms" | tr -d '[]" ')
    if [ -z "$platforms" ]; then
      echo "✘ lockfile_platforms is empty in the $profile profile;" \
        "list the platforms the locks record in mise.toml" >&2
      exit 1
    fi
    # mise lock reports progress even with --quiet, so its output shows only
    # when it fails.
    if ! output=$(isolated MISE_ENV="$profile" mise lock --platform "$platforms" 2>&1); then
      printf '%s\n' "$output" >&2
      echo "✘ mise lock failed in the $profile profile; see its error above" >&2
      exit 1
    fi
    locked="$locked mise.$profile.lock"
  done
  # A profile's first lock shows as untracked.
  # shellcheck disable=SC2086 # one lock file name per word
  git status --porcelain --untracked-files=all -- $locked ||
    { echo "✘ git status failed; see its error above" >&2; exit 1; }
)

if [ -t 0 ]; then
  lines=""
else
  lines=$(cat) || { echo "✘ reading the pre-push lines failed; see its error above"; exit 1; }
fi
if [ -z "$lines" ]; then
  [ "$caller" = pre-push ] && exit 0
  changed=$(check .) || exit 1
  if [ -n "$changed" ]; then
    printf '%s\n' "$changed"
    echo "✘ these lockfiles differ from HEAD after mise lock: review the change and commit it"
    exit 1
  fi
  exit 0
fi
# The empty tree in this repository's object format, the base of each key.
empty=$(git hash-object -t tree /dev/null) ||
  { echo "✘ git hash-object failed; see its error above"; exit 1; }
status=0
printf '%s\n' "$lines" | {
  while read -r ref sha _ rsha; do
    case $sha in *[!0]*) ;; *) continue ;; esac
    known=""
    case $rsha in *[!0]*) git cat-file -e "$rsha^{commit}" 2>/dev/null && known=$rsha ;; esac
    # shellcheck disable=SC2086 # an empty $known adds no revision
    commits=$(git rev-list --reverse --parents "$sha" --not --remotes $known) ||
      { echo "✘ git rev-list failed for $ref; see its error above"; exit 1; }
    # An annotated tag's local sha names the tag object; rev-list lists its
    # commit, so the tip is compared as a commit.
    tip=$(git rev-parse --verify --quiet "$sha^{commit}") || continue
    while read -r commit parent second; do
      [ -n "$commit" ] || continue
      # The ref's tip is always checked: its parent may be a remote commit
      # this hook never saw, pushed before the check or past it.
      if [ -z "${second:-}" ] && [ -n "${parent:-}" ] && [ "$commit" != "$tip" ]; then
        touched=$(inputs diff-tree --no-commit-id --name-only -r "$parent" "$commit") ||
          { echo "✘ git diff-tree failed for $commit; see its error above"; exit 1; }
        [ -n "$touched" ] || continue
      fi
      # ls-tree reads a pathspec literally, so mise.*.toml would match nothing;
      # a diff against the empty tree expands it as diff-tree does above.
      listing=$(inputs diff-tree -r --no-commit-id "$empty" "$commit") ||
        { echo "✘ git diff-tree failed for $commit; see its error above"; exit 1; }
      key=$(printf '%s\n' "$listing" | git hash-object --stdin) ||
        { echo "✘ git hash-object failed for $commit; see its error above"; exit 1; }
      # Commits whose mise inputs match one this push checked lock the same.
      [ -e "$scratch/seen/$key" ] && continue
      : >"$scratch/seen/$key" || { echo "✘ marking $commit checked failed"; exit 1; }
      tree="$scratch/$commit"
      git -c core.hooksPath=/dev/null worktree add --quiet --detach "$tree" "$commit" </dev/null ||
        { echo "✘ git worktree add failed for $ref; see its error above"; exit 1; }
      # git runs pre-push with GIT_DIR set in a linked worktree, and a caller
      # can also export GIT_WORK_TREE or GIT_INDEX_FILE. Each names the
      # pusher's checkout, so git in the scratch worktree would compare it
      # with the pusher's index. The substitution's subshell unsets them, so
      # git finds that worktree's repository from its path. A run by hand
      # keeps the caller's own layout.
      changed=$(
        unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
        check "$tree" </dev/null
      ) || exit 1
      # One checkout at a time: a long push would otherwise keep one per
      # checked commit on disk until the EXIT trap.
      git worktree remove --force "$tree" </dev/null ||
        { echo "✘ git worktree remove failed for $ref; see its error above"; exit 1; }
      if [ -n "$changed" ]; then
        printf '%s\n' "$changed"
        short=$(git rev-parse --short "$commit")
        echo "✘ $ref: these lockfiles differ from commit $short after mise lock:" \
          "run mise run hooks:lockfile on it, review the change and commit it"
        status=1
      fi
    done <<EOF_COMMITS
$commits
EOF_COMMITS
  done
  exit "$status"
}
