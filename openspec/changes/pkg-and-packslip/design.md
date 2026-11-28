# Design

## Context

rawr ships today as two tarballs, a Homebrew formula that a release dispatch moves, and a mise `github:` entry.
[signd][signd] ships as a notarized pkg with a cask and keeps itself current with a root updater daemon ([signd README][signd]).

## Decisions

### 1. How each channel updates

| Channel | Installs | Stays current through |
|:--|:--|:--|
| mise, `packslip:` | binary, completions | mise's `tool_update.global_auto` setting, which installs a newer `latest` tool on the next mise run |
| Homebrew formula | binary, completions, `brew services` unit | `brew upgrade` |
| pkg and cask | `/usr/local/bin/rawr`, `rawr.lua`, the updater | the updater; the cask sets `auto_updates true` |

Alternative: one channel.
It lost: mise and Linux users need the packslip, and the Mac install that sets everything up needs the pkg (operator, 2026-11-27).

### 2. The updater

A root launch daemon checks the latest GitHub release every 6 hours and installs a newer `rawr.pkg` only when it is notarized and signed by the updater's own team, as signd's updater does.
After an install, each user's rawr daemon finds a different binary at its next client's handshake and is replaced ([app-daemon][app-daemon] decision 3).

### 3. Installer choices

`distribution.xml` sets `customize="allow"` and lists two visible choices, "Shell line" and "Rex module", both selected, beside the binary, which is required.
Each choice installs a component package whose payload is one marker file under `/Library/Application Support/rawr/setup/`, so no install script decides which choices were taken.
The first rawr daemon of each user reads those markers and applies the chosen pieces with `rawr setup` ([setup-and-tap][setup-and-tap] decision 4), so nothing running as root writes into a user's home.
signd's pkg sets `customize="never"` and has no choices, so these choices are new here.

`brew install --cask` and `installer -pkg` take the default choices without asking, so both pieces are written for every user of that Mac.
A user who wants neither runs `rawr setup remove`.

### 4. Install and uninstall

The pkg installs `/usr/local/bin/rawr`, the updater's launch daemon `/Library/LaunchDaemons/com.victor-software-house.rawr.updater.plist`, and the marker files.
`brew uninstall --cask rawr` runs `rawr setup remove` for the user who runs it, unloads the updater and deletes the package's files.

### 5. `AGENTS.md` rule 4 changes

The updater is a second program rawr ships that starts by itself, as root, and installs only rawr's own signed packages.

### 6. packslip waits for verctl

verctl's [packslip design][verctl-packslip] publishes a signed bundle beside the tarballs.
rawr adds `.ctl/packslip.toml` with the binary and its completions once verctl releases it; until then mise keeps the `github:` entry.

## Not verified

1. That a root updater can replace a binary a running rawr daemon uses without breaking open windows.

[signd]: https://github.com/victor-software-house/signd/blob/49bd5b540ab11d371df454c613a705187572d7cd/README.md#updates
[verctl-packslip]: https://github.com/victor-software-house/verctl
[app-daemon]: ../app-daemon/design.md
[setup-and-tap]: ../setup-and-tap/design.md
