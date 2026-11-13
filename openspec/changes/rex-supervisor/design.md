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

1. **Homebrew is the install, with a service.**
   - `victor-software-house/homebrew-tap` holds a `rawr` formula.
   - The formula installs the binary and generates completions with `generate_completions_from_executable`.
   - Its `service do` block runs `rawr daemon`, so `brew services start rawr` runs the daemon under launchd on macOS and a systemd user unit on Linux ([Homebrew services][brew-services]).
   - The binary sits at `$(brew --prefix)/opt/rawr/bin/rawr`, a path that survives upgrades.
   - The caveats print `rawr setup`; the formula writes nothing under the user's home.

   Alternative: the install script or `mise use -g` as the main path.
   They lost: neither registers a service.
   They stay for Linux machines without Homebrew.
2. **One process outside every pane: `rawr daemon`.**
   It subscribes to Rex's events, keeps the index, prunes it, and runs drains and restarts.
   A restart ends every program in every pane, so the process that runs it cannot live in one.
   Alternative: `rawr restart` starts a detached helper on demand.
   It lost: a helper does nothing between restarts.
   The daemon also prunes the index as panes close and answers `rawr status` at once.
3. **The Claude Code side is one mod, in a plugin this repository serves.**
   - `.claude-plugin/marketplace.json` lists one plugin, `rawr`, whose `hooks/hooks.json` holds only `{ "modules": ["./register.ts"] }`.
   - On `session.start`, the mod records the session under `REX_BLOCK` through the rawr binary.
     It runs the binary at the path `rawr setup` stored in the plugin's data folder (`${CLAUDE_PLUGIN_DATA}`).
   - While a drain is open, it carries the request into the session (decision 7) and shows the drain with `ui.status`.

   The mods API is documented ([mods API][cc-mods]): `$.fs.read`, `$.process.run` with an argument list and no shell, `$.clock.every`, `$.prompt.submit` and `$.session.append`.
   Alternative: plain command hooks in `hooks.json` calling `rawr record`.
   It lost: a command hook finds `rawr` through the `PATH` of whatever started Claude Code.
   The drain also needs logic a command line cannot hold, so the Claude side would be split across two mechanisms.
4. **Status comes from Rex only.**
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
5. **The picker follows Claude Code's screen.**
   - The list is on top.
     The search box sits at the bottom, drawn like Claude Code's prompt box, with the pane's name in its top rule and the key line under it.
   - Each row holds the glyph and status word, the title, the folder, and a right-aligned age, after the agents view's columns.
   - The newest rows show first, up to `picker.rows` (default 3).
     When more sessions exist, a dim `+N earlier · ctrl+o` row stands for the rest, and `ctrl+o` expands and collapses them, as it does for detail in Claude Code.
   - Esc, `q` and `ctrl+c` leave one dim line in the shell: `rawr pick reopens this pane's sessions · rawr find searches all`.
6. **The history per pane is a setting.**
   `history.per_pane` (default 10) bounds each pane's index.
   `picker.rows` bounds what shows before `ctrl+o`.
   Both live in `$XDG_CONFIG_HOME/rawr/config.yaml`, read through ctl-core's `input` loader, so a bad value names its file and line.
7. **A drain is a request in a file, and nothing types into a terminal.**
   - `rawr drain [--reason TEXT]` makes the daemon write one request to `$XDG_STATE_HOME/rawr/drain.yaml`: an ID, the reason, and the time it opened.
   - Each session's mod reads that file every 2 seconds with `$.fs.read`, and acts once per request ID.
     A session that starts while a drain is open reads it on its first read, like any other.
   - An idle session gets the hand-off request through `$.prompt.submit`, which Claude Code marks as sent by the rawr mod.
   - A busy session gets it through `$.session.append` before its next step.
   - The mod acknowledges a request with `rawr drain ack <id>` when the turn that carried it ends, whether it completes (`turn.complete`) or ends in error (`turn.abort`).
     The acknowledgement sits under the session's block, keyed by the drain's ID and the session's ID, and counts only for the block's current session, so neither an earlier drain's acknowledgement nor an earlier session's in the same pane counts.
   - The request text covers finishing the current step, committing or noting what is unfinished, writing a hand-off, starting no new work, and ending the turn.

   Delivery does not depend on `crossSessionInbound` or on the inbox socket.
   Alternative: Claude Code's per-session inbox socket.
   It lost: delivery depends on each session's inbound policy, the message arrives framed as another session's, and the message format is not documented ([cross-session messaging][cc-messaging]).
8. **A restart waits for idle, within a bound.**
   - `rawr restart` opens a drain.
   - The daemon then holds the restart until every live Claude block has settled, re-checking on each status event.
   - A block has settled when it has acknowledged the drain's ID and reads Idle or Failed with no child record.
     Since the acknowledgement comes at the end of the hand-off turn, an Idle reading from before that turn started cannot count.
   - The set of blocks is live: a block whose Claude exits or whose pane closes leaves it, and a Claude that starts during the drain joins it.
   - A block that reads Needs input is named at once, so the user can answer it, and the wait goes on.
   - A block whose session has no rawr mod, such as one started before `rawr setup`, is named at once as one that cannot acknowledge.
     It counts as settled once it has read Idle with no child record for `drain.grace` (default 2 minutes).
   - Past `restart.wait` (default 30 minutes, Claude Code's own cap), the restart fails, closes the drain so no later session receives its request, and names each pane as busy, waiting on input, or not acknowledged.
   - Once every block has settled, the daemon checks every block once more right before the stop.
   - It then stops the Rex server, starts it again, and closes the drain.
   - On a timeout or any other failure, the daemon closes the drain, so no session goes on winding down for a restart that will not come.
   - The mod tells a session that acknowledged a cancelled drain that work can go on.
   - `drain.yaml` records the daemon's process ID and start time; a drain whose owner is gone is stale, delivers nothing, and goes at the next daemon start.
   - `--force` skips the wait, and so does a second `rawr restart` during the wait, after the self-hosted runner's second-signal rule.
9. **The daemon offers restored panes their sessions.**
   After a restart, each block comes back with no program status.
   For each block with sessions in the index whose foreground process is a shell, the daemon runs the picker there, as the "Resume Claude here" Rex action does today.
   The shell snippet becomes optional, for those who want the picker without the daemon.
10. **The index cleans itself.**
    - Each entry carries `transcript_seen`, set the first time rawr finds its transcript.
    - An entry goes when `transcript_seen` is set and the transcript is gone; Claude Code deletes transcripts with no message after `cleanupPeriodDays`, 30 days by default ([settings][cc-settings]).
    - An entry with no transcript goes 24 hours after it was recorded, so a session that has not written its first message yet stays.
    - A block event never deletes a file by itself.
      It marks the pane, and the file goes only when a later full listing from a running Rex server confirms the block is gone.
      A Rex crash, upgrade or manual restart that emits removal events on shutdown therefore deletes nothing.
    - A listing counts only once the server reports its sessions restored, not merely when its socket answers.
      After a restart, pruning resumes with the first such listing; a daemon that died mid-restart starts on the same rule.
    - A failed or empty listing deletes nothing.
      When every pane really is closed, the old files stay until a later non-empty listing, which costs a few small files and never a pane's history.
    - The per-pane bound is `history.per_pane`.
11. **`rawr setup` installs everything after the binary, and only what it names.**
    - `rawr setup` adds this repository's marketplace and installs the plugin through Claude Code's plugin commands.
    - It stores the binary's stable path in the plugin's data folder and starts the service when Homebrew installed rawr.
    - It removes an old `SessionStart` hook that runs `rawr record`, when it finds one.
    - `rawr setup status` reports each piece as current, outdated, missing or foreign, as `rex skill status` does.
    - `rawr setup remove` removes only what `rawr setup` installed.
12. **Rules in `AGENTS.md` that change.**
    - Rule 1: the daemon becomes a second writer of the index, for pruning only; `record` stays the only writer of new entries.
    - Rule 3: after a dismissal, `pick` prints the one hint line on stderr.
    - Rule 4: the binary also talks to Claude Code's plugin commands during `rawr setup`.
      Nothing resumes without a choice, as before.

## Open points for the maintainer

1. **How rawr talks to Rex.**
   Rex documents its methods (`rex api list`, `rex api describe`) and its Lua API, but not the protocol a program speaks on the server socket.
   - Speaking the socket directly gives one connection and streamed events with no process per call, but the protocol can change without notice.
   - A Lua module in Rex's configuration uses only documented calls (`rex.call`, `terminal.program_status_changed`) and runs inside the server, but it stops with the server and must hand rawr what it needs.
2. **Who offers restored panes their sessions:** the daemon (decision 9), or the shell snippet as today.
3. **The defaults:** 10 sessions kept per pane, 3 rows shown before `ctrl+o`, 30 minutes of drain wait, and 2 minutes of grace for a session with no rawr mod.

## Not verified

1. Whether Rex's program status records come back empty after a server restart; a user on the Rex feedback forum reports that they do.
2. How a stopped Rex server starts again on macOS when the app started it, and whether `rex server install` is the right owner there.
3. Whether `$.prompt.submit` keeps a half-typed prompt in the session's prompt box.
4. Which Rex call or event marks a server whose sessions are restored, as decision 10 needs.

[pps]: https://www.superlogical.com/rex/docs/build/program-status
[rex-events]: https://www.superlogical.com/rex/docs/reference/events#terminal-events
[cc-terminal]: https://code.claude.com/docs/en/terminal-config
[cc-agent-view]: https://code.claude.com/docs/en/agent-view
[cc-runner]: https://code.claude.com/docs/en/self-hosted-environments-deploy
[cc-mods]: https://code.claude.com/docs/en/plugins/mods/api
[cc-dir]: https://code.claude.com/docs/en/claude-directory
[cc-messaging]: https://code.claude.com/docs/en/cross-session-messaging
[cc-settings]: https://code.claude.com/docs/en/settings
[brew-services]: https://docs.brew.sh/Formula-Cookbook#service-files
[herdr]: https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/agent_resume.rs
[cmux]: https://github.com/manaflow-ai/cmux/blob/d07cd34e57962278788305a8ce6b966bac40b47a/docs/agent-session-tracking-spec.md
[resurrect]: https://github.com/tmux-plugins/tmux-resurrect/blob/cff343cf9e81983d3da0c8562b01616f12e8d548/scripts/save.sh
[agent-deck]: https://github.com/asheshgoplani/agent-deck/blob/f7017710f879f2a232c1616ff43dded8247573e1/docs/session-id-lifecycle.md
