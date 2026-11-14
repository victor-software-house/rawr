# lefthook runs this script with git's pre-push lines on stdin; exec hands
# that stdin to the task.
exec mise run hooks:head
