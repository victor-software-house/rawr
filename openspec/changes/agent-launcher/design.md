# Design

## Context

Rex 0.1.0-preview.25 resumes a program after a server restart when a [resume rule][rules] trusts the command the program reported ([protocol][protocol]).
Its built-in `claude` rule and hook resume every Claude Code session that was running, so a restart starts all of them at once.

rawr already restores without that: the shell line runs the stand-in once before the first prompt, and the stand-in, a client of the rawr daemon ([app-daemon][app-daemon]), offers the pane's sessions and starts nothing until the user picks.

## Rex behaviour this design relies on

Two probes against a Rex 0.1.0-preview.25 server on Linux (2026-11-27) checked each point with real executables and `program_resume_records`.

1. With `resume.trust.claude` set to `false`, Rex keeps Claude Code's record untrusted (`no_rule`), and after a restart the pane is a plain shell.
2. Rex trusts a record only when the owner process's executable is the program the command names, and no program other than a shell sits between the pane's first process and the owner.
3. A process the shell started can own a trusted record; one under `claude`, or detached from the pane's terminal, cannot.
4. Rex dropped a dead owner's record about 2.6 seconds after it noticed the exit, and resumed one when nothing touched the terminal before the restart.
5. Rex has no per-terminal `env` option and no `init.lua` terminal setting; only `resume.*` settings exist.
   `rex server start --env PATH=…` and the terminal option `path_prepend` put a folder behind what the shell's startup files prepend.
6. `require("rawr")` in `init.lua` loads `rawr.lua` from the same folder (checked with `rex config check` on macOS).

Not yet checked on macOS: points 1 to 4, and a real Claude Code session.

## Decisions

### 1. The stand-in restores, and Rex resumes no Claude Code

`rawr.lua` sets `resume.trust.claude` to `false`.
The shell line's first-prompt hook runs `rawr resume` in each restored pane, as `rawr --startup` does today.

Alternative: rawr owns the pane's record, so Rex runs `rawr resume` at a restart.
It lost: the launcher would have to be every agent's parent and report for itself, while the shell line it needs already restores the pane.
It would add only panes whose shell line is missing, and only for agents started through the launcher.

Alternative: the agent's hook reports a record for a rawr process.
It lost on probe point 3.

### 2. `rawr resume` replaces `rawr --startup`

ctl-core's `shell_init` runs `<bin> <subcommand>`, and `Hook::new` takes a subcommand that starts with a letter or digit, so a flag cannot be the hook.
`--startup` stays as a hidden alias for one release, for shell lines saved before this change.

### 3. The launcher records, then becomes the agent

`rawr <agent> [args]` writes the pane's launch record with its own process ID, and replaces itself with the agent through `exec`, so the agent keeps that ID.
The launch record holds the agent, its process ID, its folder and its arguments.
`rawr record`, run by the agent's session hook, reads the pane's index file from `REX_BLOCK` and walks up its own ancestors, through any shell that runs the hook, to the nearest agent process.
It adds the session to the launch record whose process ID and start time match that process, so a reused process ID never matches.
A nested agent started from inside it is another process: its session is recorded as nested, and the stand-in does not offer it.
No rawr process stays: signals, the exit status and Rex's process name are the agent's own.

Alternative: the launcher stays the agent's parent until it exits.
It lost: it costs one process per agent and shows `rawr` as the pane's program.
Its one gain is that Rex marks the agent's own record `wrapped`, which the `rawr.lua` rule already makes untrusted.

### 4. A resume runs through the launcher

The stand-in and the app become `rawr <agent>` with the adapter's resume arguments and the recorded flags, by `exec` in their own pane or through the daemon in a new tab.
The resumed agent then writes its launch record again, so a second restart offers it too.
Only the flags come back: a prompt given as an argument, and the flags that pick a session such as `--resume` and `--continue`, do not, as Rex's own `claude` rule drops them.

### 5. Adapters per agent

| Field | Claude Code |
|:--|:--|
| Session source | `rawr record`, from its `SessionStart` hook |
| Resume arguments | `--resume <id>` |
| Flags kept | the flags Rex's built-in `claude` rule keeps |

An agent without a session hook resumes with its own "continue the last session in this folder" argument, from the launch record's folder.

### 6. The agent's name reaches rawr through the shell line

Inside a Rex pane, the shell line defines one function per supported agent, `function claude { command rawr claude "$@"; }` in zsh, and the bash and fish forms.
The shell line does all of this only when `rawr` is on `PATH`, so a block left after rawr is removed does nothing.
Outside Rex it defines nothing, and it defines nothing for a name that already names a function, so a user's own wrapper stays in charge.
An alias of the same name stays: bash and zsh expand it once and do not expand its own name again, so `alias claude='claude --dangerously-skip-permissions'` reaches the function and runs `rawr claude --dangerously-skip-permissions`, keeping the alias's flags and the launcher (checked in zsh and bash).
An alias whose body names a path, such as `~/.claude/local/claude`, runs that path and bypasses the launcher; `rawr setup status` reports it.
Alternative: remove the alias.
It lost: it drops the flags the alias adds.
Alternative: define nothing when an alias exists.
It lost: the common alias that adds flags would then bypass the launcher.
This needs ctl-core's `shell_init` to render functions, a ctl-core change with its own ledger row; until it ships, `rawr init` prints only the hook.

Alternative: a folder of agent links put first on `PATH` by a Rex mechanism.
It lost on probe point 5.

### 7. `rawr.lua` sits beside `init.lua`

`rawr setup` writes `rawr.lua` into each user's Rex configuration folder; no package installs it system-wide and adds `require("rawr")` to `init.lua`; `require` finds a module beside `init.lua` (probe point 6).
When the daemon starts, the setup state records the Rex module as installed and the file exists, it compares the file with the copy its binary carries, rewrites it when they differ, and calls `server.config_reload`, so an upgrade needs no step and a removed module stays removed.
`rawr.lua` names the binary's path, which the daemon writes into it; when that file is gone, the module turns no Rex rule off and adds no palette action, so a module left after rawr is removed leaves Rex's own resume in place.
While the rawr binary exists, `rawr.lua` turns off Rex's rule for every agent that has an adapter, so Rex starts none of them at a restart.
It calls `rex.set` only when `rex.set` exists: Rex 0.1.0-preview.21 and older have no `rex.set`, and an unguarded call makes `rex config check` fail with "attempt to call a non-function object" (checked on preview.20 and preview.21).
Without the Rex module, Rex 0.1.0-preview.25 or later resumes every Claude Code session at a restart again.

## Repository rules that change

1. Rule 1: the launcher writes the launch record, so `record` is no longer the only writer.
2. Rule 3: a resume replaces the window's process with `rawr <agent>`, not `claude --resume`.
3. Rule 4: rawr starts the supported agents, through the launcher.
4. Rule 1 again: the daemon writes `rawr.lua` and the shell file under `~/.local/state/rawr/`.

## Risks

1. A Rex server older than 0.1.0-preview.25 has no `claude` rule to turn off, and resumes nothing itself.
2. A shell without the line restores nothing, and its agents run without the launcher.

[protocol]: https://www.superlogical.com/rex/docs/build/program-resume
[rules]: https://www.superlogical.com/rex/docs/customize/resume-rules
[app-daemon]: ../app-daemon/proposal.md
