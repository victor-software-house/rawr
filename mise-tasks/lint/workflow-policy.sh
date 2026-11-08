#!/bin/sh
#MISE description="The workflows follow the shared workflow policy: drafts, concurrency, labels, permissions, pins, mise_env"
#MISE tools = { conftest = "0.71.1" }
# conftest pulls the policy by commit into the git directory, so the
# repository carries no copy; --absolute-paths keeps a linked worktree's
# absolute git directory whole. Each pinned commit gets a folder of its own,
# filled once and never rewritten, so verify runs offline after the first run
# and no file of an older pin mixes in. Renders that share the git directory,
# such as cli/ and worker/, may run at once: a mkdir lock lets one of them
# pull, into a .part folder it renames when the pull is whole. An exit, or HUP,
# INT, QUIT or TERM, while it holds the lock removes both; a run killed outright
# leaves the lock, and the next run fails after 600 s with a message that
# names it. Each run writes the workflows' text to a file of its own.
# conftest parses each workflow as YAML, and the policy's texts.sh writes the
# workflows' text, which the pins rule reads for comments, as data beside it.
# It reads the policy's defaults.yaml, the repository's parameters and
# exceptions (.github/workflow-policy/config.yaml), and
# .github/actionlint.yaml's runner labels. Each --namespace is a rule the
# workflow_policy answer chose, or one of the repository's own packages in
# .github/workflow-policy/ that workflow_policy_namespaces names.
# GitHub runs only the repository root's workflows, so, as lint:workflows
# does, the task checks the root's workflows with the root's actionlint.yaml.
# A render below the root, such as cli/, reads its own .github/workflow-policy/
# when it keeps one, and the root's otherwise; each file is passed only where
# it exists, and a named package that no file defines fails the task. conftest
# fails on a missing folder, so it checks the tracked workflows only, and with
# none nothing runs and the check passes. -z hands xargs each path unquoted,
# whatever it holds.
render=$(pwd)
cd "$(git rev-parse --show-toplevel)" || exit 1
files=$(git ls-files ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml') || exit 1
[ -n "$files" ] || exit 0
policy=$(git rev-parse --git-path 'workflow-policy/9eb809757e973495c62ab316f5bbb858ae87747e') || exit 1
texts=$(git rev-parse --git-path "workflow-texts.$$.yaml") || exit 1
trap 'rm -f "$texts"' EXIT
if [ ! -d "$policy" ]; then
  mkdir -p "${policy%/*}" || exit 1
  waited=0
  until mkdir "$policy.lock" 2>/dev/null; do
    waited=$((waited + 1))
    [ "$waited" -le 600 ] || { echo "✘ $policy.lock has been held for 600 s; remove it if no run is pulling" >&2; exit 1; }
    sleep 1
  done
  trap 'rm -rf "${policy:?}.part"; rmdir "$policy.lock" 2>/dev/null; rm -f "$texts"' EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 131' QUIT
  trap 'exit 143' TERM
  if [ ! -d "$policy" ]; then
    rm -rf "${policy:?}.part"
    if conftest pull --absolute-paths -p "$policy.part" 'git::https://github.com/victor-software-house/actions.git//policy?ref=9eb809757e973495c62ab316f5bbb858ae87747e'; then mv "$policy.part" "$policy"; fi
  fi
  trap 'rm -f "$texts"' EXIT
  trap - HUP INT QUIT TERM
  rmdir "$policy.lock"
  [ -d "$policy" ] || exit 1
fi
sh "$policy/texts.sh" >"$texts" || exit 1
settings=.github/workflow-policy
if [ -d "$render/.github/workflow-policy" ]; then settings="$render/.github/workflow-policy"; fi
set -- -p "$policy" --data "$policy/defaults.yaml" --data "$texts"
if [ -d "$settings" ]; then set -- "$@" -p "$settings"; fi
if [ -f "$settings/config.yaml" ]; then set -- "$@" --data "$settings/config.yaml"; fi
if [ -f .github/actionlint.yaml ]; then set -- "$@" --data .github/actionlint.yaml; fi
git ls-files -z ':(glob).github/workflows/*.yml' ':(glob).github/workflows/*.yaml' | xargs -0 conftest test "$@" --namespace workflow_drafts --namespace workflow_concurrency --namespace workflow_labels --namespace workflow_permissions --namespace workflow_pins --namespace workflow_mise_env
