@AGENTS.md

## Claude Code

- AGENTS.md above is the source of truth for all agents; put shared guidance there, not here.
  Keep this file to Claude Code–specific notes.
- `.claude/rules/*.md` load automatically for the files in their `paths:`. They're shared
  guidance too (AGENTS.md sends other agents to them); keep AGENTS.md under 200 lines by
  putting topic detail there.
- The separate-session review in Workflow: run `/code-review <level>` from a new session, or
  spawn a fresh `Agent` with `effort` set to that level. Not a fork: it inherits this
  session's context and conclusions, and ignores `effort`.
- The Boundaries section applies to Bash too: ask before any command that installs software,
  changes Noctalia config or state, or writes to the mouse.
