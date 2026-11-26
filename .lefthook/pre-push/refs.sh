#!/bin/sh
# lefthook runs this script with git's pre-push lines on stdin.
# lefthook 2.2.2 gives every use_stdin job of a parallel hook one shared
# reader. Two such jobs then race, and one of them can read no lines and pass.
#
# This one job reads the lines once and passes them to each check in turn:
# the lock check, the commit check, and the checkout check.
# Each check runs even when one before it fails.
# A push therefore reports every failure at once.
lines=$(cat)
status=0
feed() { [ -z "$lines" ] || printf '%s\n' "$lines"; }
feed | MISE_ENV=$LOCK_MISE_ENV sh .lefthook/pre-push/lockfile.sh || status=1
feed | sh .lefthook/pre-push/commits.sh || status=1
feed | sh .lefthook/pre-push/head.sh || status=1
exit $status
