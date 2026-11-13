# Spec Delta

## Purpose

Record each Claude Code session under its Rex pane through one mod that uses only the documented mods API.

## ADDED Requirements

### Requirement: Record on session start

The rawr mod SHALL, on `session.start` in a process whose environment holds `REX_BLOCK`, run the rawr binary at the path stored in the plugin's data folder with the session's ID, folder and transcript path.

#### Scenario: A session starts outside Rex

- **WHEN** a session starts and `REX_BLOCK` is unset
- **THEN** the mod runs nothing
