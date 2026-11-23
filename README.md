# rawr

![rawr: Rex Agents, wake and resume](assets/rawr-banner-light.svg#gh-light-mode-only)
![rawr: Rex Agents, wake and resume](assets/rawr-banner-dark.svg#gh-dark-mode-only)

<p align="center">
  <b>Rex Agents: Wake and Resume.</b><br>
  Bring Claude Code agents back into their panes after a <a href="https://www.superlogical.com/rex">Rex</a> server restart.
</p>

<p align="center">
  <a href="#get-started">Get started</a> ·
  <a href="#the-app">The app</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#shell-integration">Shell integration</a>
</p>

Rex restores sessions, tabs and panes after a restart, but each pane comes back as a fresh shell.
`rawr` remembers which Claude Code sessions ran in each pane, and offers them again in a full-screen app.
Choosing one resumes it in that pane; quitting the app leaves a plain shell.

## Get started

1. Install the binary from the [releases], built for macOS on Apple silicon and Linux on x64:

   ```sh
   mise use -g github:victor-software-house/rawr
   ```

   Or build it with Cargo: `cargo install --git https://github.com/victor-software-house/rawr --locked`.

2. Record each Claude Code session under its pane, once:

   ```sh
   rawr setup claude
   ```

3. Offer a restored pane its sessions when its shell starts, with the line in [Shell integration](#shell-integration).

Then type `rawr` in any pane.

## The app

Bare `rawr` opens the app on the alternate screen, so the pane's output is back unchanged when it closes.
It lists every Rex pane that has a Claude session and what its Claude is doing, kept current from Rex's events.
Panes that need you come first, then Failed, Done, Working, Idle and Stopped, then the sessions of the last day whose pane is gone.

The selected pane shows its sessions, and the session shown has four tabs:

| Tab | Shows |
| --- | --- |
| Overview | The model, branch, prompt and reply counts, tokens out, sub-agents, when it started and last wrote, and its task list |
| Conversation | What you typed and what Claude answered, newest first |
| Activity | Everything, with each tool call and its result |
| Files | The files the session changed and read; a click opens a file in its default app, and a click on its folder narrows the list |

Each entry is folded to a few lines with a `+N` count of the rest; a click opens it, with its Markdown drawn and its code highlighted.

| Action | Keys | Mouse |
| --- | --- | --- |
| Focus the panes, or the pane and its session | `1` `2`, `Tab` | Click a column |
| Walk the session's tabs | `←` `→`, `h` `l`, `[` `]` | Click a tab |
| Move the selection, or scroll the session | `↑` `↓`, `j` `k` | Click a row, or scroll the wheel |
| Go to the pane in the Rex app | `Enter` on a pane, `g` | Click the selected pane |
| Resume the session | `Enter` on a session, `r` | Click the selected session |
| Filter what has focus | `/`; `Esc` clears it | |
| Search every local session | `Ctrl+F`; `Esc` closes it | |
| One column at a time, or two | `Ctrl+L` | |
| Show the keys | `?` | |
| Back to the panes; quit from there | `Esc`, `q`, `Ctrl+C` | |

Going to a pane asks the Rex app to show it, which needs Remote Control turned on in the app.
A resume in a pane tiled in its tab becomes `claude --resume` there; from a floating window it opens in a new tab of the pane's session.

Search matches every word typed, ignoring case, against the prompts typed in a session, its title, its folder and its ID.

## How it works

Rex keeps a pane's block ID across a server restart, so the block ID is the pane's identity.

| Command | Run by | Does |
| --- | --- | --- |
| `rawr` | You, from any shell | Opens the app |
| `rawr --startup` | The shell, when it starts in a Rex pane | Opens the app on the pane's sessions when one can be resumed; otherwise prints nothing and exits 0 |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index, then deletes the indexes of panes the Rex server no longer lists |
| `rawr daemon` | The first `rawr` window, or `brew services` | Follows Rex and reads transcripts and the search catalog for every window |
| `rawr status [--format json]` | You, from any shell | Lists each Rex pane that has a session with its newest session's title and folder, and what its Claude is doing |
| `rawr setup claude [--dry-run]` | You, once | Adds the `SessionStart` hook that runs `rawr record` to Claude Code's user settings |
| `rawr completion bash\|zsh\|fish` | Your shell, at start | Prints the shell's completion script; Tab then completes from the installed binary |

Each `rawr` window is a client of one `rawr daemon`, which it starts when none answers on `~/.local/state/rawr/daemon.sock`.
The daemon reads each transcript from its newest line back, so a long session shows its latest entries at once.

## Shell integration

On a resume, `rawr` restores the terminal and becomes `claude --resume` in the session's folder; when Claude Code exits, the pane's shell prompt is back.
It needs no wrapper.
In zsh, offer a restored pane its sessions once, before the first prompt:

```zsh
if [[ -n $REX_BLOCK ]] && (( $+commands[rawr] )); then
  autoload -Uz add-zsh-hook
  _rawr_once() {
    add-zsh-hook -d precmd _rawr_once
    rawr --startup
  }
  add-zsh-hook precmd _rawr_once
fi
```

`record` runs from a Claude Code `SessionStart` hook.
`rawr setup claude` adds it to `~/.claude/settings.json`, or to `$CLAUDE_CONFIG_DIR/settings.json`, and keeps every other setting in its order; `--dry-run` reports what it would do and changes nothing.
A second run reports the hook is there and changes nothing.
It edits the file only when you run it.
The hook it adds:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "rawr record", "timeout": 10 }
        ]
      }
    ]
  }
}
```

Load completion at shell start: `source <(rawr completion zsh)` in `~/.zshrc` after `compinit`, `source <(rawr completion bash)` in `~/.bashrc`, or `rawr completion fish | source` in `~/.config/fish/config.fish`.

## License

MIT

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/rawr-icon-dark.svg">
    <img src="assets/rawr-icon-light.svg" alt="The rawr icon: a small dinosaur asleep in a terminal window" width="96">
  </picture>
</p>

[releases]: https://github.com/victor-software-house/rawr/releases
