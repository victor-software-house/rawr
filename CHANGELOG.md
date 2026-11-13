# Changelog

## rawr 0.0.3

- `~/.config/rawr/config.yaml` sets how many sessions each pane remembers, `history.per_pane`, 5 by default, and a bad value names the file and line.
  `rawr pick` no longer offers a session whose transcript Claude Code has deleted, and the pane's index forgets it ([`56fa85d`](https://github.com/victor-software-house/rawr/commit/56fa85d7d887667cf9db461575de00c0d4b52620)).
- The picker puts the session list on top and a search box at the bottom, drawn like Claude Code's prompt box.
  Printable keys type into the box, so `/`, `q`, `j`, `k`, `g`, `G` and `1` to `9` no longer act on the list.
  `Esc` clears the text, then dismisses; `Ctrl+C` dismisses.
  The list shows `picker.rows` sessions, 3 by default, then `+N earlier · ctrl+o`, and `Ctrl+O` shows and hides the rest.
  After a dismissal, `rawr pick` and `rawr find` print one line on stderr that names the way back ([`de3ac40`](https://github.com/victor-software-house/rawr/commit/de3ac4068a68d9b95640bd6b33fdb8c48962a9f8)).
- `rawr status` lists each Rex pane that has a Claude session with its title, its folder, and what its Claude is doing: Working, Needs input, Idle, Failed, or Stopped.
  rawr reads the Rex server through its control protocol instead of running the `rex` command ([`a3bb28e`](https://github.com/victor-software-house/rawr/commit/a3bb28e4293f9c8a5d4e8df47c5cbc78444fb660)).

## rawr 0.0.2

- Build on ctl-core 0.7.4, which words the `--color` and `--no-color` help differently and exits with the command's own code when a reader closes the pipe early, as `head -1` does ([`d520896`](https://github.com/victor-software-house/rawr/commit/d520896b5972e8055d49993375684033b5e7db04)).
- `rawr completion bash|zsh|fish` prints a shell completion script, which completes rawr's verbs and flags from the installed binary ([`80a75b1`](https://github.com/victor-software-house/rawr/commit/80a75b137665bc68d69cfda02a7c6e24b7647dde)).

## rawr 0.0.1

- First release: `rawr record` remembers each Claude Code session under its Rex pane, and `rawr pick` offers it again in that pane after a Rex server restart, resuming the chosen session in its own folder ([`91ecfe3`](https://github.com/victor-software-house/rawr/commit/91ecfe35bc031a731f75118ea909a5baa474be21)).
- While the picker is open, its Rex pane shows a needs-attention dot on its tab, so a restored pane in a background tab is easy to find ([`fc2161c`](https://github.com/victor-software-house/rawr/commit/fc2161c363ee1a91cf0fe949dc5c165b87fc88bd)).

