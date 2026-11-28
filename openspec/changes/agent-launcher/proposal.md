# Proposal

## Why

Rex 0.1.0-preview.25 restores programs after a server restart through the [Program Resume Protocol][protocol], and ships a rule that resumes Claude Code.
After a restart it starts a full Claude Code in every pane that had one, all at once.
rawr's stand-in already offers each restored pane its sessions and starts nothing until the user picks one.

rawr also knows only what Claude Code's session hook tells it: the session, not the flags it was started with, and nothing for an agent without such a hook.

The user gains:

1. A Rex restart starts no agent; each restored pane offers its sessions, as today.
2. A resume brings the agent back with the flags it was started with.
3. Other agents than Claude Code, one adapter each.
4. The user still types `claude`.

## What Changes

1. rawr ships its Rex module, `rawr.lua`, which turns Rex's built-in `claude` resume rule off and holds the palette actions.
2. `rawr resume` replaces `rawr --startup` as the stand-in's command.
3. `rawr <agent> [args]` records the pane's launch with the exact arguments, then becomes the agent.
4. A resume, from the stand-in or the app, becomes `rawr <agent>` with the agent's resume arguments and the recorded flags, so the resumed agent records again.
5. The shell line from [install-and-shells][install-and-shells] also defines, inside a Rex pane, one function per supported agent that runs `rawr <agent>`.

## Capabilities

### New Capabilities

- `agent-launcher`: `rawr <agent>`, the launch record, the per-agent adapters, and `rawr.lua`.

### Modified Capabilities

None.
This change edits the open changes that define the stand-in and the shell line in place: [stand-in][stand-in] (`rawr resume`) and [install-and-shells][install-and-shells] (the agent functions).

## Impact

1. Modules: `src/main.rs` (the launcher and `resume`), new `src/launcher.rs`, new `src/agents/` (one adapter per agent), `src/index.rs` (the launch record), `src/daemon/` (keeps `rawr.lua` current), a new `rex/rawr.lua`.
2. `AGENTS.md` rules 1, 3 and 4.
3. ctl-core: `shell_init` gains agent functions, with its own ledger row and release.
4. Requires Rex server 0.1.0-preview.25 or later for the rule; an older server has no Claude rule to turn off.

[protocol]: https://www.superlogical.com/rex/docs/build/program-resume
[stand-in]: ../stand-in/proposal.md
[install-and-shells]: ../install-and-shells/proposal.md
