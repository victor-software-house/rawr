#!/usr/bin/env bash
#MISE description="Lint one commit or range with committed"
# This task is a check to run by hand.
# The pre-push hook runs committed on its own.
committed "${1:-HEAD}"
