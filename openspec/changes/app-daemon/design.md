# Design

## Context

The app in [rawr-app][rawr-app] ran three producers inside each window: a Rex feed thread, a transcript facts worker and a search worker.
Each window paid for all three, and a second window, such as the copy that going to a pane starts in another Rex session, paid again from the start.
Rex keeps a block in its session, so that second copy cannot be avoided; what can be avoided is its starting empty.

An earlier agent supervisor the maintainer wrote is the prior art for the split.
Its daemon holds sessions and journals, its ratatui client holds the view, and they speak framed messages over a Unix socket in the daemon's state folder.
The client starts the daemon under a kernel file lock when none answers, and reads one readiness line before connecting.
Several clients attach at once, each with its own view, and the daemon keeps nothing of a client once it disconnects.
A slow client is disconnected rather than buffered without bound, and reconnects by itself.

rawr is smaller: one binary is both sides, and every message fits in one line.

## Decisions

1. **One daemon holds what every window shares, and acts in Rex for it.**
   - `rawr daemon` keeps the Rex connection and the pane list built from it, the sessions no open pane holds, what it has read of each transcript, and the search catalog.
   - It also runs the window's actions in Rex: going to a pane, handing a floating window over to a copy in another tab, and resuming a session in a tab of its own.
     The window only asks, and the daemon answers with a `notice` when an action fails.
   - A resume in the window's own pane is the one action the window does itself: the daemon answers `exec`, and the window gives the terminal back and becomes `rawr <agent>` with the resume arguments and the recorded flags ([agent-launcher][agent-launcher] decision 4).
   - Considered: actions stay in the window.
     It lost: a hand-off has to close the old window after the new one draws, which only a process outside both windows can do.
   - Considered: a shared cache file that each window reads.
     It lost: a file cannot push a status change, and each window would still follow Rex itself.

2. **JSON lines over a Unix socket.**
   - The socket is `$XDG_STATE_HOME/rawr/daemon.sock` (default `~/.local/state/rawr/daemon.sock`), mode 0600 in a 0700 folder.
   - Each message is one JSON object on one line, tagged by `type`.
   - Considered: Protobuf in length-prefixed frames, as that supervisor uses.
     It lost: both sides are the same binary, so no schema has to bind two programs, and `serde_json` is already a dependency.

3. **The handshake compares the binary, not a version number.**
   - The client sends `hello` with its build: the path, size and modification time of its own executable.
   - The daemon answers `welcome` when the build matches, or `refused` with its own build.
   - On `refused`, the client sends `shutdown`, waits for the socket to close, and starts its own daemon.
   - Considered: a protocol version, as that supervisor compares its major version.
     It lost: a rebuilt or upgraded binary keeps its version during development, and the old daemon would serve old code.

4. **The first client starts the daemon.**
   - With no answer on the socket, the client takes an exclusive lock on `start.lock`, checks the socket again, and starts `rawr daemon`, which leaves the terminal's session at once.
   - The daemon binds the socket, then prints `ready` on that stream; the client waits for that line, up to 5 seconds, and connects.
   - The daemon holds `daemon.lock` for its life, so a second daemon exits at once; a client never waits on that lock, so a daemon that stops answering cannot hold a client.
   - The `brew services` unit from [setup-and-tap][setup-and-tap] runs the same command at login.
   - Considered: only the service starts the daemon.
     It lost: a preview build and a fresh install would show no panes until the service is set up.

5. **The daemon pushes; the client asks.**

   | From | Message | Carries |
   |:--|:--|:--|
   | client | `hello` | the build, the Rex pane the window runs in, and the view a hand-off started it for |
   | daemon | `welcome`, `refused` | the daemon's build; `welcome` adds the window's view ID and, after a hand-off, the view of the window it replaces |
   | daemon | `panes` | every pane, in full, at once after `welcome` and after each change |
   | daemon | `link` | `live`, `offline` or `stale`, with the reason |
   | client | `drawn` | nothing; the window has drawn, so the one it replaces may close |
   | client | `facts` | a transcript path |
   | daemon | `facts` | the path and its facts so far, as they are read |
   | client | `search` | a query ID, the query and the pane whose sessions rank first |
   | daemon | `hits` | the query ID, a batch of hits, and how many transcripts are read of how many |
   | client | `go_to` | the pane's session and block, and the window's view |
   | client | `resume` | the session to resume, and the Rex session to open a tab in when the window floats |
   | daemon | `exec` | the session, and the `rawr <agent>` arguments the window should become |
   | daemon | `notice` | one line for the window's status bar, such as why an action failed |
   | client | `shutdown` | nothing |

   - `panes` is a full snapshot, so a client that missed one needs no replay.
   - A newer `search` from the same client ends the older one between transcripts.
   - A newer `facts` from the same client ends the older read between parts.
   - Considered: a request and response for each list.
     It lost: the panes change on Rex's events, not on the client's asks.

6. **A slow client is dropped, not waited for.**
   - Each connection has a queue of 64 messages; a full queue closes the connection.
   - The client reconnects at once, then after 100 ms, doubling to 2 seconds, and shows `stale` while it does.
   - Considered: an unbounded queue.
     It lost: a stopped client would grow the daemon's memory until it resumed.

7. **View state stays in the client.**
   - Selection, focus, arrangement, tab, query, filters, column widths and scroll live in `App`, as in that supervisor, where no client's view binds another's.
   - `go_to` carries the window's view to the daemon, which starts the copy over the pane as `rawr --view <id>` and gives it the view in `welcome`.
     The daemon keeps that view only until the copy connects.
   - Considered: the copy reads the view from its command line.
     It lost: the view holds the query and filters the user typed, which a command line would show in the process list and in Rex's pane title.
   - Considered: the daemon keeps each view by an ID for good.
     It lost: two windows would then share one selection, and the daemon would hold state no Rex event changes.

8. **A transcript is read from its newest line back.**
   - The daemon reads a transcript in 256 KiB parts from its end, so the newest prompts and calls show first, however long the file.
   - It sends the facts once the first part holds a whole entry, then at most every 50 ms, and once more when the read reaches the start.
     The window shows `reading older lines…` until that last message.
   - A tool's result is matched to its call by ID across parts, and task-list operations are replayed in their written order once both are read.
   - When the window asks for another transcript, the read stops between parts and keeps what it read, so coming back goes on where it stopped.
     Lines written since are read from where the last read ended.
   - The task list Claude Code keeps under `~/.claude/tasks/` wins over the one the transcript's calls give, when the session has one there.
   - Considered: reading forward to the end before sending anything.
     It lost: a 288 MB transcript took about 3 seconds to show anything, and a second click waited for the first read.

9. **Sessions no pane holds are listed too.**
   - Every 15 seconds, on its own thread, the daemon lists the transcripts written in the last day, newest first, up to 40, leaving out scripted runs started in a temporary folder.
   - Each one no open pane holds joins `panes` as a closed pane, after the open ones.
   - A Rex event never waits for this listing: `panes` goes out with the last closed list the thread found.
   - Considered: listing only what the pane index holds.
     It lost: a session whose pane closed vanished from the app, though its transcript was still there to resume.

10. **The caches go with the last client.**
    - The transcripts read and the catalog are dropped when the last client disconnects, and built again on the next ask.
    - The Rex connection, the pane list and the closed list stay, so a new window draws at once.
    - Considered: keeping the caches while the daemon runs.
      It lost: they hold every transcript read since login, for windows that are closed.

11. **Search reads each transcript from its end.**
    - The catalog scan reads each transcript in 256 KiB parts from its end and checks between parts whether a newer search ended this one.
    - Considered: reading each transcript whole.
      It lost: one large transcript held a stale search for seconds after the query changed.

## Not verified

1. How long the catalog takes to build again in the release build after the last client leaves.

[rawr-app]: ../rawr-app/design.md
[setup-and-tap]: ../setup-and-tap/design.md
[agent-launcher]: ../agent-launcher/design.md
