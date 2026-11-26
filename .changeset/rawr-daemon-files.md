---
rawr: patch
verctl:group: added
---

`rawr daemon` reads the Rex server, the transcripts and the search data once for every rawr window, and the first window starts it.
The `files` key in `config.yaml` sets how a click in the Files tab opens a file: in its default app, in a text editor, or in its folder.
By default a folder or a file with an execute bit opens in its folder, so a click never runs a file a session wrote.
