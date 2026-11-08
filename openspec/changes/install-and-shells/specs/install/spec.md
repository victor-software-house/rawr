# Spec Delta

## Purpose

Install rawr in one step on macOS and Linux without touching the user's shell files.

## ADDED Requirements

### Requirement: Release targets

Each release SHALL attach `darwin-arm64`, `linux-x64` and `linux-arm64` tarballs, each holding the `rawr` binary, with a `.sha256` beside each.

#### Scenario: A release

- **WHEN** a release is published
- **THEN** its assets list three tarballs, three `.sha256` files and `install.sh`

### Requirement: Install script

`install.sh` SHALL install the binary matching the machine into `$RAWR_INSTALL_DIR`, else `~/.local/bin`, after verifying its digest, SHALL write nothing else, and SHALL end by printing the `rawr init` line for the user's shell.

#### Scenario: A digest mismatch

- **WHEN** the downloaded tarball does not match its `.sha256`
- **THEN** the script installs nothing and exits non-zero

#### Scenario: Shell files are untouched

- **WHEN** the script finishes on a machine whose shell is zsh
- **THEN** `~/.zshrc` is unchanged and the last lines printed include `eval "$(rawr init zsh)"`

#### Scenario: An unsupported machine

- **WHEN** the script runs on an Intel Mac
- **THEN** it names the supported targets and exits non-zero

### Requirement: Homebrew

`brew install victor-software-house/tap/rawr` SHALL install the binary and its bash, zsh and fish completions, and SHALL print the `rawr init` line as a caveat.

#### Scenario: Install with Homebrew

- **WHEN** the user installs rawr with Homebrew
- **THEN** zsh completes `rawr` with no change to the user's shell files
