# Changelog

## rawr 0.0.2

- Build on ctl-core 0.7.4, which words the `--color` and `--no-color` help differently and exits with the command's own code when a reader closes the pipe early, as `head -1` does ([`d520896`](https://github.com/victor-software-house/rawr/commit/d520896b5972e8055d49993375684033b5e7db04)).
- `rawr completion bash|zsh|fish` prints a shell completion script, which completes rawr's verbs and flags from the installed binary ([`80a75b1`](https://github.com/victor-software-house/rawr/commit/80a75b137665bc68d69cfda02a7c6e24b7647dde)).

## rawr 0.0.1

- First release: `rawr record` remembers each Claude Code session under its Rex pane, and `rawr pick` offers it again in that pane after a Rex server restart, resuming the chosen session in its own folder ([`91ecfe3`](https://github.com/victor-software-house/rawr/commit/91ecfe35bc031a731f75118ea909a5baa474be21)).
- While the picker is open, its Rex pane shows a needs-attention dot on its tab, so a restored pane in a background tab is easy to find ([`fc2161c`](https://github.com/victor-software-house/rawr/commit/fc2161c363ee1a91cf0fe949dc5c165b87fc88bd)).

