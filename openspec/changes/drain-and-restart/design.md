# Design

## Context

A Rex server restart or upgrade ends every program in every pane, so it cuts Claude Code turns, background tasks and monitors without warning.
[rex-supervisor][supervisor] gives rawr a daemon outside every pane, the mod inside each session, and each pane's status from Rex; this change uses all three to wind sessions down before a restart.

None drains agents before a restart.
The closest first-party models are Claude Code's own supervisor, which defers its restart while any session is mid-turn, and the self-hosted runner's drain ([agent view][cc-agent-view], [self-hosted deploy][cc-runner]).

## Decisions

1. **A drain is a request in a file, and nothing types text into a terminal.**
   - `rawr drain [--reason TEXT]` makes the daemon write one request to `$XDG_STATE_HOME/rawr/drain.yaml`: an ID, the reason, and the time it opened.
   - Each session's mod reads that file every 2 seconds with `$.fs.read`, and acts once per request ID.
     A session that starts while a drain is open reads it on its first read, like any other.
   - An idle session gets the hand-off request through `$.prompt.submit`, which Claude Code marks as sent by the rawr mod.
   - A busy session gets it through `$.session.append` before its next step.
   - When that busy session has a Bash or Agent call running in the foreground, the mod at once asks the binary to press the background key in its own block, with `rawr drain background`.
     Claude Code binds `ctrl+b` and `ctrl+x ctrl+b` to `task:background` while a task runs in the foreground; with `CLAUDE_CODE_KB_COHESION_FIXES` set, only the chord works.
     The binary inherits Claude Code's environment through `$.process.run`, so it sends the single byte `0x02` (`ctrl+b`) unless that variable is set, and then both chord bytes `0x18 0x02` in one `com.superlogical.terminal.write` call on the block.
     A single control byte cannot merge with text the user is typing, as a typed message would; the chord can, if the user's own key lands between its two bytes.
     Each backgrounded call returns at once with a "running in the background" result, so the turn reaches its next step, and the request, without waiting for the call.
     The mods API has no call that backgrounds a task, so a key is the one way.
   - The mod marks a Bash or Agent call in flight when its `tool.call` hook starts and clears it when `next(e)` resolves, so it asks for the key only while a call runs.
     Right before the write, the binary reads the block's program status and writes only while the root record reads `working`; a permission prompt reads `blocked`, so the key does not go to one that is already open.
     The read and the write go out back to back on one connection, so the gap between them is the time between two messages.
     A call that ends inside that gap leaves the key in the prompt box, where Claude Code 2.1.323 moves the cursor one character left on `ctrl+b`, as emacs does; it deletes nothing.
     No `ctrl+x` chord in the prompt box ends in `ctrl+b` (its chords end in `ctrl+k`, `enter`, `ctrl+s`, `ctrl+e`, `ctrl+a` and `tab`).
     A permission prompt that opens inside the gap, for the next call or for a subagent's tool under a running Agent call, receives the key instead.
   - A busy session that has not settled after `drain.interrupt` (default 2 minutes) gets its running turn cancelled with `$.turn.abort`, which stops the turn's running tools, and, once that turn's `turn.complete` arrives with `reason` `aborted` or `error`, the hand-off request through `$.prompt.submit`.
     That happens once per request ID, only for a request delivered through `$.session.append`; the submitted hand-off turn becomes the turn that carries the request, and it is not interrupted again.
     It covers what the key cannot: a long model response, or a tool other than Bash and Agent.
   - The mod acknowledges a request with `rawr drain ack <id>` on `turn.complete` of the turn that carried it.
     After a `$.turn.abort` for `drain.interrupt`, the mod waits for the cancelled turn's `turn.complete`: with `reason` `aborted` or `error` it submits the hand-off and acknowledges only when the hand-off turn completes; with `reason` `answer` or `refusal` the turn ended before the abort took effect, so it submits no hand-off and acknowledges on that completion.
     That event fires however the turn ends: its `reason` is `answer`, `aborted`, `refusal` or `error`.
     A user who presses Esc during the hand-off turn also ends it; the pane then settles without a hand-off, since the user stepped in.
     The acknowledgement sits under the session's block, keyed by the drain's ID and the session's ID, and counts only for the block's current session, so neither an earlier drain's acknowledgement nor an earlier session's in the same pane counts.
   - The request text covers finishing the current step, stopping each background task, committing or noting what is unfinished, writing a hand-off, starting no new work, and ending the turn.
     A restart ends every background task anyway, so stopping one in the hand-off loses nothing, and its note says how to start it again.
     A background task keeps its child record, so a block whose session leaves one running does not settle, and the restart names it when `restart.wait` runs out.
     The mods API has no call that stops a task, so the session stops it with its own tools.

   Delivery does not depend on `crossSessionInbound` or on the inbox socket.
   Alternative: Claude Code's per-session inbox socket.
   It lost: delivery depends on each session's inbound policy, the message arrives framed as another session's, and the message format is not documented ([cross-session messaging][cc-messaging]).
2. **A restart waits for idle, within a bound.**
   - `rawr restart` opens a drain.
   - The daemon then holds the restart until every live Claude block has settled, re-checking on each status event.
   - A block has settled when it has acknowledged the drain's ID and reads Idle or Failed with no child record in `working`.
     Since the acknowledgement comes at the end of the hand-off turn, an Idle reading from before that turn started cannot count.
   - The set of blocks is live: a block whose Claude exits or whose pane closes leaves it, and a Claude that starts during the drain joins it.
   - A block that reads Needs input is named at once, so the user can answer it, and the wait goes on.
   - A block whose session has no rawr mod, such as one started before `rawr setup`, is named at once as one that cannot acknowledge.
     It counts as settled once it has read Idle with no child record in `working` for `drain.grace` (default 1 minute).
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

## Not verified

1. How a stopped Rex server starts again on macOS when the app started it, and whether `rex server install` is the right owner there.
2. Whether `$.prompt.submit` keeps a half-typed prompt in the session's prompt box.
3. Whether a byte written by Rex reaches Claude Code as `ctrl+b`, or the pair as `ctrl+x ctrl+b`, while Claude Code has the kitty keyboard protocol on.
4. What the prompt box does with `ctrl+x` followed by a key no chord binds, which only the chord path can leave there.
5. What a permission prompt does with `ctrl+b` or `ctrl+x ctrl+b`; Claude Code 2.1.323 binds neither in its `Confirmation` context.

[cc-agent-view]: https://code.claude.com/docs/en/agent-view
[cc-runner]: https://code.claude.com/docs/en/self-hosted-environments-deploy
[cc-messaging]: https://code.claude.com/docs/en/cross-session-messaging
[supervisor]: ../rex-supervisor/design.md
