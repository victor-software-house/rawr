# Tasks

## 1. pkg

- [ ] 1.1 `Bundle/` with `distribution.xml`, the component packages and their postinstalls; proved by the pkg check in CI.
- [ ] 1.2 The updater daemon; proved by installing an older pkg on a test Mac, which then reports the newer version.
- [ ] 1.3 The release workflow builds, notarizes and attaches `rawr.pkg`, and the tap gains a `rawr` cask; proved by the release run.

## 2. packslip

- [ ] 2.1 `.ctl/packslip.toml` after verctl publishes packslips; proved by `mise use packslip:github.com/victor-software-house/rawr` and a completion.
