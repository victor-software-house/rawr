#!/bin/sh
#MISE description="Fail when a mise lockfile no longer matches its config"
#MISE raw=true
# pre-push runs this with git's pre-push lines on stdin, one
# `<local ref> <local sha> <remote ref> <remote sha>` per ref.
# raw connects that stdin to the task.
#
# The task checks each pushed commit that changes a lock input, and each
# pushed merge commit.
# A lock input is mise.toml, mise.*.toml, a mise lock, .miserc.toml, or
# mise-tasks/, whose task tools the locks record.
#
# For each such commit, the task runs `mise lock` for the lockfile_platforms
# entries in a detached worktree of that commit.
# It fails when a lockfile then differs from the commit.
# The pusher then reviews and commits the change.
#
# A commit that is not a merge and changes no lock input reuses the result of
# its parent, with no lock run of its own.
# It locks the same as that parent, which this push or an earlier one checked.
#
# The ref's tip is checked even then.
# Its parent may be on the remote already, pushed before this check or with
# --no-verify.
#
# A commit whose lock inputs match, file for file, those of a commit this push
# already checked reuses that result too.
# A long first push therefore locks once per distinct set of inputs.
#
# A delete pushes nothing. Its local sha is all zeros: 40 zeros under SHA-1
# and 64 under SHA-256.
# A ref needs no check when its commits are already in a remote-tracking ref
# or the remote sha. A tag on a pushed commit is an example.
# Those commits were checked when they were pushed.
#
# The worktree is checked out with no hooks, so a checkout hook of the
# repository's own does not run in it.
# The pre-push wrapper passes the argument `pre-push`.
# git runs pre-push with no lines when a push sends nothing, as in
# "Everything up-to-date". There is then nothing to check.
#
# Run by hand, with no argument and no pre-push lines, the task locks the
# working tree in place.
# It fails when a lockfile then differs from HEAD, which leaves the new locks
# to commit.
#
# `mise lock --dry-run` reports only version changes.
# It misses a missing platform entry or download URL, which the real lock
# writes.
# The real lock also leaves out what no machine installs.
# That is a platform that the `os` option of a tool excludes, and a platform
# that the release of the tool has no build for.
#
# The task runs against an empty global and system config directory.
# A tool that the pusher's own config pins has no entry in any project lock.
# The repository is trusted by path instead, since the pusher's
# trusted_config_paths is dropped with that config.
#
# The task ignores a machine's own *.local.toml configs too.
# The lock neither reads them nor writes a lock for them.
# mise matches the ignored paths against the canonical path, so the pattern
# starts from `pwd -P`.
#
# `mise lock` writes only the locks of the profiles MISE_ENV loads.
# The task therefore runs it once for each committed mise.<profile>.toml, which
# also locks mise.toml.
# A MISE_ENV that the pusher's shell exports does not change this.
#
# Under the hooks profile, mise would assign the task tools of the pinned
# catalogs to the lockfile of that profile, and nothing writes that lockfile.
# The task therefore leaves that profile out, and a machine's own
# mise.local.toml too.
#
# Only tracked files count, so an untracked profile the push does not carry
# cannot fail it. hooks pieces always render mise.dev.toml.
caller=${1:-}
case $caller in
  "" | pre-push) ;;
  *)
    usage="the pre-push wrapper passes pre-push, and a run by hand passes none"
    echo "✘ unknown argument $caller; $usage"
    exit 2
    ;;
esac
scratch=$(mktemp -d) || {
  echo "✘ mktemp failed; see its error above"
  exit 1
}
trap 'rm -rf "$scratch"; git worktree prune' EXIT
# sh runs the `EXIT` trap on a signal only when a trap for it exits.
trap 'exit 130' INT TERM HUP
mkdir "$scratch/config" "$scratch/seen" || {
  echo "✘ mkdir failed; see its error above"
  exit 1
}

# inputs runs git with the given arguments on the files a lock depends on.
# Those are the mise configs and locks, .miserc.toml, and mise-tasks/, whose
# task tools the locks record.
inputs() {
  git "$@" -- mise.toml 'mise.*.toml' mise.lock 'mise.*.lock' .miserc.toml mise-tasks
}

# check takes a directory and locks each committed profile of the tree there.
# For each profile, it uses the platforms that lockfile_platforms lists in
# that tree.
# It prints the locks that then differ from the commit of the tree.
# Errors go to stderr.
check() (
  cd "$1" || exit 1
  root=$(pwd -P) || {
    echo "✘ pwd -P failed; see its error above" >&2
    exit 1
  }
  # MISE_IGNORED_CONFIG_PATHS is a list split on ':'.
  # Each entry is a glob that reads '\' as an escape.
  # A root with either character therefore fails.
  # The other glob characters of the root are each put in a bracket class.
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
    {
      echo "✘ git ls-files failed; see its error above" >&2
      exit 1
    }
  # The locks of the profiles locked below; no other lockfile can fail the push.
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
  # shellcheck disable=SC2086 # one lockfile name per word
  git status --porcelain --untracked-files=all -- $locked ||
    {
      echo "✘ git status failed; see its error above" >&2
      exit 1
    }
)

if [ -t 0 ]; then
  lines=""
else
  lines=$(cat) || {
    echo "✘ reading the pre-push lines failed; see its error above"
    exit 1
  }
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
  {
    echo "✘ git hash-object failed; see its error above"
    exit 1
  }
status=0
printf '%s\n' "$lines" | {
  while read -r ref sha _ rsha; do
    case $sha in *[!0]*) ;; *) continue ;; esac
    known=""
    case $rsha in *[!0]*) git cat-file -e "$rsha^{commit}" 2>/dev/null && known=$rsha ;; esac
    # shellcheck disable=SC2086 # an empty $known adds no revision
    commits=$(git rev-list --reverse --parents "$sha" --not --remotes $known) ||
      {
        echo "✘ git rev-list failed for $ref; see its error above"
        exit 1
      }
    # An annotated tag's local sha points to the tag object.
    # rev-list lists its commit, so the tip is compared as a commit.
    tip=$(git rev-parse --verify --quiet "$sha^{commit}") || continue
    while read -r commit parent second; do
      [ -n "$commit" ] || continue
      # The ref's tip is always checked: its parent may be a remote commit
      # this hook never saw, pushed before the check or past it.
      if [ -z "${second:-}" ] && [ -n "${parent:-}" ] && [ "$commit" != "$tip" ]; then
        touched=$(inputs diff-tree --no-commit-id --name-only -r "$parent" "$commit") ||
          {
            echo "✘ git diff-tree failed for $commit; see its error above"
            exit 1
          }
        [ -n "$touched" ] || continue
      fi
      # ls-tree reads a pathspec literally, so mise.*.toml would match nothing;
      # a diff against the empty tree expands it as diff-tree does above.
      listing=$(inputs diff-tree -r --no-commit-id "$empty" "$commit") ||
        {
          echo "✘ git diff-tree failed for $commit; see its error above"
          exit 1
        }
      key=$(printf '%s\n' "$listing" | git hash-object --stdin) ||
        {
          echo "✘ git hash-object failed for $commit; see its error above"
          exit 1
        }
      # Commits whose mise inputs match one this push checked lock the same.
      [ -e "$scratch/seen/$key" ] && continue
      : >"$scratch/seen/$key" || {
        echo "✘ marking $commit checked failed"
        exit 1
      }
      tree="$scratch/$commit"
      git -c core.hooksPath=/dev/null worktree add --quiet --detach "$tree" "$commit" </dev/null ||
        {
          echo "✘ git worktree add failed for $ref; see its error above"
          exit 1
        }
      # git runs pre-push with GIT_DIR set in a linked worktree, and a caller
      # can also export GIT_WORK_TREE or GIT_INDEX_FILE.
      # Each points to the checkout of the pusher.
      # git in the scratch worktree would then compare it with the index of
      # the pusher.
      #
      # The subshell of the substitution unsets them.
      # git then finds the repository of that worktree from its path.
      # A run by hand keeps the layout of the caller.
      changed=$(
        unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
        check "$tree" </dev/null
      ) || exit 1
      # One checkout at a time: a long push would otherwise keep one per
      # checked commit on disk until the `EXIT` trap.
      git worktree remove --force "$tree" </dev/null ||
        {
          echo "✘ git worktree remove failed for $ref; see its error above"
          exit 1
        }
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
