# rawr

![rawr: Rex Agents, wake and resume](assets/rawr-banner-light.svg#gh-light-mode-only)
![rawr: Rex Agents, wake and resume](assets/rawr-banner-dark.svg#gh-dark-mode-only)

[![Release][release-badge]][releases] [![CI][ci-badge]][ci] [![License: MIT][license-badge]](LICENSE)

Bring Claude Code agents back into their panes after a [Rex] server restart.

[Get started](#get-started) · [The app](#the-app) · [Setup](#setup) · [Commands](#commands)

Rex restores your panes after a restart, but each one comes back as a fresh shell.
`rawr` remembers which Claude Code sessions ran in each pane.
Pick one to resume it, or quit for a plain shell.

rawr works with Claude Code for now; support for other coding agents is planned.

![rawr lists Claude sessions by status, opens one session's conversation and activity, then searches every session](assets/rawr-demo.svg)

The sessions in this demo are made up.

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

2. Let rawr record which pane each Claude Code session runs in, as [Claude Code hook](#claude-code-hook) explains:

   ```sh
   rawr setup claude
   ```

3. Add the lines from [Shell startup](#shell-startup) to your shell's startup file.
   After a Rex restart, rawr opens in each pane that had a Claude session.

Then type `rawr` in any pane.

## Open rawr

Press `⌥⌘R` (`cmd+alt+r`) to open rawr.
It shows the full app floating over the window in view, and hides it on the next press.
Only one copy runs at a time: opening it in another window closes the copy in the first.

<details>
<summary>The action to add to Rex's <code>init.lua</code></summary>

```lua
local terminal = "com.superlogical.terminal"
local rawr_bin = "rawr"

-- Every floating pane that runs rawr, in every session, by its IDs.
local function rawr_windows()
  local found = {}
  -- A view of another session needs the connection attached to it.
  rex.call("session.attach_all", { event_scope = "session" })
  local list = rex.call("session.list", {})
  for _, s in ipairs(list.sessions or {}) do
    -- A session whose view does not come back, such as one going away,
    -- holds nothing to show.
    local view = rex.call("session.view", { session_id = s.session_id }) or {}
    for _, w in ipairs(view.windows or {}) do
      for _, l in ipairs(w.layers or {}) do
        if l.kind == "floating" then
          for _, b in ipairs(l.blocks or {}) do
            local p = rex.block.call(terminal, "process", { session_id = s.session_id, block_id = b.block_id })
            if p ~= nil and p.foreground ~= nil and p.foreground.name == "rawr" then
              table.insert(found, {
                session_id = s.session_id,
                window_id = w.window_id,
                layer_id = l.layer_id,
                block_id = b.block_id,
                minimized = l.state == "minimized",
              })
            end
          end
        end
      end
    end
  end
  return found
end

rex.action{
  name = "rawr_open",
  title = "Show or Hide rawr",
  description = "Show the rawr app over this window, or hide it: every pane, its sessions, and a search over every session.",
  category = "rawr",
  keywords = "rawr panes sessions resume search toggle",
  run = function(ctx)
    if ctx.session_id == nil then error("no session is in view", 0) end
    local here = rex.call("session.view", { session_id = ctx.session_id }).active_window_id
    local mine = nil
    for _, w in ipairs(rawr_windows()) do
      if mine == nil and w.session_id == ctx.session_id and w.window_id == here then
        mine = w
      else
        rex.call("block.close", { session_id = w.session_id, block_id = w.block_id, keep_last = true })
      end
    end
    if mine ~= nil then
      rex.call("session.set_layer_state", {
        session_id = mine.session_id,
        layer_id = mine.layer_id,
        state = mine.minimized and "normal" or "minimized",
        focus = mine.minimized,
      })
      return
    end
    rex.call("session.new_layer", {
      session_id = ctx.session_id,
      window_id = here,
      bounds = { 0.1, 0.06, 0.8, 0.88 },
      focus = true,
      layout = { block = {
        flavor = "com.superlogical.terminal.shell",
        label = "rawr",
        options = { command = { rawr_bin } },
      } },
    })
  end,
}

rex.bind("cmd+alt+r", "rawr_open")
```

</details>

After a Rex restart, each pane where Claude ran shows only its own sessions, in a stand-in you can resume from or quit.

## The app

Like `less` or `vim`, rawr takes over the terminal and puts back what was on screen when you quit.

### Panes

rawr lists every pane with a Claude session, grouped by what Claude is doing there.
The list updates live.

| Mark | Status | Meaning |
| :-: | --- | --- |
| `?` `!` | **Needs you** | Claude waits for a permission, an answer, input or a login |
| `✘` | **Failed** | Claude stopped on an error |
| `✔` | **Done** | Claude finished its turn |
| `⠋` | **Working** | Claude, or one of its background tasks, is running |
| `○` | **Idle** | Claude is open and waiting for a prompt |
| `·` | **Stopped** | No Claude runs in the pane; its sessions can be resumed |
| `·` | **Closed** | The pane is gone; its sessions from the past day can still be resumed |

### Sessions

Select a pane to see the sessions that ran there.
Entries show a few lines and a `+N` count of the rest; click one to expand it, with Markdown rendered and code highlighted.

| Tab | Shows |
| --- | --- |
| Overview | Model, branch, prompts and replies, tokens out, sub-agents, times, tasks |
| Conversation | What you typed and what Claude answered, newest first |
| Activity | Every entry, tool calls and results included |
| Files | Files the session changed and read. Click a file to open it, or a folder to filter the list |

### Resume and go to a pane

Resuming a session restores the terminal and runs `claude --resume` in the session's folder.
When Claude exits, you are back at the shell prompt.
From a floating rawr window, the session resumes in a new tab instead.

Going to a pane switches the Rex app to it, which needs Remote Control turned on in the app.

### Keys

| Action | Keys | Mouse |
| --- | --- | --- |
| Switch columns | `1` `2`, `Tab` | Click a column |
| Switch tabs | `←` `→`, `h` `l`, `[` `]` | Click a tab |
| Move the selection or scroll | `↑` `↓`, `j` `k` | Click a row, or scroll the wheel |
| Go to the pane in the Rex app | `Enter` on a pane, `g` | Click the selected pane |
| Resume the session | `Enter` on a session, `r` | Click the selected session |
| Filter the focused column | `/`; `Esc` clears the filter | |
| Search all sessions by prompt, title, folder or ID | `Ctrl+F`; `Esc` closes the search | |
| One column or two | `Ctrl+L` | |
| Show the keys | `?` | |
| Back, or quit from the pane list | `Esc`, `q`, `Ctrl+C` | |

In the stand-in, `1`, `g` and `Ctrl+F` do nothing, and `Esc` quits.

## Setup

### Shell startup

After a Rex restart, these lines open rawr in every pane where Claude was running.

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

### Claude Code hook

`rawr setup claude` adds a `SessionStart` hook to `~/.claude/settings.json`, or to `$CLAUDE_CONFIG_DIR/settings.json`.
The hook runs `rawr record`, which notes the pane each session starts in.
Your other settings stay as they are, and `--dry-run` previews the change.

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

### Completion

```sh
source <(rawr completion zsh)    # ~/.zshrc, after compinit
source <(rawr completion bash)   # ~/.bashrc
rawr completion fish | source    # ~/.config/fish/config.fish
```

## Commands

| Command | Run by | Does |
| --- | :-: | --- |
| `rawr` | You | Opens the app |
| `rawr --startup` | Your shell, at start | Opens a stand-in with the pane's own sessions if one can be resumed; otherwise exits quietly |
| `rawr record` | A Claude Code `SessionStart` hook | Adds the session to the pane's index and removes the indexes of closed panes |
| `rawr daemon` | The first `rawr` window | Watches Rex and reads transcripts and search data for every window |
| `rawr status` | You | Lists the panes with sessions: newest title, folder, and what Claude is doing. `--format json` prints JSON |
| `rawr setup claude` | You, once | Adds the hook that runs `rawr record` to Claude Code's settings |
| `rawr completion <shell>` | Your shell, at start | Prints the completion script for bash, zsh or fish |

rawr identifies each pane by its Rex block ID, which survives a server restart.
Each `rawr` window talks to one `rawr daemon`, which the first window starts.
The daemon reads transcripts from the newest line back, so long sessions show their latest entries at once.

## License

MIT

[releases]: https://github.com/victor-software-house/rawr/releases
[Rex]: https://www.superlogical.com/rex
[release-badge]: https://img.shields.io/github/v/release/victor-software-house/rawr
[ci-badge]: https://github.com/victor-software-house/rawr/actions/workflows/ci.yml/badge.svg?branch=main
[ci]: https://github.com/victor-software-house/rawr/actions/workflows/ci.yml
[license-badge]: https://img.shields.io/github/license/victor-software-house/rawr
