# Proposal

## Why

Each `rawr` window does all the work itself.
It connects to Rex, lists every pane, reads every transcript it shows and searches every local session, then throws it all away when it quits.
Going to a pane in another Rex session starts a second copy, because a block stays in its session, and that copy starts from nothing.
It reads Rex again, reads the transcripts again, and the maintainer watched it arrive late, blank and resizing, with the search and selection gone.

## What Changes

1. `rawr daemon` runs outside every pane and holds what all windows share: the Rex feed, each pane's status, the transcript facts and the search catalog.
2. Bare `rawr` becomes a client of it over a Unix socket.
   Its first frame draws the daemon's current panes, with no Rex listing of its own.
   It asks the daemon for a transcript's facts and for search hits, and draws them as they arrive.
3. A client starts the daemon when none answers.
   The `brew services` unit from [setup-and-tap][setup-and-tap] keeps one running from login.
4. A client whose binary differs from the daemon's stops that daemon and starts its own, so an upgrade never talks to old code.
5. View state stays in the client: selection, focus, query, sidebar and scroll.
   A copy started by going to a pane receives it from the copy it replaces.
6. Actions on the Rex app stay in the client: going to a pane, resuming, and the app's own status record.

## Capabilities

### New Capabilities

- `daemon-link`: the socket, the handshake, the messages, starting and replacing the daemon, and what each side holds.

### Modified Capabilities

None.
[rex-supervisor][supervisor] adds pruning and restarts to the same daemon.

## Impact

1. Modules: new `src/daemon.rs` with `src/daemon/{server,link}.rs`; `src/app.rs` and `src/app/feed.rs` read from the link; `src/app/facts.rs` and `src/catalog.rs` run in the daemon; `src/main.rs` gains `rawr daemon`.
2. Dependencies: none new; the messages are JSON lines through `serde_json`.

[setup-and-tap]: ../setup-and-tap/design.md
[supervisor]: ../rex-supervisor/design.md
