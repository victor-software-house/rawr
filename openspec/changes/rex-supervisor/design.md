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
None drains agents before a restart.
The closest first-party models are Claude Code's own supervisor, which defers its restart while any session is mid-turn, and the self-hosted runner's drain ([agent view][cc-agent-view], [self-hosted deploy][cc-runner]).

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
   - While a drain is open, it carries the request into the session (decision 4) and shows the drain with `ui.status`.

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
   | `working`, or any child record | Working | spinner |
   | `blocked` | Needs input | `✻` in the warning colour |
   | `done` or `idle`, no child record | Idle | `✻` dimmed |
   | `error` | Failed | `∙` in the error colour |
   | no record, session in the index | Stopped | `∙` dimmed |

   rawr stops reading `~/.claude/sessions/<pid>.json`.
   Its format is not documented ([.claude directory][cc-dir]), and Rex already marks which blocks run a live Claude.
   Alternative: `claude agents --json`, the documented outside view.
   It lost: it needs Claude Code's agent view on, and adds nothing for sessions that run in Rex.
4. **A drain is a request in a file, and nothing types into a terminal.**
   - `rawr drain [--reason TEXT]` makes the daemon write one request to `$XDG_STATE_HOME/rawr/drain.yaml`: an ID, the reason, and the time it opened.
   - Each session's mod reads that file every 2 seconds with `$.fs.read`, and acts once per request ID.
     A session that starts while a drain is open reads it on its first read, like any other.
   - An idle session gets the hand-off request through `$.prompt.submit`, which Claude Code marks as sent by the rawr mod.
   - A busy session gets it through `$.session.append` before its next step.
   - A busy session that has not settled after `drain.interrupt` (default 2 minutes) gets its running turn cancelled with `$.turn.abort`, which stops the turn's running tools, and, once that turn's `turn.complete` arrives with `reason` `aborted` or `error`, the hand-off request through `$.prompt.submit`.
     That happens once per request ID, only for a request delivered through `$.session.append`; the submitted hand-off turn becomes the turn that carries the request, and it is not interrupted again.
     No mod call moves a running tool to the background, as `ctrl+b` does for the user, so cancelling the turn is the one way to cut a long tool call short.
   - The mod acknowledges a request with `rawr drain ack <id>` on `turn.complete` of the turn that carried it.
     After a `$.turn.abort` for `drain.interrupt`, the mod waits for the cancelled turn's `turn.complete`: with `reason` `aborted` or `error` it submits the hand-off and acknowledges only when the hand-off turn completes; with `reason` `answer` or `refusal` the turn ended before the abort took effect, so it submits no hand-off and acknowledges on that completion.
     That event fires however the turn ends: its `reason` is `answer`, `aborted`, `refusal` or `error`.
     A user who presses Esc during the hand-off turn also ends it; the pane then settles without a hand-off, since the user stepped in.
     The acknowledgement sits under the session's block, keyed by the drain's ID and the session's ID, and counts only for the block's current session, so neither an earlier drain's acknowledgement nor an earlier session's in the same pane counts.
   - The request text covers finishing the current step, committing or noting what is unfinished, writing a hand-off, starting no new work, and ending the turn.

   Delivery does not depend on `crossSessionInbound` or on the inbox socket.
   Alternative: Claude Code's per-session inbox socket.
   It lost: delivery depends on each session's inbound policy, the message arrives framed as another session's, and the message format is not documented ([cross-session messaging][cc-messaging]).
5. **A restart waits for idle, within a bound.**
   - `rawr restart` opens a drain.
   - The daemon then holds the restart until every live Claude block has settled, re-checking on each status event.
   - A block has settled when it has acknowledged the drain's ID and reads Idle or Failed with no child record.
     Since the acknowledgement comes at the end of the hand-off turn, an Idle reading from before that turn started cannot count.
   - The set of blocks is live: a block whose Claude exits or whose pane closes leaves it, and a Claude that starts during the drain joins it.
   - A block that reads Needs input is named at once, so the user can answer it, and the wait goes on.
   - A block whose session has no rawr mod, such as one started before `rawr setup`, is named at once as one that cannot acknowledge.
     It counts as settled once it has read Idle with no child record for `drain.grace` (default 1 minute).
   - Past `restart.wait` (default 5 minutes), the restart fails, closes the drain so no later session receives its request, and names each pane as busy, waiting on input, or not acknowledged.
   - Once every block has settled, the daemon checks every block once more right before the stop.
   - It then stops the Rex server, starts it again, and closes the drain.
   - On a timeout or any other failure, the daemon closes the drain, so no session goes on winding down for a restart that will not come.
   - The mod tells every session it delivered a cancelled drain to, acknowledged or not, that work can go on.
   - Interrupting `rawr restart` with `ctrl+c` cancels the restart and closes its drain.
   - At start, the daemon closes any drain it finds open, unless its restart record says a restart is pending, in which case it resumes the wait.
   - `drain.yaml` records the daemon's process ID and start time; a drain whose owner is gone is stale, delivers nothing, and goes at the next daemon start.
   - While it waits, `rawr restart` draws a live list of the panes and their state.
     `f` restarts at once, despite the panes still pending, and Esc cancels.
   - `--force` skips the wait, and so does a second `rawr restart` during the wait, after the self-hosted runner's second-signal rule.
6. **The daemon prunes pane files only against a confirmed listing.**
   - A block event never deletes a file by itself.
     It marks the pane, and the file goes only when a later full listing from a running Rex server confirms the block is gone.
     A Rex crash, upgrade or manual restart that emits removal events on shutdown therefore deletes nothing.
   - A listing counts only once the server reports its sessions restored, not merely when its socket answers.
     After a restart, pruning resumes with the first such listing; a daemon that died mid-restart starts on the same rule.
   - A failed or empty listing deletes nothing.
     When every pane really is closed, the old files stay until a later non-empty listing, which costs a few small files and never a pane's history.
   - Entries inside a pane file are cleaned on read, by [picker-and-index][picker-and-index].
7. **`AGENTS.md` rule 1 changes.**
   The daemon becomes a second writer of the index, for pruning only; `record` stays the only writer of new entries.
8. **rawr speaks Rex's documented control protocol, behind one client interface.**
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
9. **The mods API comes from Claude Code's own types.**
   Claude Code writes the mods API's TypeScript declarations into each mod's `.claude-plugin/types/claude-code/index.d.ts` when it loads the mod.
   The calls this design uses, `$.turn.abort`, `$.prompt.submit`, `$.session.append`, `$.fs.read`, `$.clock.every` and the `turn.complete` reasons, are checked there for Claude Code 2.1.322, and the mod's type check runs against that file.

## Not verified

1. Whether Rex's program status records come back empty after a server restart; a user on the Rex feedback forum reports that they do.
2. How a stopped Rex server starts again on macOS when the app started it, and whether `rex server install` is the right owner there.
3. Whether `$.prompt.submit` keeps a half-typed prompt in the session's prompt box.
4. Which Rex call or event marks a server whose sessions are restored, as decision 6 needs.

[picker-and-index]: ../picker-and-index/design.md
[pps]: https://www.superlogical.com/rex/docs/build/program-status
[rex-events]: https://www.superlogical.com/rex/docs/reference/events#terminal-events
[cc-terminal]: https://code.claude.com/docs/en/terminal-config
[cc-agent-view]: https://code.claude.com/docs/en/agent-view
[cc-runner]: https://code.claude.com/docs/en/self-hosted-environments-deploy
[cc-mods]: https://code.claude.com/docs/en/plugins/mods/api
[cc-dir]: https://code.claude.com/docs/en/claude-directory
[cc-messaging]: https://code.claude.com/docs/en/cross-session-messaging
[cc-settings]: https://code.claude.com/docs/en/settings
[herdr]: https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/agent_resume.rs
[cmux]: https://github.com/manaflow-ai/cmux/blob/d07cd34e57962278788305a8ce6b966bac40b47a/docs/agent-session-tracking-spec.md
[resurrect]: https://github.com/tmux-plugins/tmux-resurrect/blob/cff343cf9e81983d3da0c8562b01616f12e8d548/scripts/save.sh
[agent-deck]: https://github.com/asheshgoplani/agent-deck/blob/f7017710f879f2a232c1616ff43dded8247573e1/docs/session-id-lifecycle.md
