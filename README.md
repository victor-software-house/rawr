# rawr

![rawr: Rex Agents, wake and resume](assets/rawr-banner-light.svg#gh-light-mode-only)
![rawr: Rex Agents, wake and resume](assets/rawr-banner-dark.svg#gh-dark-mode-only)

[![Release][release-badge]][releases] [![CI][ci-badge]][ci] [![License: MIT][license-badge]](LICENSE)

Bring Claude Code agents back into their panes after a [Rex] server restart.

[Get started](#get-started) · [The app](#the-app) · [Commands](#commands) · [Shell integration](#shell-integration)

Rex restores your panes after a restart, but each one comes back as a fresh shell.
`rawr` remembers which Claude Code sessions ran in each pane.
Pick one to resume it, or quit for a plain shell.

## Get started

1. Install rawr from the [releases], for macOS on Apple silicon or Linux on x64:

   ```sh
   mise use -g github:victor-software-house/rawr
   ```

   <details>
   <summary>Build with Cargo instead</summary>

   ```sh
   cargo install --git https://github.com/victor-software-house/rawr --locked
   ```

   </details>

2. Let rawr record which pane each Claude Code session runs in:

   ```sh
   rawr setup claude
   ```

3. Add the startup lines for your shell from [Shell integration](#shell-integration).
   After a Rex restart, rawr opens in each pane that had a Claude session.

Then type `rawr` in any pane.

## The app

`rawr` opens the app full screen.
When you quit, the pane's output is still there.

The app lists every pane with a Claude session and shows what Claude is doing there, live.
Panes that need you come first, then Failed, Done, Working, Idle and Stopped.
Last come sessions from the past day whose pane is gone.

Select a pane to see the sessions that ran there.
Each session has four tabs:

| Tab | Shows |
| --- | --- |
| Overview | Model, branch, prompt and reply counts, tokens out, sub-agents, start and last activity, task list |
| Conversation | What you typed and what Claude answered, newest first |
| Activity | Every entry, tool calls and results included |
| Files | Files the session changed and read. Click a file to open it, or a folder to filter the list |

Each entry shows a few lines and a `+N` count of the rest.
Click an entry to expand it.
Expanded entries render Markdown and highlight code.

| Action | Keys | Mouse |
| --- | --- | --- |
| Switch columns | `1` `2`, `Tab` | Click a column |
| Switch tabs | `←` `→`, `h` `l`, `[` `]` | Click a tab |
| Move the selection or scroll | `↑` `↓`, `j` `k` | Click a row, or scroll the wheel |
| Go to the pane in the Rex app | `Enter` on a pane, `g` | Click the selected pane |
| Resume the session | `Enter` on a session, `r` | Click the selected session |
| Filter the focused column | `/`; `Esc` clears the filter | |
| Search every local session | `Ctrl+F`; `Esc` closes the search | |
| One column or two | `Ctrl+L` | |
| Show the keys | `?` | |
| Back, or quit from the pane list | `Esc`, `q`, `Ctrl+C` | |

Going to a pane needs Remote Control turned on in the Rex app.
In a tiled pane, a resume runs `claude --resume` right there.
From a floating window, the session resumes in a new tab.

Search matches all the words you type, in any case.
It looks at each session's prompts, title, folder and ID.

## Commands

rawr identifies each pane by the Rex block ID, which survives a server restart.

| Command | Run by | Does |
| --- | --- | --- |
| `rawr` | You | Opens the app |
| `rawr --startup` | Your shell, at start | Opens the app if the pane has a session to resume; otherwise exits quietly |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index and removes the indexes of closed panes |
| `rawr daemon` | The first `rawr` window | Watches Rex and reads transcripts and search data for every window |
| `rawr status` | You | Lists the panes with sessions: newest title, folder, and what Claude is doing. `--format json` prints JSON |
| `rawr setup claude` | You, once | Adds the hook that runs `rawr record` to Claude Code's settings |
| `rawr completion <shell>` | Your shell, at start | Prints the completion script for bash, zsh or fish |

Each `rawr` window talks to one `rawr daemon`.
The first window starts the daemon if none is running.
The daemon reads transcripts from the newest line back, so long sessions show their latest entries at once.

## Shell integration

On a resume, rawr restores the terminal and runs `claude --resume` in the session's folder.
When Claude exits, you are back at the shell prompt.
No wrapper is needed.

To open rawr in each restored pane, add these lines to your shell's startup file.
For zsh, in `~/.zshrc`:

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

<details>
<summary>bash, in ~/.bashrc</summary>

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

</details>

<details>
<summary>fish, in ~/.config/fish/config.fish</summary>

```fish
if set -q REX_BLOCK; and type -q rawr
    function _rawr_once --on-event fish_prompt
        functions -e _rawr_once
        rawr --startup
    end
end
```

</details>

`rawr setup claude` adds the `SessionStart` hook to `~/.claude/settings.json`, or to `$CLAUDE_CONFIG_DIR/settings.json`.
Your other settings stay as they are.
Add `--dry-run` to preview the change first.

<details>
<summary>The hook added by <code>rawr setup claude</code></summary>

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

</details>

To load completion, add the line for your shell:

```sh
source <(rawr completion zsh)    # ~/.zshrc, after compinit
source <(rawr completion bash)   # ~/.bashrc
rawr completion fish | source    # ~/.config/fish/config.fish
```

## License

MIT

[releases]: https://github.com/victor-software-house/rawr/releases
[Rex]: https://www.superlogical.com/rex
[release-badge]: https://img.shields.io/github/v/release/victor-software-house/rawr
[ci-badge]: https://github.com/victor-software-house/rawr/actions/workflows/ci.yml/badge.svg?branch=main
[ci]: https://github.com/victor-software-house/rawr/actions/workflows/ci.yml
[license-badge]: https://img.shields.io/github/license/victor-software-house/rawr
