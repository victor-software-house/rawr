#!/usr/bin/env bash
#MISE description="Spelling in code, docs, and commit-bound text"
#MISE tools = { typos = "1.51.1" }
# With files as arguments, it checks only those. typos skips an excluded file
# only when it is found by a walk, so --force-exclude makes .typos.toml's
# extend-exclude apply to the files named here too.
if [ "$#" -gt 0 ]; then
  typos --force-exclude "$@"
else
  typos
fi
