# Design

## Context

rawr needs three pieces beside its binary: a way for each Claude Code session to record itself under its Rex pane, a line in the shell's startup file, and the `rawr daemon` service that [rex-supervisor][supervisor] adds.
Today each is wired up by hand, and the session hook finds `rawr` only through whatever `PATH` started Claude Code.

Prior art for the shell line: `direnv hook`, `zoxide init` and `starship init` take the shell as a required argument and print a snippet; `brew shellenv` and mise detect the shell from the running process, then `$SHELL`; none of them edits a startup file on its own.

## Decisions

1. **Homebrew is the install, with a service.**
   - `victor-software-house/homebrew-tap` holds a `rawr` formula.
   - The formula installs the binary and generates completions with `generate_completions_from_executable`.
   - Its `service do` block runs `rawr daemon`, so `brew services start rawr` runs the daemon under launchd on macOS and a systemd user unit on Linux ([Homebrew services][brew-services]).
   - The binary sits at `$(brew --prefix)/opt/rawr/bin/rawr`, a path that survives upgrades.
   - The caveats print `rawr setup`; the formula writes nothing under the user's home.

   Alternative: the install script or `mise use -g` as the main path.
   They lost: neither registers a service.
   They stay for Linux machines without Homebrew.
2. **The shell offers a restored pane its sessions.**
   - A line in the shell's startup file runs `rawr pick` once, before the first prompt, so each restored pane with sessions opens the full-screen picker when Rex starts.
   - `rawr setup` prints that line for the user's shell; `rawr init <shell>` prints the hook it loads, from ctl-core's shared shell integration.
   - `rawr init` takes the shell as a required argument, as `direnv hook`, `zoxide init` and `starship init` do; `rawr setup` picks the shell for the line it prints from the running shell, then `$SHELL`, as `brew shellenv` does, through ctl-core's shell detection (CTC-057).
   - Like those `init` commands, `rawr setup` prints the line and edits no startup file.
   - The hook unhooks itself after its first run, since every shell's prompt hook fires on each prompt: `add-zsh-hook precmd` in zsh, `PROMPT_COMMAND` in bash, a `fish_prompt` event in fish.
   - bash, zsh and fish come first, the shells ctl-core's `Shell` holds; nushell and PowerShell, which mise and direnv also cover, follow when ctl-core adds them.
   - Alternative: the daemon types `rawr pick` into each restored shell, as the "Resume Claude here" Rex action does.
     It lost: typed input can collide with what the user types first, and nothing else in rawr types into a terminal.
3. **`rawr setup` installs everything after the binary, and only what it names.**
   - `rawr setup` adds this repository's marketplace and installs the plugin through Claude Code's plugin commands.
   - It stores the binary's stable path in the plugin's data folder and starts the service when Homebrew installed rawr.
   - It removes an old `SessionStart` hook that runs `rawr record`, when it finds one.
   - `rawr setup status` reports each piece as current, outdated, missing or foreign, as `rex skill status` does.
   - `rawr setup remove` removes only what `rawr setup` installed.
4. **`AGENTS.md` rule 4 changes.**
   The binary also talks to Claude Code's plugin commands during `rawr setup`.
   Nothing resumes without a choice, as before.

[supervisor]: ../rex-supervisor/design.md
[brew-services]: https://docs.brew.sh/Formula-Cookbook#service-files
