# Design

## Context

The app in [rawr-app][rawr-app] runs three producers inside each window: a Rex feed thread, a transcript facts worker and a search worker.
Each window pays for all three, and a second window, such as the copy that going to a pane starts in another Rex session, pays again from the start.
Rex keeps a block in its session, so that second copy cannot be avoided; what can be avoided is its starting empty.

The maintainer's `a` project is the prior art for the split.
Its daemon holds sessions and journals, its ratatui client holds the view, and they speak framed messages over a Unix socket in the daemon's state folder.
The client starts the daemon under a kernel file lock when none answers, and reads one readiness line before connecting.
Several clients attach at once, each with its own view, and the daemon keeps nothing of a client once it disconnects.
A slow client is disconnected rather than buffered without bound, and reconnects by itself.

rawr is smaller: one binary is both sides, the daemon only reads, and every message fits in one line.

## Decisions

1. **One daemon holds what every window shares.**
   - `rawr daemon` keeps the Rex connection and the pane list built from it, the facts of each transcript read so far, and the search catalog.
   - It reads only: actions on the Rex app, such as going to a pane, opening a tab or reporting the app's own status, stay in the client, because they act for the person at that window.
   - Considered: a shared cache file that each window reads.
     It lost: a file cannot push a status change, and each window would still follow Rex itself.
2. **JSON lines over a Unix socket.**
   - The socket is `$XDG_STATE_HOME/rawr/daemon.sock` (default `~/.local/state/rawr/daemon.sock`), mode 0600 in a 0700 folder.
   - Each message is one JSON object on one line, tagged by `type`.
   - Considered: Protobuf in length-prefixed frames, as `a` uses.
     It lost: both sides are the same binary, so no schema has to bind two programs, and `serde_json` is already a dependency.
3. **The handshake compares the binary, not a version number.**
   - The client sends `hello` with its build: the path, size and modification time of its own executable.
   - The daemon answers `welcome` when the build matches, or `refused` with its own build.
   - On `refused`, the client sends `shutdown`, waits for the socket to close, and starts its own daemon.
   - Considered: a protocol version, as `a` compares its major version.
     It lost: a rebuilt or upgraded binary keeps its version during development, and the old daemon would serve old code.
4. **The first client starts the daemon.**
   - With no answer on the socket, the client takes an exclusive lock on `daemon.lock`, checks the socket again, and starts `rawr daemon` detached in its own session, with its standard streams closed but one.
   - The daemon binds the socket, then prints `ready` on that stream; the client waits for that line, up to 5 seconds, and connects.
   - The daemon holds the same lock for its life, so a second daemon exits at once.
   - The `brew services` unit from [setup-and-tap][setup-and-tap] runs the same command at login.
   - Considered: only the service starts the daemon.
     It lost: a preview build and a fresh install would show no panes until the service is set up.
5. **The daemon pushes; the client asks.**

   | From | Message | Carries |
   |:--|:--|:--|
   | client | `hello` | the build |
   | daemon | `welcome`, `refused` | the daemon's build |
   | daemon | `panes` | every pane, in full, at once after `welcome` and after each change |
   | daemon | `link` | `live`, `offline` or `stale`, with the reason |
   | client | `facts` | a transcript path |
   | daemon | `facts` | the path and its facts, once read |
   | client | `search` | a query ID, the query and the sessions to rank first |
   | daemon | `hits` | the query ID, a batch of hits, and how many transcripts are read of how many |
   | client | `shutdown` | nothing |

   - `panes` is a full snapshot, so a client that missed one needs no replay.
   - A newer `search` from the same client ends the older one between transcripts.
   - Considered: a request and response for each list.
     It lost: the panes change on Rex's events, not on the client's asks.
6. **A slow client is dropped, not waited for.**
   - Each connection has a queue of 64 messages; a full queue closes the connection.
   - The client reconnects at once, then after 100 ms, doubling to 2 seconds, and shows `stale` while it does.
   - Considered: an unbounded queue.
     It lost: a stopped client would grow the daemon's memory until it resumed.
7. **View state stays in the client.**
   - Selection, focus, query, sidebar width and fold, and scroll live in `App`, as in `a`, where no client's view binds another's.
   - The copy that going to a pane starts gets them from the copy it replaces, on its command line.
   - Considered: the daemon keeps each view by an ID.
     It lost: two windows would then share one selection, and the daemon would hold state no Rex event changes.
8. **The caches go with the last client.**
   - The facts and the catalog are dropped when the last client disconnects, and built again on the next ask.
   - The Rex connection and the pane list stay, so a new window draws at once.
   - Considered: keeping the caches while the daemon runs.
     It lost: they hold every transcript read since login, for windows that are closed.

## Not verified

1. How long the catalog takes to build again in the release build after the last client leaves.

[rawr-app]: ../rawr-app/design.md
[setup-and-tap]: ../setup-and-tap/design.md
