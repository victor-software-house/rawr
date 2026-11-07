# Tasks

## 1. Shell integration

- [ ] 1.1 `src/shell.rs` holds the bash, zsh and fish `init` scripts and per-shell quoting; proved by unit tests in CI, including the fish quote.
- [ ] 1.2 `rawr init <SHELL>` prints them; proved by a test that two runs match byte for byte.
- [ ] 1.3 `rawr setup claude [--dry-run]` adds the hook once; proved by unit tests over a fixture settings file in CI.
- [ ] 1.4 `rawr completion <SHELL>` comes from ctl-core's completion verb; proved by a test that the bash script names every command. Blocked on a ctl-core release with the verb.

## 2. Release and install

- [ ] 2.1 `.ctl/ver.yaml` lists `darwin-arm64`, `linux-x64` and `linux-arm64`; proved by `verctl assets` printing the three.
- [ ] 2.2 `install.sh` passes shellcheck and installs into a temporary `RAWR_INSTALL_DIR` from a test release in CI.
- [ ] 2.3 `victor-software-house/homebrew-tap` holds a `rawr` formula that a release updates; proved by `brew install` and `brew test rawr` on macOS after the first release the maintainer asks for.

## 3. Verify

- [ ] 3.1 `mise run verify` passes in the pull request's CI.
- [ ] 3.2 `openspec validate --all --strict` passes.
