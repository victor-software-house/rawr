# Tasks

## 1. Daemon

- [ ] 1.1 `rawr daemon` binds `daemon.sock` under `daemon.lock`, leaves the terminal's session, prints `ready`, follows Rex and sends `panes` and `link`; proved by tests with a fake Rex server and a socket in a temporary folder, in CI.
- [ ] 1.2 The daemon refuses a different build and shuts down on `shutdown`; proved by handshake tests in CI.
- [ ] 1.3 `facts` and `search` run in the daemon, a newer search ends an older one, and the caches go with the last client; proved by tests in CI.
- [ ] 1.4 A connection with 64 queued messages is closed; proved by a test in CI.
- [ ] 1.5 A transcript is read from its newest line back and gives the same facts as a read forward; a newer `facts` ask stops the older read; proved by the `facts` tests in CI and by a daemon test for the stop.
- [ ] 1.6 Sessions no open pane holds join `panes` within 15 seconds, without holding back a Rex event; proved by a test of `feed::with_closed` in CI.
- [ ] 1.7 `go_to` and `resume` run in the daemon and answer `notice` or `exec`; proved by a `rex::fake` test in CI.

## 2. Client

- [ ] 2.1 `rawr` connects, starting the daemon when none answers, and draws the first `panes`; proved by a test that starts two clients with no daemon, in CI.
- [ ] 2.2 The window asks for facts and hits over the link and reconnects with backoff, showing `stale`; proved by tests in CI.
- [ ] 2.3 The maintainer watches the preview build go to a pane in another Rex session with no blank or late frame; recorded here.
