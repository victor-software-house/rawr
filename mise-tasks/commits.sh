#!/usr/bin/env bash
#MISE description="Lint one commit or range with committed"
# pre-push runs this once for each pushed commit.
committed "${1:-HEAD}"
