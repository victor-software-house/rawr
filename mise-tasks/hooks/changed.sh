#!/usr/bin/env bash
#MISE description="Run the fast file checks on the files the pushed commits change"
#MISE raw=true
# pre-push runs this with git's pre-push lines on stdin, one
# `<local ref> <local sha> <remote ref> <remote sha>` per ref; raw connects
# that stdin to the task. The wrapper passes the argument `pre-push`. git runs
# pre-push with no lines when a push sends nothing, as in "Everything
# up-to-date", and then there is nothing to check.
#
# For each pushed ref, the changed files are the union of the files the pushed
# commits touch: the commits that no remote-tracking ref and not the remote
# sha holds, as in hooks:pushed. Each file is checked as the pushed commit
# holds it, and only if the commit still has it as a regular file. A merge
# commit lists no files of its own, so the files of the commits it merges
# stand for it.
#
# The checks run in place when the working tree holds, for each file a check
# reads and for its config, what the pushed commit holds. Otherwise they run
# in a detached worktree of that commit, as hooks:lockfile does: no git hooks,
# no mise config of the pusher's, and the worktree's path trusted. The worktree
# loads the hooks profile alone. The dev profile's postinstall hooks run
# whenever mise installs a tool, which is not safe in a checkout that is not
# the pusher's: they install the git hooks and, in a Bun repository, the
# dependencies.
#
# Each check runs on the files it selects. rumdl lints any file it is given,
# so Markdown files are picked here by the extensions its own walk reads
# (rumdl 0.3.2: md, markdown, mkd, mkdn, mdown, mdwn, mdx, qmd, rmd, in any
# case); check them again when rumdl moves. taplo reads TOML. typos and Vale
# take every changed file and pick their own, from .typos.toml and the
# ProseStyle package's settings, so a change to a Rust or TypeScript file
# still reaches them. specs and the workflow checks read a whole tree, so they
# run when a file under openspec/ or .github/ changed, a deleted file included.
# A push that changes no file a check selects runs no check.
#
# Every selected check runs, even after one fails, and the task fails naming
# each failed check. A name starting with `-` gets `./` in front, so no tool
# reads it as an option.
caller=${1:-}
case $caller in
  "" | pre-push) ;;
  *)
    usage="the pre-push wrapper passes pre-push, and a run by hand passes none"
    echo "✘ unknown argument $caller; $usage"
    exit 2
    ;;
esac
if [ -t 0 ]; then
  lines=""
else
  lines=$(cat) || { echo "✘ reading the pre-push lines failed; see its error above"; exit 1; }
fi
[ -n "$lines" ] || exit 0
scratch=$(mktemp -d) || { echo "✘ mktemp failed; see its error above"; exit 1; }
trap 'rm -rf "$scratch"; git worktree prune' EXIT
# sh runs the EXIT trap on a signal only when a trap for it exits.
trap 'exit 130' INT TERM HUP
mkdir "$scratch/config" || { echo "✘ mkdir failed; see its error above"; exit 1; }

# has ARGS succeeds when there is at least one argument; an empty array has no
# portable expansion in the bash that macOS ships.
has() { [ "$#" -gt 0 ]; }

# same PATH... succeeds when the working tree holds, at each path, what $tip
# holds, and no untracked file stands at one.
same() {
  local magic=() path
  for path in "$@"; do magic+=(":(literal)$path"); done
  git diff --quiet "$tip" -- "${magic[@]}" || return 1
  [ -z "$(git ls-files --others --exclude-standard -- "${magic[@]}")" ]
}

# check NAME ARGS... runs the mise task where $where is, with the environment
# in $launch, and notes a failure.
check() {
  local name=$1
  shift
  (cd "$where" && "${launch[@]}" mise run "$name" "$@" </dev/null) && return 0
  echo "✘ $ref: $name failed on commit $short; its output is above"
  status=1
}

# want_files NAME CONFIG FILES... is a check on the files it is given, and
# want_tree NAME PATH a check on the tree under PATH. In the plan mode each
# notes the paths the check reads, and in the run mode it runs the check.
want_files() {
  local name=$1 config=$2
  shift 2
  has "$@" || return 0
  if [ "$mode" = plan ]; then
    planned=1
    reads+=("$config" "$@")
  else
    check "$name" "$@"
  fi
}
want_tree() {
  if [ "$mode" = plan ]; then
    planned=1
    reads+=("$2")
  else
    check "$1"
  fi
}
# The checks, as the pieces turn them on.
run_checks() {
  want_files format:toml .taplo.toml "${toml[@]}"
  want_files lint:md .rumdl.toml "${markdown[@]}"
  want_files lint:typos .typos.toml "${files[@]}"
  [ "$github" = 1 ] && want_tree lint:workflows .github
  [ "$github" = 1 ] && want_tree lint:workflow-policy .github
  [ "$openspec" = 1 ] && want_tree specs openspec
  [ "$openspec" = 1 ] && want_tree specs:budget openspec
  return 0
}

status=0
while read -r ref lsha _ rsha; do
  # A delete (all-zero local sha, 40 zeros under SHA-1 and 64 under SHA-256)
  # pushes nothing. An annotated tag's local sha names the tag object, so the
  # commit is the one checked.
  case $lsha in *[!0]*) ;; *) continue ;; esac
  tip=$(git rev-parse --verify --quiet "$lsha^{commit}") || continue
  known=""
  case $rsha in *[!0]*) git cat-file -e "$rsha^{commit}" 2>/dev/null && known=$rsha ;; esac
  short=$(git rev-parse --short "$tip")
  # shellcheck disable=SC2086 # an empty $known adds no revision
  git log -z --name-only --format= "$tip" --not --remotes $known >"$scratch/log" ||
    { echo "✘ git log failed for $ref; see its error above"; status=1; continue; }
  LC_ALL=C sort -zu "$scratch/log" >"$scratch/names" ||
    { echo "✘ sort failed for $ref; see its error above"; status=1; continue; }
  names=()
  while IFS= read -r -d '' name; do
    if [ -n "$name" ]; then names+=("$name"); fi
  done <"$scratch/names"
  has "${names[@]}" || continue
  # The files the commit still has, as regular files. ls-tree reads each path
  # literally.
  printf '%s\0' "${names[@]}" | xargs -0 git ls-tree -r -z "$tip" -- >"$scratch/entries" ||
    { echo "✘ git ls-tree failed for $ref; see its error above"; status=1; continue; }
  files=()
  markdown=()
  toml=()
  while IFS= read -r -d '' entry; do
    case ${entry%% *} in 100644 | 100755) ;; *) continue ;; esac
    path=${entry#*$'\t'}
    case $path in -*) path=./$path ;; esac
    files+=("$path")
    shopt -s nocasematch
    case $path in
      *.md | *.markdown | *.mkd | *.mkdn | *.mdown | *.mdwn | *.mdx | *.qmd | *.rmd)
        markdown+=("$path")
        ;;
      *.toml) toml+=("$path") ;;
    esac
    shopt -u nocasematch
  done <"$scratch/entries"
  github=0
  openspec=0
  for name in "${names[@]}"; do
    case $name in
      .github/*) github=1 ;;
      openspec/*) openspec=1 ;;
    esac
  done
  mode=plan
  planned=0
  reads=()
  run_checks
  [ "$planned" = 1 ] || continue
  tree=""
  # git runs pre-push with GIT_DIR set in a linked worktree, and a caller can
  # also export GIT_WORK_TREE or GIT_INDEX_FILE. A check that runs git, such
  # as the workflow policy's conftest pull, must find the repository from its
  # own directory, so neither launch passes them on.
  if same "${reads[@]}"; then
    where=.
    launch=(env -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE)
  else
    tree="$scratch/$tip"
    git -c core.hooksPath=/dev/null worktree add --quiet --detach "$tree" "$tip" </dev/null ||
      { echo "✘ git worktree add failed for $ref; see its error above"; status=1; continue; }
    where=$tree
    launch=(env -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE
      MISE_ENV=hooks MISE_CONFIG_DIR="$scratch/config" MISE_SYSTEM_CONFIG_DIR="$scratch/config"
      MISE_TRUSTED_CONFIG_PATHS="$tree")
  fi
  mode=run
  run_checks
  if [ -n "$tree" ]; then
    git worktree remove --force "$tree" </dev/null ||
      { echo "✘ git worktree remove failed for $ref; see its error above"; status=1; }
  fi
done <<EOF_LINES
$lines
EOF_LINES
exit "$status"
