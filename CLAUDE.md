@AGENTS.md

## Claude Code

- AGENTS.md above is the source of truth for all agents; put shared guidance there, not here.
  Keep this file to Claude Code–specific notes.
- `.claude/rules/*.md` load automatically for the files in their `paths:`. They're shared
  guidance too (AGENTS.md sends other agents to them); keep AGENTS.md under 200 lines by
  putting topic detail there.
- The separate-session review in Workflow: spawn a fresh `general-purpose` `Agent` (not
  `Explore`, which locates code but doesn't review it, and no `model` override) with `effort`
  set to that level. Its prompt names what to review (for a release, the diff since the
  last release), gives the intent and the hardware results, and repeats Boundaries: don't
  change the system or install anything, Rust toolchains included (a reviewer's
  `cargo +1.88` once installed one). Never use a fork (`subagent_type: "fork"`) for this: a
  fork inherits this session's context and conclusions, and ignores `effort`. `/code-review`
  takes an effort level and a target but can't carry the intent or hardware results; it's
  for when the user runs it.
- The Boundaries section applies to Bash too: ask before any command that installs software,
  changes Noctalia config or state, or writes to the mouse. `.claude/settings.json` backs it
  up: ask rules for the commands Boundaries names, and a hook
  (`.claude/hooks/guard_mouse_writes.py`) that denies `settings set` and `import` without
  `--dry-run` or `--demo`. They catch slips, not everything (`sh -c`, a script, keys sent to
  the settings screen), so Boundaries still decides. A command added to Boundaries gets a
  rule there too.
