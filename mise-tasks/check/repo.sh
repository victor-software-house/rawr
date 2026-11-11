#!/usr/bin/env bash
#MISE description="Shared repository checks and the repository's own check:local"
#MISE depends=[
#MISE   "format:toml",
#MISE   "lint:md",
#MISE   "lint:typos",
#MISE   "lint:workflows",
#MISE   "lint:workflow-policy",
#MISE   "specs",
#MISE   "specs:budget",
#MISE   "check:local",
#MISE ]
# Repository checks every project type runs. verify calls check:repo: the
# shared checks and check:local, in parallel. check/local.sh holds the
# repository's own checks. The tools come from mise.toml.
