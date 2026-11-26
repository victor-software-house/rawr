---
rawr: patch!
---

`rawr` with no arguments opens one full-screen app, which replaces `rawr pick` and `rawr find`.
The app lists every Rex pane that has a Claude session, grouped by what Claude is doing there, and updates the list live.
For the selected pane it shows each session's overview, conversation, activity and the files it changed and read.
`Ctrl+F` searches every session by prompt, title, folder or ID.
`Enter` resumes a session in its folder, or switches the Rex app to the session's pane.
The shell startup line runs `rawr --startup`, which opens a stand-in with the pane's own sessions, and the `picker.rows` setting is gone.
