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

   Add CSV export to the reports page
   ~/code/shop · 1 day ago

 ↑↓ select  ⏎ resume  / search  1-9 resume nth  click select, again resume  esc shell
```

The picker opens on the alternate screen, so the pane's restored output is back unchanged when it closes.

| Action | Keys | Mouse |
| --- | --- | --- |
| Select | `↑` `↓`, `k` `j`, `Home` `End`, `g` `G`, `PgUp` `PgDn` | Click another session, or scroll the wheel |
| Resume | `Enter`, or `1` to `9` for that session | Click the selected session |
| Search every local session | `/`, then type; `Backspace`, `Ctrl+U` to clear, `Esc` to close | |
| Leave a plain shell | `Esc`, `q`, `Ctrl+C` | |

Search matches every word typed, ignoring case, against the prompts typed in a session, its title, its folder and its ID.
It reads Claude Code's prompt history, `~/.claude/history.jsonl`, and lists `~/.claude/projects/` once to keep only sessions whose transcript is still there.
Each row's title is the one Claude Code shows, read from the end of the session's transcript; a session with none takes its first prompt that is not a slash command such as `/config`.

Hovering a session brightens it and shows the hand pointer in terminals that support OSC 22.
Each session shows its title, then its folder and how long ago it was active; long ones wrap, and nothing is cut.

## How it works

Rex keeps a pane's block ID across a server restart, so the block ID is the pane's identity.

| Command | Run by | Does |
| --- | --- | --- |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index, then deletes the indexes of panes the Rex server no longer lists |
| `rawr pick` | The shell, when it starts in a Rex pane | Offers the pane's sessions that no running Claude Code holds, and resumes the chosen one in its folder |
| `rawr find [WORDS…]` | You, from any shell | Opens the picker on the search line, and resumes the chosen session as `pick` does |
| `rawr setup claude [--dry-run]` | You, once | Adds the `SessionStart` hook that runs `rawr record` to Claude Code's user settings |

## Shell integration

On a choice, `pick` and `find` restore the terminal and become `claude --resume` in the session's folder; when Claude Code exits, the pane's shell prompt is back.
They need no wrapper, so `rawr find` works as typed in any shell.
In zsh, offer a restored pane its sessions once, before the first prompt:

```zsh
if [[ -n $REX_BLOCK ]] && (( $+commands[rawr] )); then
  autoload -Uz add-zsh-hook
  _rawr_once() {
    add-zsh-hook -d precmd _rawr_once
    rawr pick
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

## License

MIT

[Rex]: https://www.superlogical.com/rex
