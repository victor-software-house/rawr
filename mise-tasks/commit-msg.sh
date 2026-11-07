#!/usr/bin/env bash
#MISE description="Lint a commit message file with committed"
# lefthook's commit-msg hook passes the message file; committed lints it
# against committed.toml.
committed --commit-file "$1"
