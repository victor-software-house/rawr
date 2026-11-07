# rex-resume

Bring Claude Code agents back into their panes after a [Rex] server restart.

Rex restores sessions, tabs and panes after a restart, but each pane comes back as a fresh shell.
`rex-resume` remembers which Claude Code sessions ran in each pane, and in a restored pane offers them again in a full-screen picker.
Choosing one resumes it in that pane; dismissing the picker leaves a plain shell.

## How it works

Rex keeps a pane's block ID across a server restart, so the block ID is the pane's identity.

| Command | Run by | Does |
| --- | --- | --- |
| `rex-resume record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index, then deletes the indexes of panes the Rex server no longer lists |
| `rex-resume pick` | The shell, when it starts in a Rex pane | Offers the pane's sessions that no running Claude Code holds, and prints the chosen `cd` and `claude --resume` line |

## License

MIT

[Rex]: https://www.superlogical.com/rex
