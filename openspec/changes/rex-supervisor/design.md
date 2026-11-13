# Design

## Context

rawr serves one case: Claude Code sessions running in Rex panes.
Sessions in other terminals, background sessions and Claude Code's daemon workers are out of scope.

Two facts carry the design.

1. **The pane-to-session mapping already exists.**
   - Each Claude Code session started in a Rex pane runs with Rex's `REX_BLOCK` in its environment.
   - rawr records the session under that block ID.
   - Rex keeps a block's ID across a server restart, so the ID is the pane's identity.
2. **Rex already holds each pane's agent status.**
   - From version 2.1.295, Claude Code reports its state through Rex's [Program Status Protocol][pps], an operating system command (OSC) sequence numbered 7501, in foreground sessions ([terminal configuration][cc-terminal]).
   - The root record is `working`, `blocked`, `done`, `idle` or `error`; a `blocked` record also names what it waits on, such as `permission` or `question`.
   - Each running background task adds a child record.
   - Rex stores the records per block and returns them from the terminal block's `program_status` method.
   - Rex emits `terminal.program_status_changed` and `terminal.program_status_removed` on each change ([events][rex-events]).

So rawr needs no status of its own.
It needs the mapping it already writes, the status Rex holds, and a way to act on both from outside every pane.

The other tools, in short:

| Tool | Keeps per pane | Status from | Restore | Survives its own update |
|:--|:--|:--|:--|:--|
| [herdr][herdr] | one session reference | screen-matching rules | types `claude --resume <id>` into the restored shell | `herdr update` keeps compatible servers |
| [cmux][cmux] | one record per surface, keyed by an env variable | Claude Code hooks, OSC 7501 for missed stops | resumes when a hook saved an ID | opt-in tmux owner |
| [tmux-resurrect][resurrect] tools | one ID per pane | none | replays a saved command | n/a |
| [agent-deck][agent-deck] | one ID per instance | hooks plus polling | `--resume` only when the transcript exists | systemd scope (Linux) |

No tool keeps more than one session per pane.
rawr keeps a short history per pane, and that history is the reason it exists.

## Decisions

1. **One process outside every pane: `rawr daemon`.**
   It subscribes to Rex's events, keeps the index, prunes it, and runs drains and restarts.
   A restart ends every program in every pane, so the process that runs it cannot live in one.
   Alternative: `rawr restart` starts a detached helper on demand.
   It lost: a helper does nothing between restarts.
   The daemon also prunes the index as panes close and answers `rawr status` at once.
2. **The Claude Code side is one mod, in a plugin this repository serves.**
   - `.claude-plugin/marketplace.json` lists one plugin, `rawr`, whose `hooks/hooks.json` holds only `{ "modules": ["./register.ts"] }`.
   - On `session.start`, the mod records the session under `REX_BLOCK` through the rawr binary.
     It runs the binary at the path `rawr setup` stored in the plugin's data folder (`${CLAUDE_PLUGIN_DATA}`).
   - While a drain is open, it carries the request into the session ([drain-and-restart][drain-and-restart]) and shows the drain with `ui.status`.

   The mods API is documented ([mods API][cc-mods]): `$.fs.read`, `$.process.run` with an argument list and no shell, `$.clock.every`, `$.prompt.submit` and `$.session.append`.
   Alternative: plain command hooks in `hooks.json` calling `rawr record`.
   It lost: a command hook finds `rawr` through the `PATH` of whatever started Claude Code.
   The drain also needs logic a command line cannot hold, so the Claude side would be split across two mechanisms.
3. **Status comes from Rex only.**
   `rawr status` joins Rex's per-block program status with the index's newest session for each block.
   A block whose root record has `app=claude-code` runs a live Claude.
   The words follow Claude Code's agents view:

   | Rex record | Word | Glyph |
   |:--|:--|:--|
   | `blocked` | Needs input | `✻` in the warning colour |
   | `working`, or any child record in `working` | Working | spinner |
   | `done` or `idle`, no child record in `working` | Idle | `✻` dimmed |
   | `error` | Failed | `∙` in the error colour |
   | no record, session in the index | Stopped | `∙` dimmed |

   The first row that matches wins, so Needs input shows even while a background task runs, since the user has to act.
   A child record that reports `done`, `idle` or `error` is a finished task and makes no block Working.

   rawr stops reading `~/.claude/sessions/<pid>.json`.
   Its format is not documented ([.claude directory][cc-dir]), and Rex already marks which blocks run a live Claude.
   Alternative: `claude agents --json`, the documented outside view.
   It lost: it needs Claude Code's agent view on, and adds nothing for sessions that run in Rex.
4. **The daemon prunes pane files only against a confirmed listing.**
   - A block event never deletes a file by itself.
     It marks the pane, and the file goes only when a later full listing from a running Rex server confirms the block is gone.
     A Rex crash, upgrade or manual restart that emits removal events on shutdown therefore deletes nothing.
   - A listing counts only once the server reports its sessions restored, not merely when its socket answers.
     After a restart, pruning resumes with the first such listing; a daemon that died mid-restart starts on the same rule.
   - A failed or empty listing deletes nothing.
     When every pane really is closed, the old files stay until a later non-empty listing, which costs a few small files and never a pane's history.
   - Entries inside a pane file are cleaned on read, by [picker-and-index][picker-and-index].
5. **`AGENTS.md` rule 1 changes.**
   The daemon becomes a second writer of the index, for pruning only; `record` stays the only writer of new entries.
6. **rawr speaks Rex's documented control protocol, behind one client interface.**
   - The Rex server documents its own API and serves the documentation from the same socket: `GET /llms.txt` describes the protocol, and `GET /api/help` lists every method with the JSON Schema of its arguments and result.
   - The control connection is a WebSocket at `GET /api/control` with the subprotocol `rex.control.v1`.
     Every message is one JSON object: the client sends one `hello`, then `request` messages; the server sends a `welcome`, a `response` per request, and `event` messages.
   - The hello names the newest and oldest API revisions the client speaks, and the welcome names the revision the connection uses, so rawr pins the revision it was written for (13 today) and refuses a server outside its range.
   - A `rex` module owns the connection and exposes typed calls and an event stream: list sessions and blocks, read a block's program status, watch status and block events, stop and start the server.
   - Its request and response types come from `/api/help`'s schemas, and a test against a recorded exchange shows when a Rex change touches them.
   - Nothing outside that module sees the protocol.
   - Alternative: a Lua module in Rex's configuration, which uses only documented calls.
     It lost: it runs inside the server, stops with it, and can hand rawr its findings only through a file or a process.
   - Alternative: running `rex api call` per request.
     It lost: one process per call, and no event stream.
7. **The mods API comes from Claude Code's own types.**
   Claude Code writes the mods API's TypeScript declarations into each mod's `.claude-plugin/types/claude-code/index.d.ts` when it loads the mod.
   The calls this design uses, `$.turn.abort`, `$.prompt.submit`, `$.session.append`, `$.fs.read`, `$.clock.every` and the `turn.complete` reasons, are checked there for Claude Code 2.1.322, and the mod's type check runs against that file.

## Not verified

1. Whether Rex's program status records come back empty after a server restart; a user on the Rex feedback forum reports that they do.
2. Which Rex call or event marks a server whose sessions are restored, as decision 4 needs.

[picker-and-index]: ../picker-and-index/design.md
[pps]: https://www.superlogical.com/rex/docs/build/program-status
[rex-events]: https://www.superlogical.com/rex/docs/reference/events#terminal-events
[cc-terminal]: https://code.claude.com/docs/en/terminal-config
[cc-mods]: https://code.claude.com/docs/en/plugins/mods/api
[cc-dir]: https://code.claude.com/docs/en/claude-directory
[herdr]: https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/agent_resume.rs
[cmux]: https://github.com/manaflow-ai/cmux/blob/d07cd34e57962278788305a8ce6b966bac40b47a/docs/agent-session-tracking-spec.md
[resurrect]: https://github.com/tmux-plugins/tmux-resurrect/blob/cff343cf9e81983d3da0c8562b01616f12e8d548/scripts/save.sh
[agent-deck]: https://github.com/asheshgoplani/agent-deck/blob/f7017710f879f2a232c1616ff43dded8247573e1/docs/session-id-lifecycle.md
[drain-and-restart]: ../drain-and-restart/design.md
