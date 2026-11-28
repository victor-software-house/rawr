## ADDED Requirements

### Requirement: The launcher records the exact launch

`rawr <agent> [args]` SHALL write the pane's launch record with the agent, its folder and its arguments, SHALL export `RAWR_LAUNCH` with the record's ID, and SHALL replace itself with the agent.

#### Scenario: Claude Code with flags

- **WHEN** the user runs `rawr claude --model opus` in Rex block `block:01a1d2ce-9e06-715b-a2fb-e1fcb3c7f67e` in `/home/ada/code/shop`
- **THEN** that block's index file holds a launch record with agent `claude`, folder `/home/ada/code/shop` and arguments `--model opus`, and the process becomes `claude --model opus`

#### Scenario: The session joins the launch

- **WHEN** Claude Code's session hook runs `rawr record` with `RAWR_LAUNCH` set, for session `0f3c2a9e-1d4b-4c8e-9a71-5b6d2e8f4c10`
- **THEN** the launch record holds that session

### Requirement: A resume brings back the launch flags

A resume from the stand-in or the app SHALL run `rawr <agent>` with the adapter's resume arguments and the recorded flags, and SHALL drop a positional prompt and the flags that pick a session.

#### Scenario: Resume with a model flag

- **WHEN** the user picks session `0f3c2a9e-1d4b-4c8e-9a71-5b6d2e8f4c10`, launched with `--model opus`, in a stand-in
- **THEN** the stand-in's process becomes `rawr claude --resume 0f3c2a9e-1d4b-4c8e-9a71-5b6d2e8f4c10 --model opus` in the session's folder

#### Scenario: Launched with a prompt and --continue

- **WHEN** the session was launched as `rawr claude --continue --model opus "fix the tests"`
- **THEN** its resume runs `rawr claude --resume <id> --model opus`

### Requirement: rawr resume opens the stand-in

`rawr resume` SHALL open the stand-in, and `rawr --startup` SHALL do the same as a hidden alias.

#### Scenario: A saved shell line

- **WHEN** a startup file saved before this change runs `rawr --startup` in a pane with sessions
- **THEN** the stand-in opens as with `rawr resume`

### Requirement: Rex resumes no supported agent itself

`rawr.lua` SHALL set `resume.trust.<agent>` to `false` for each agent with an adapter when `rex.set` exists, and SHALL load without error when it does not.

#### Scenario: A current Rex

- **WHEN** `init.lua` holds `require("rawr")` on Rex 0.1.0-preview.25 and the user runs `rex config check`
- **THEN** the resume rules list `claude` under "not in force"

#### Scenario: An older Rex

- **WHEN** the same `init.lua` loads on Rex 0.1.0-preview.21
- **THEN** `rex config check` reports no error

### Requirement: The daemon keeps rawr.lua current

When the daemon starts and `rawr.lua` in Rex's configuration folder differs from the copy its binary carries, the daemon SHALL rewrite it and call `server.config_reload`.

#### Scenario: After an upgrade

- **WHEN** rawr 0.0.6 replaces 0.0.5 and its daemon starts
- **THEN** `rawr.lua` matches 0.0.6's copy and `server.status` shows a higher configuration generation
