#!/usr/bin/env python3
"""Verify that a function extracted by an LLM matches the original."""
import sys
import re


def extract_function(path, func_name):
    with open(path) as f:
        content = f.read()

    # Find fn NAME( ... }  (naive but effective for our codebase)
    # Handles multi-line functions with proper Rust indentation
    pattern = rf'(?:pub\s+)?(?:crate\s+)?\s*fn\s+{re.escape(func_name)}\s*\([^)]*\)(?:\s*->\s*[^{{]+)?\s*\{{'
    match = re.search(pattern, content)
    if not match:
        # Fallback: simpler pattern
        pattern = rf'fn\s+{re.escape(func_name)}\s*\('
        match = re.search(pattern, content)
        if not match:
            raise ValueError(f"Could not find {func_name} in {path}")

    start = match.start()
    brace_count = 0
    in_string = False
    string_char = None
    i = start

    in_comment = False

    while i < len(content):
        ch = content[i]
        if in_comment:
            if ch == '\n':
                in_comment = False
            i += 1
            continue
        if not in_string:
            if ch == '/' and i + 1 < len(content) and content[i + 1] == '/':
                in_comment = True
                i += 2
                continue
            if ch in ('"', "'"):
                in_string = True
                string_char = ch
            elif ch == '{':
                brace_count += 1
            elif ch == '}':
                brace_count -= 1
                if brace_count == 0:
                    return content[start:i+1]
        else:
            if ch == string_char and content[i-1] != '\\':
                in_string = False
        i += 1

    raise ValueError(f"Could not find matching braces for {func_name}")


def normalize(text):
    """Remove comments and normalize whitespace."""
    # Remove // comments
    lines = []
    for line in text.split('\n'):
        # Keep the line if it's not a pure comment
        stripped = line.strip()
        if stripped.startswith('//'):
            continue
        # Remove inline // comments
        if '//' in line:
            line = line[:line.index('//')]
        lines.append(line.rstrip())

    # Remove blank lines and normalize indentation
    lines = [line for line in lines if line.strip()]
    return '\n'.join(lines)


def main():
    if len(sys.argv) != 4:
        print(f"Usage: {sys.argv[0]} <old_file> <new_file> <function_name>")
        sys.exit(1)

    old_path, new_path, func_name = sys.argv[1:4]

    try:
        old = extract_function(old_path, func_name)
        new = extract_function(new_path, func_name)
    except ValueError as e:
        print(f"❌ {func_name}: {e}")
        sys.exit(1)

    old_norm = normalize(old)
    new_norm = normalize(new)

    if old_norm == new_norm:
        print(f"✅ {func_name}: identical after normalization")
        sys.exit(0)
    else:
        print(f"❌ {func_name}: bodies differ!")
        import difflib
        diff = difflib.unified_diff(
            old_norm.split('\n'),
            new_norm.split('\n'),
            fromfile=f"{old_path}:{func_name}",
            tofile=f"{new_path}:{func_name}",
            lineterm=''
        )
        print('\n'.join(diff))
        sys.exit(1)


if __name__ == '__main__':
    main()
