# Proposal

## Why

rawr installs from a Homebrew formula or a mise `github:` entry.
mise users get no completions that match their version, and no release is signed as a whole.
Mac users have no installer that sets rawr up and keeps it current.

The user gains two more channels, each updating by its own means:

1. mise through a signed [packslip][packslip], with completions that follow the installed version.
2. A signed, notarized pkg and a cask, whose Installer offers the setup pieces and whose updater keeps rawr current.

## What Changes

1. Each release carries a packslip bundle, from verctl's release workflow once it publishes one.
2. Each release carries `rawr.pkg`, signed with a Developer ID, notarized and stapled.
3. The pkg's Installer offers two choices under Customize, the shell line and the Rex module, both on.
4. The pkg installs a root updater that installs a newer release by itself, as signd's does; the cask sets `auto_updates true`.

## Capabilities

### New Capabilities

- `channels`: what each install channel installs and how it stays current.

### Modified Capabilities

None.

## Impact

1. New `Bundle/` with `distribution.xml`, the postinstall and the updater's launchd property list.
2. The release workflow builds the pkg on a macOS runner and sends the tap a cask bump.
3. `.ctl/packslip.toml` once verctl publishes packslips.

[packslip]: https://packslip.dev
