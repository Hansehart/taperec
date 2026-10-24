"""Empties the "features" of a devcontainer.json (JSON with comments), so the tutorial's cage builds fast.

Usage: python3 no-features.py path/to/devcontainer.json
"""

import sys


def features_block(text: str) -> tuple[int, int]:
    """The start of the `{` after "features" and the index just past its matching `}`."""
    key = text.index('"features"')
    start = text.index("{", key)
    depth, i = 0, start
    while i < len(text):
        c = text[i]
        if c == '"':
            # Skip the string, honouring escapes.
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif text.startswith("//", i):
            i = text.index("\n", i)
        elif text.startswith("/*", i):
            i = text.index("*/", i) + 1
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return start, i + 1
        i += 1
    raise ValueError("unbalanced features block")


path = sys.argv[1]
text = open(path, encoding="utf-8").read()
start, end = features_block(text)
open(path, "w", encoding="utf-8").write(text[:start] + "{}" + text[end:])
