# Proposal

## Why

After a Rex restart, `rawr --startup` opened the full app in every pane where Claude had run.
Each copy reported a count over all panes to Rex, so a change in one pane moved every copy to the top of Rex's Activity list at once, and the stopped agents the user looked for sank below them (operator, 2026-11-23).
Each copy was also titled rawr, though only the origin should say rawr, as Claude Code panes say claude.

## What Changes

1. `rawr --startup` opens a stand-in: only the sessions recorded for its own pane, with no panes column and no search.
2. Bare `rawr` stays the full app, which Rex opens floating over the window in view on `cmd+alt+r` through an `init.lua` action the README gives.
3. Each window reports only its own state through OSC 7501.
   A stand-in reports once, under its session's title; the full app reports only when it flips between idle and blocked.
4. A stand-in sets its pane's terminal title to its session's title, and empties it when it gives the terminal back.

## Capabilities

### New Capabilities

- `stand-in`: what `rawr --startup` shows, and what every rawr window tells Rex about itself.

### Modified Capabilities

None; the `app` capability is still added by the open `rawr-app` change, and its startup requirement stays true.

## Impact

1. Modules: `src/main.rs` (the mode and the stand-in's title), `src/app.rs` (reports), `src/app/{state,view,report,terminal}.rs`.
2. README: a section on `cmd+alt+r` with the `init.lua` action.
3. The maintainer's dotfiles keep their own `init.lua` action.
