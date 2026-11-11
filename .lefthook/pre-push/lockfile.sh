# lefthook runs this script with git's pre-push lines on stdin; exec hands
# that stdin to the task. The argument marks the hook call, so a push that
# sends nothing, and so has no lines, checks nothing.
exec mise run hooks:lockfile pre-push
