#!/usr/bin/env bash
#MISE description="Spelling in code, docs, and commit-bound text"
#MISE tools = { typos = "1.52.2" }
# With files as arguments, it checks only those.
# typos skips an excluded file only when a walk finds it.
# --force-exclude applies the extend-exclude of .typos.toml to these files too.
if [ "$#" -gt 0 ]; then
  typos --force-exclude "$@"
else
  typos
fi
