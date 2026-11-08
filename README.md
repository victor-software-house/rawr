# rawr

Rex Agents: Wake and Resume.
Bring Claude Code agents back into their panes after a [Rex] server restart.

Rex restores sessions, tabs and panes after a restart, but each pane comes back as a fresh shell.
`rawr` remembers which Claude Code sessions ran in each pane, and in a restored pane offers them again in a full-screen picker.
Choosing one resumes it in that pane; dismissing the picker leaves a plain shell.

```text
 ↻ Resume a Claude Code session  2 sessions

 ❯ Fix the login redirect loop after the OAuth callback
   ~/code/shop · 2 h ago
   6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33

   Add CSV export to the reports page
   ~/code/shop · 1 day ago
   b8e4d7c2-1a9f-4e63-8d07-2c5f9a3e6b14

 ↑↓ select  ⏎ resume  / search  1-9 resume nth  click select, again resume  esc shell
```

The picker opens on the alternate screen, so the pane's restored output is back unchanged when it closes.

| Action | Keys | Mouse |
| --- | --- | --- |
| Select | `↑` `↓`, `k` `j`, `Home` `End`, `g` `G`, `PgUp` `PgDn` | Click another session, or scroll the wheel |
| Resume | `Enter`, or `1` to `9` for that session | Click the selected session |
| Search every local session | `/`, then type; `Backspace`, `Ctrl+U` to clear, `Esc` to close | |
| Leave a plain shell | `Esc`, `q`, `Ctrl+C` | |

Search matches every word typed, ignoring case, against the prompts typed in a session, its folder and its ID.
It reads Claude Code's prompt history, `~/.claude/history.jsonl`, and lists `~/.claude/projects/` once to keep only sessions whose transcript is still there; it opens no transcript.
Search rows take their title from the session's first prompt that is not a slash command such as `/config`.

Hovering a session brightens it and shows the hand pointer in terminals that support OSC 22.
Long titles, folders and session IDs wrap; nothing is cut.

## How it works

Rex keeps a pane's block ID across a server restart, so the block ID is the pane's identity.

| Command | Run by | Does |
| --- | --- | --- |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index, then deletes the indexes of panes the Rex server no longer lists |
| `rawr pick` | The shell, when it starts in a Rex pane | Offers the pane's sessions that no running Claude Code holds, and prints the chosen `cd` and `claude --resume` line |
| `rawr find [WORDS…]` | You, from any shell | Opens the picker on the search line, and prints the chosen line as `pick` does |

## Shell integration

`pick` draws on the terminal through stderr and writes only the chosen command to stdout, so the shell can run it.
In zsh, run it once before the first prompt:

```zsh
if [[ -n $REX_BLOCK ]] && (( $+commands[rawr] )); then
  autoload -Uz add-zsh-hook
  _rawr_once() {
    add-zsh-hook -d precmd _rawr_once
    local cmd
    cmd=$(rawr pick) || return 0
    print -s -- "$cmd"
    eval "$cmd"
  }
  add-zsh-hook precmd _rawr_once
fi

# Run what `rawr find` chooses; pass every other command through.
rawr() {
  if [[ $1 == find ]]; then
    local cmd
    cmd=$(command rawr "$@") || return 0
    print -s -- "$cmd"
    eval "$cmd"
  else
    command rawr "$@"
  fi
}
```

`record` runs from a Claude Code `SessionStart` hook in `~/.claude/settings.json`:

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

## License

MIT

[Rex]: https://www.superlogical.com/rex
