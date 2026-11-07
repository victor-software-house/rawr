#!/usr/bin/env bash
#MISE description="Shared repository checks and the repository's own check:local"
#MISE depends=["format:toml", "lint:md", "lint:typos", "lint:workflows", "lint:workflow-policy", "check:local"]
# Repository checks every project type runs. verify calls check:repo: the
# shared checks and check:local, in parallel. check/local.sh holds the
# repository's own checks. The tools come from mise.toml.
