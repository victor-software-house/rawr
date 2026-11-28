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
   - A line in the shell's startup file runs `rawr resume` once, before the first prompt, so each restored pane with sessions opens the stand-in on them when Rex starts, and defines the agent functions of [agent-launcher][agent-launcher].
   - `rawr setup` writes that line for the user's shell; `rawr init <shell>` prints the hook it loads, from ctl-core's shared shell integration.
   - `rawr init` takes the shell as a required argument, as `direnv hook`, `zoxide init` and `starship init` do; `rawr setup` picks the shell for the line it writes from the running shell, then `$SHELL`, as `brew shellenv` does, through ctl-core's shell detection (CTC-057).
   - The line sources `~/.local/state/rawr/init.<shell>`, which holds `rawr init <shell>`'s output; the daemon rewrites that file when its binary changes, so a shell start runs no `rawr` process and the script follows upgrades.
   - Alternative: print the line and edit no startup file, as `zoxide init` does.
     It lost: no Rex mechanism routes a typed agent name without a startup-file line ([agent-launcher][agent-launcher] probe point 5), so setup writes it.
   - The hook unhooks itself after its first run, since every shell's prompt hook fires on each prompt: `add-zsh-hook precmd` in zsh, `PROMPT_COMMAND` in bash, a `fish_prompt` event in fish.
   - bash, zsh and fish come first, the shells ctl-core's `Shell` holds; nushell and PowerShell, which mise and direnv also cover, follow when ctl-core adds them.
   - Alternative: the daemon types `rawr resume` into each restored shell, as the "Resume Claude here" Rex action does.
     It lost: typed input can collide with what the user types first, and nothing else in rawr types text into a terminal.

3. **`rawr setup` installs everything after the binary, and only what it names.**
   - `rawr setup` adds this repository's marketplace and installs the plugin through Claude Code's plugin commands.
   - It stores the binary's stable path in the plugin's data folder and starts the service when Homebrew installed rawr.
   - It removes an old `SessionStart` hook that runs `rawr record`, when it finds one.
   - It writes the shell line into the startup file, and `require("rawr")` into Rex's `init.lua` with `rawr.lua` beside it ([agent-launcher][agent-launcher] decision 7), each between `# >>> rawr >>>` and `# <<< rawr <<<` (`--` comments in Lua).
     A second run rewrites each block in place, so a file holds at most one.
   - The Claude Code side, the plugin or, until it ships, the `SessionStart` hook, is installed without a question; the shell line and the Rex module are choices.
   - `rawr setup status` reports each piece as current, outdated, missing or foreign, as `rex skill status` does, and whether Rex has the `rawr` module's rules in force, from `rex config check --json`.
   - `rawr setup remove` removes only what `rawr setup` installed.

4. **One setup, asked once per user.**
   - The daemon owns the setup state, in `~/.local/state/rawr/setup.json`, because it is the one rawr process per user.
   - A pkg install takes the choices from Installer ([pkg-and-packslip][pkg-and-packslip]); the first daemon of each user applies them and asks nothing.
   - Any other install asks in the first rawr window: the daemon gives that window the checklist, every other window waits on it, and all of them take the result together.
   - `rawr setup` opens the same checklist again, and `rawr setup --yes` applies the defaults without asking.

5. **`AGENTS.md` rule 4 changes.**
   The binary also talks to Claude Code's plugin commands during `rawr setup`.
   Nothing resumes without a choice, as before.

[supervisor]: ../rex-supervisor/design.md
[agent-launcher]: ../agent-launcher/design.md
[pkg-and-packslip]: ../pkg-and-packslip/design.md
[brew-services]: https://docs.brew.sh/Formula-Cookbook#service-files
