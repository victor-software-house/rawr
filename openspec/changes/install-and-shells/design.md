# Design

## Context

Tools that hook into the shell split into two camps.
[mise], [zoxide] and [starship] print the line to add and leave the shell files alone; their installers end with instructions.
The [uv] installer, built by cargo-dist, appends to shell profiles unless `UV_NO_MODIFY_PATH` is set.
rawr follows the first camp.

## Decisions

0. **ctl-core owns the shell syntax.**
   `init` and `completion` come from ctl-core features that every ctl CLI shares.
   rawr declares its one hook, running `pick` once before the first prompt in a Rex pane, and ctl-core renders it for each shell.
   Alternative: rawr ships its own scripts for each shell.
   It lost: every ctl CLI that hooks into a shell would repeat the same hook code, and fix the same bugs one CLI at a time.
1. **`init` prints; nothing writes shell files.**
   The installers print `eval "$(rawr init zsh)"`, or the bash and fish forms, and stop.
   Alternative: append that line to `~/.zshrc`, `~/.bashrc` or `config.fish`.
   It lost: it edits files the user may manage elsewhere, such as in a dotfiles repository, and does it without asking.
2. **`init` output is static.**
   It depends only on the shell and the rawr version, never on the machine, so it can be saved once with `rawr init zsh > ~/.config/zsh/rawr.zsh` and sourced with no process at shell start.
   Alternative: `eval "$(rawr init zsh)"` as the only form.
   It lost as the only form: it runs rawr in every new shell, a few milliseconds that add to start time; it stays the shortest form for those who accept that.
3. **bash, zsh and fish.**
   These are the shells Homebrew installs completions for, and each has a hook that runs once before the first prompt: `PROMPT_COMMAND` in bash, `precmd` in zsh, the `fish_prompt` event in fish.
   Alternative: also nushell, PowerShell and others that mise supports.
   It lost for now: their hooks differ again, and nobody has asked for them.
4. **Nothing is quoted for a shell.**
   `pick` and `find` resume the chosen session themselves (fullscreen-picker design, decision 12), so no shell evaluates a line rawr prints.
5. **`setup claude` is explicit and idempotent.**
   It reads `$CLAUDE_CONFIG_DIR/settings.json` or `~/.claude/settings.json`, adds the hook only if no `SessionStart` hook already runs `rawr record`, keeps every other key as it was, and reports `added` or `unchanged`.
   This follows `rex skill setup`, which edits Claude Code settings only when run.
6. **Completions come from ctl-core's `completion` verb.**
   It prints a short registration script that asks the installed binary for candidates at each TAB, so completions always match the installed version.
   Homebrew installs it with `generate_completions_from_executable`, with no shell configuration.
7. **The install script installs one binary and checks it.**
   It detects the OS and CPU, downloads the matching tarball and its `.sha256` from the release, verifies the digest, and installs `rawr` into `~/.local/bin`, or `RAWR_INSTALL_DIR`.
   It needs only `sh`, `curl` or `wget`, `tar`, and `shasum` or `sha256sum`.
8. **The formula lives in a tap of its own.**
   `victor-software-house/homebrew-tap` holds formulae for the organization's own tools; `homebrew-forks` stays for patched forks.
   The formula installs the binary and generates completions with `generate_completions_from_executable`, and its caveats print the `init` line.
9. **Targets: macOS on Apple silicon, Linux on glibc.**
   `darwin-arm64`, `linux-x64` and `linux-arm64`, built on free GitHub-hosted runners.
   Intel Macs and musl builds are out.

[mise]: https://mise.jdx.dev/getting-started.html
[zoxide]: https://github.com/ajeetdsouza/zoxide#installation
[starship]: https://starship.rs/guide/
[uv]: https://docs.astral.sh/uv/configuration/installer/
