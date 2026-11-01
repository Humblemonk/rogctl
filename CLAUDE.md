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
  last release) and gives the intent and the hardware results. Never use a fork
  (`subagent_type: "fork"`) for this: a fork inherits this session's context and
  conclusions, and ignores `effort`. `/code-review` takes only a target, so it can't carry
  the intent or hardware results; it's for when the user runs it.
- The Boundaries section applies to Bash too: ask before any command that installs software,
  changes Noctalia config or state, or writes to the mouse.
