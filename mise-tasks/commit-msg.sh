#!/usr/bin/env bash
#MISE description="Lint a commit message file with committed"
# lefthook's commit-msg hook passes the message file to this task.
# committed lints that file against committed.toml.
committed --commit-file "$1"
