---
rawr: patch
---

Build on ctl-core 0.7.4, which words the `--color` and `--no-color` help differently and exits with the command's own code when a reader closes the pipe early, as `head -1` does.
