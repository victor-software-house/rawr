#!/usr/bin/env bash
#MISE description="Fail each OpenSpec change file over its word budget"
# The budgets of each change under openspec/changes/, in words:
#
#   proposal.md          600
#   design.md            2500
#   tasks.md             1500, and 60 for each evidence note
#   specs/**/spec.md     1200
#   research.md          2500
#
# Archived changes and the specs under openspec/specs/ are not checked. The
# budgets bound a change's scope, so a change over one is split into smaller
# changes, not cut down to fit.
#
# A word is a whitespace-separated token, as wc -w counts it, so list markers,
# table pipes, code blocks and HTML comments count. A reference-style link
# definition does not: `[label]: <url-or-path>`, with an optional title in
# double quotes or parentheses and no prose after it. A footnote definition,
# `[^1]: text`, and a line such as `[Note]: some prose` count.
#
# In tasks.md the budget covers the task text: headings, the task lines and
# their Verify lines. An evidence note is a nested list item that starts with
# a date, such as `  - 2026-11-09 (UTC): …`, or with a linked date, such as
# `  - [2026-11-09][notes]: …`, with the lines indented under it. It counts
# toward its own budget instead.
set -euo pipefail
[ -d openspec/changes ] || exit 0
find openspec/changes -path openspec/changes/archive -prune -o -type f -name '*.md' -print |
  LC_ALL=C sort |
  awk -v proposal=600 -v design=2500 -v tasks=1500 \
    -v spec=1200 -v research=2500 -v note=60 '
    # BusyBox awk keeps a -v value a string, and would compare 949 > 1200 as
    # text. Adding 0 makes each budget a number.
    BEGIN { proposal += 0; design += 0; tasks += 0; spec += 0; research += 0; note += 0 }
    # A reference-style link definition: a label that is not a footnote, one
    # target token, and at most a title in double quotes or parentheses.
    BEGIN {
      definition = "^[ \t]*\\[[^]^][^]]*\\]:[ \t]*[^ \t]+"
      definition = definition "([ \t]+(\"[^\"]*\"|\\([^)]*\\)))?[ \t]*$"
    }
    function budget(path) {
      if (path ~ /^openspec\/changes\/[^\/]+\/proposal\.md$/) return proposal
      if (path ~ /^openspec\/changes\/[^\/]+\/design\.md$/) return design
      if (path ~ /^openspec\/changes\/[^\/]+\/tasks\.md$/) return tasks
      if (path ~ /^openspec\/changes\/[^\/]+\/research\.md$/) return research
      if (path ~ /^openspec\/changes\/[^\/]+\/specs\/.+\/spec\.md$/) return spec
      return 0
    }
    function end_note() {
      if (in_note && note_words > note) {
        printf "%s:%d: evidence note has %d words, over its budget of %d\n",
          path, note_line, note_words, note
        failed = 1
      }
      in_note = 0
    }
    {
      path = $0
      limit = budget(path)
      if (limit == 0) next
      is_tasks = path ~ /\/tasks\.md$/
      words = 0
      in_note = 0
      line = 0
      while ((getline text < path) > 0) {
        line++
        # Prose after the target keeps counting, and so does a title in
        # single quotes.
        if (text ~ definition) continue
        n = split(text, tokens)
        if (!is_tasks) { words += n; continue }
        indent = match(text, /[^ \t]/) - 1
        if (in_note && n > 0 && indent <= note_indent) end_note()
        if (!in_note && text ~ /^[ \t]+[-*+][ \t]+\[?[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]/) {
          in_note = 1
          note_indent = indent
          note_line = line
          note_words = 0
        }
        if (in_note) note_words += n
        else words += n
      }
      close(path)
      if (is_tasks) end_note()
      if (words > limit) {
        printf "%s: %d words, over its budget of %d\n", path, words, limit
        failed = 1
      }
    }
    END {
      if (failed) print "✘ split each change over a budget into smaller changes,",
        "each with its own proposal; do not cut text to fit"
      exit failed
    }
  '
