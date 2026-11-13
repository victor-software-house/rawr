# Tasks

## 1. Daemon

- [ ] 1.1 `rawr daemon` binds `daemon.sock` under `daemon.lock`, leaves the terminal's session, prints `ready`, follows Rex and sends `panes` and `link`; proved by tests with a fake Rex server and a socket in a temporary folder, in CI.
- [ ] 1.2 The daemon refuses a different build and shuts down on `shutdown`; proved by handshake tests in CI.
- [ ] 1.3 `facts` and `search` run in the daemon, a newer search ends an older one, and the caches go with the last client; proved by tests in CI.
- [ ] 1.4 A connection with 64 queued messages is closed; proved by a test in CI.

## 2. Client

- [ ] 2.1 `rawr` connects, starting the daemon when none answers, and draws the first `panes`; proved by a test that starts two clients with no daemon, in CI.
- [ ] 2.2 The window asks for facts and hits over the link and reconnects with backoff, showing `stale`; proved by tests in CI.
- [ ] 2.3 The maintainer watches the preview build go to a pane in another Rex session with no blank or late frame; recorded here.
