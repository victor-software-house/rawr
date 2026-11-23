# rawr

![rawr: Rex Agents, wake and resume](assets/rawr-banner-light.svg#gh-light-mode-only)
![rawr: Rex Agents, wake and resume](assets/rawr-banner-dark.svg#gh-dark-mode-only)

**Rex Agents: Wake and Resume.**
Bring Claude Code agents back into their panes after a [Rex] server restart.

[Get started](#get-started) · [The app](#the-app) · [Commands](#commands) · [Shell integration](#shell-integration)

Rex restores sessions, tabs and panes after a restart, but each pane comes back as a fresh shell.
`rawr` remembers which Claude Code sessions ran in each pane, and offers them again in a full-screen app.
Pick a session to resume it in that pane, or quit the app for a plain shell.

## Get started

1. Install the binary from the [releases], built for macOS on Apple silicon and Linux on x64:

   ```sh
   mise use -g github:victor-software-house/rawr
   ```

   Or build rawr with Cargo: `cargo install --git https://github.com/victor-software-house/rawr --locked`.

2. Add the Claude Code hook that records which pane each session runs in:

   ```sh
   rawr setup claude
   ```

3. Add the startup lines for your shell from [Shell integration](#shell-integration).
   After a Rex restart, rawr then opens in each pane that had a Claude session.

Then type `rawr` in any pane.

## The app

Bare `rawr` opens the app on the alternate screen, so the pane's output comes back unchanged when you quit.
The app lists every Rex pane that has a Claude session, shows what Claude is doing there, and updates as Rex reports changes.

Panes that need you come first, then Failed, Done, Working, Idle and Stopped, then the sessions of the last day whose pane is gone.

Select a pane to see the sessions that ran there.
Each session has four tabs:

| Tab | Shows |
| --- | --- |
| Overview | The model, branch, prompt and reply counts, tokens out, sub-agents, start time, last activity and task list |
| Conversation | What you typed and what Claude answered, newest first |
| Activity | Every entry, including tool calls and tool results |
| Files | The files the session changed and read; click a file to open it in the default app for that file type, or click a folder to list only the files inside |

The app folds each entry to a few lines, with a `+N` count of the lines left out.
Click an entry to expand it, with Markdown rendered and code highlighted.

| Action | Keys | Mouse |
| --- | --- | --- |
| Focus the pane list, or the selected pane and session | `1` `2`, `Tab` | Click a column |
| Walk the session's tabs | `←` `→`, `h` `l`, `[` `]` | Click a tab |
| Move the selection, or scroll the session | `↑` `↓`, `j` `k` | Click a row, or scroll the wheel |
| Go to the pane in the Rex app | `Enter` on a pane, `g` | Click the selected pane |
| Resume the session | `Enter` on a session, `r` | Click the selected session |
| Filter what has focus | `/`; `Esc` clears the filter | |
| Search every local session | `Ctrl+F`; `Esc` closes the search | |
| One column at a time, or two | `Ctrl+L` | |
| Show the keys | `?` | |
| Back to the panes; quit from there | `Esc`, `q`, `Ctrl+C` | |

Going to a pane needs Remote Control turned on in the Rex app.
When rawr runs in a tiled pane, a resume runs `claude --resume` in that same pane.
When rawr runs in a floating window, the session resumes in a new tab instead.

Search matches every typed word, ignoring case, against the prompts, title, folder and ID of each session.

## Commands

Rex keeps a pane's block ID across a server restart, so the block ID is the pane's identity.

| Command | Run by | Does |
| --- | --- | --- |
| `rawr` | You, from any shell | Opens the app |
| `rawr --startup` | Your shell, when it starts in a Rex pane | Opens the app on the pane's sessions when one can be resumed; otherwise prints nothing and exits 0 |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index, then deletes the index of each pane the Rex server does not list |
| `rawr daemon` | The first `rawr` window | Follows Rex and reads transcripts and the search catalog for every window |
| `rawr status [--format json]` | You, from any shell | Lists each Rex pane that has a session, with the title and folder of the newest session and what Claude is doing in that pane |
| `rawr setup claude [--dry-run]` | You, once | Adds the `SessionStart` hook that runs `rawr record` to Claude Code's user settings |
| `rawr completion bash\|zsh\|fish` | Your shell, at start | Prints the shell's completion script; Tab then completes from the installed binary |

Each `rawr` window is a client of one `rawr daemon`.
A window starts the daemon when nothing answers on `~/.local/state/rawr/daemon.sock`.
The daemon reads each transcript backwards from the newest line, so the latest entries of a long session appear at once.

## Shell integration

On a resume, `rawr` restores the terminal and becomes `claude --resume` in the session's folder; when Claude Code exits, the pane's shell prompt is back.
It needs no wrapper.
To open rawr once in each restored pane, before the first prompt, add the lines for your shell.
In zsh, in `~/.zshrc`:

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

In bash, in `~/.bashrc`:

```bash
if [[ -n $REX_BLOCK ]] && command -v rawr >/dev/null; then
  _rawr_once() {
    PROMPT_COMMAND=${PROMPT_COMMAND#_rawr_once;}
    unset -f _rawr_once
    rawr --startup
  }
  PROMPT_COMMAND="_rawr_once;${PROMPT_COMMAND}"
fi
```

In fish, in `~/.config/fish/config.fish`:

```fish
if set -q REX_BLOCK; and type -q rawr
    function _rawr_once --on-event fish_prompt
        functions -e _rawr_once
        rawr --startup
    end
end
```

`record` runs from a Claude Code `SessionStart` hook.
`rawr setup claude` adds the hook to `~/.claude/settings.json`, or to `$CLAUDE_CONFIG_DIR/settings.json`, and leaves every other setting in place and in order.
`--dry-run` reports what the command would change and changes nothing.
A second run reports the hook is there and changes nothing.
No other rawr command edits that file.
The hook `rawr setup claude` adds:

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

[releases]: https://github.com/victor-software-house/rawr/releases
[Rex]: https://www.superlogical.com/rex
