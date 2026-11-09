#!/usr/bin/env python3
"""Claude Code PreToolUse hook: keep an agent from changing the mouse.

AGENTS.md (Boundaries) lets an agent run `rogctl settings set` and `import`
only with `--dry-run`; `--demo` reaches no mouse either. This denies a Bash
call that runs one without them, however rogctl is started (`rogctl`,
`cargo run --`, `target/debug/rogctl`).

It reads the command's words, so `sh -c '...'` or a script hides a command
from it: it catches slips and is no sandbox. A failure here (no python3, a
crash) lets the call through, as Claude Code does for any hook error.
"""

import json
import re
import shlex
import sys

SEPARATORS = ";&|()\n"
SETTINGS = ("settings", "tui")  # `tui` is the command's old name
WRITES = ("set", "import")
HARMLESS = ("--dry-run", "--demo")
DENIAL = {
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": (
            "`rogctl settings set` and `import` change the mouse, which "
            "AGENTS.md (Boundaries) keeps for the user. Add --dry-run to see "
            "what would change, or --demo to run without a mouse. If the "
            "user asked for this exact change, give them the command to run "
            "themselves. If the command only mentions these words, quote "
            "them or write the text with a file tool."
        ),
    }
}


def simple_commands(command: str) -> list[list[str]]:
    """Split a shell command line into the words of each command in it."""
    # A continued line is one command, and the `&` of `2>&1` separates nothing.
    command = re.sub(r"\d*[<>]&\d*-?", " ", command.replace("\\\n", " "))
    lexer = shlex.shlex(command, posix=True, punctuation_chars=SEPARATORS)
    lexer.whitespace = " \t\r"
    lexer.whitespace_split = True
    lexer.commenters = ""
    try:
        words = list(lexer)
    except ValueError:  # an unclosed quote: split it roughly instead
        return [part.split() for part in re.split(r"[;&|()\n]+", command)]
    commands: list[list[str]] = [[]]
    for word in words:
        if word and not word.strip(SEPARATORS):
            commands.append([])
        else:
            commands[-1].append(word)
    return commands


def changes_the_mouse(words: list[str]) -> bool:
    """Whether these words run `settings set` or `import` for real."""
    if any(word in HARMLESS for word in words):
        return False
    seen_settings = False
    for word in words:
        if seen_settings and word in WRITES:
            return True
        seen_settings = seen_settings or word in SETTINGS
    return False


def main() -> int:
    """Read the hook's JSON from stdin and deny a call that would write."""
    try:
        command = json.load(sys.stdin)["tool_input"]["command"]
    except (ValueError, KeyError, TypeError):
        return 0
    if not isinstance(command, str):
        return 0
    if any(changes_the_mouse(words) for words in simple_commands(command)):
        json.dump(DENIAL, sys.stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main())
