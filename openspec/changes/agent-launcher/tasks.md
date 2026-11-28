# Tasks

## 1. Check Rex on macOS

- [ ] 1.1 Start a second Rex server 0.1.0-preview.25 or later with its own `--state-dir` and socket, unset `REX_SERVER`, pass `-S <socket>` on every command, and confirm `rex -S <socket> whoami` names it before any stop or restart.
  Repeat probe points 1 to 4 of the design there.
  Done when the results are recorded under this task.
- [ ] 1.2 Run a real Claude Code session under `rawr claude` on that server and restart it.
  Done when the restored pane shows the stand-in and no `claude` process runs.

## 2. Launcher

- [ ] 2.1 `rawr <agent>`: launch record with the process ID, `exec`, and `rawr record` joining the launch only from that process's hook.
  Done when the launcher tests pass in CI.
- [ ] 2.2 The Claude Code adapter.
  Done when the adapter tests pass in CI.

## 3. Resume

- [ ] 3.1 `rawr resume`, with `--startup` as a hidden alias, and resumes through the launcher.
  Done when the scenarios of "rawr resume opens the stand-in" and "A resume brings back the launch flags" pass in CI.

## 4. Rex module

- [ ] 4.1 `rex/rawr.lua` with the guarded rules and the palette actions, and the daemon keeping it current.
  Done when the scenarios of "Rex resumes no supported agent itself" and "The daemon keeps rawr.lua current" pass.

## 5. Shell line

- [ ] 5.1 The agent functions, after ctl-core's `shell_init` renders functions.
  Done when the agent-function scenarios in install-and-shells pass in CI for bash, zsh and fish.
