#!/usr/bin/env bash
# Check that local Markdown references in docs/*.md and *.md point to existing files.
# Only checks relative links (./foo.md or docs/foo.md), not URLs.

set -euo pipefail

cd "$(dirname "$0")/.."

errors=0

# Find all markdown files and extract relative links
for file in $(find . -maxdepth 2 -name "*.md" -not -path "./node_modules/*" -not -path "./src-tauri/target/*" | sort); do
    # Extract markdown links: [text](./path) or [text](path)
    links=$(grep -oE '\[([^]]+)\]\(([^)]+)\)' "$file" | \
        grep -oE '\]\([^)]+\)' | \
        sed 's/]//;s/(//;s/)//' | \
        grep -v '^http' | \
        grep -v '^#' | \
        grep -v '^mailto:' || true)

    for link in $links; do
        # Resolve relative to the file's directory
        dir=$(dirname "$file")
        target="$dir/$link"

        # Handle links starting with ./
        if [[ "$link" == ./* ]]; then
            target="$dir/$link"
        # Handle links starting with ../
        elif [[ "$link" == ../* ]]; then
            target="$dir/$link"
        # Handle absolute-from-repo-root links (e.g. docs/FOO.md from root md files)
        elif [[ "$link" == docs/* ]] || [[ "$link" == */* ]]; then
            target="./$link"
        else
            target="$dir/$link"
        fi

        # Normalize path
        target=$(python3 -c "import os.path; print(os.path.normpath('$target'))" 2>/dev/null || echo "$target")

        if [ ! -f "$target" ]; then
            echo "BROKEN LINK: $file → $link (resolved: $target)"
            errors=$((errors + 1))
        fi
    done
done

if [ "$errors" -gt 0 ]; then
    echo ""
    echo "Found $errors broken local link(s)."
    exit 1
fi

echo "OK: All local Markdown links resolve."
