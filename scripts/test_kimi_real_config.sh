#!/bin/bash
# Test Kimi with the REAL config files from the call-analyzer project

set -e

PROJECT_PATH="/Users/fstrauf/01_code/call-analyzer"
AUTOMATION_DIR="$PROJECT_PATH/.github/automation"

if [ ! -f "$AUTOMATION_DIR/reddit_config.md" ]; then
    echo "ERROR: reddit_config.md not found at $AUTOMATION_DIR/reddit_config.md"
    exit 1
fi

echo "=== Reading config files ==="
REDDIT_CONFIG=$(cat "$AUTOMATION_DIR/reddit_config.md")
PROJECT_SUMMARY=$(cat "$AUTOMATION_DIR/project_summary.md" 2>/dev/null || echo "")
BRANDVOICE=$(cat "$AUTOMATION_DIR/brandvoice.md" 2>/dev/null || echo "")

echo "reddit_config.md: ${#REDDIT_CONFIG} chars"
echo "project_summary.md: ${#PROJECT_SUMMARY} chars"
echo "brandvoice.md: ${#BRANDVOICE} chars"
echo ""

# Build the EXACT same prompt as the Rust code
PROMPT=$(cat <<EOF
Extract Reddit search parameters from the config files below. Return ONLY a JSON object.

## reddit_config.md
\`\`\`markdown
$REDDIT_CONFIG
\`\`\`

## project_summary.md
\`\`\`markdown
$PROJECT_SUMMARY
\`\`\`

## brandvoice.md
\`\`\`markdown
$BRANDVOICE
\`\`\`

## Required JSON Output
Return a JSON object with these exact keys:
- product_name: string
- mention_stance: string (REQUIRED, RECOMMENDED, OPTIONAL, or OMIT)
- trigger_topics: array of strings
- query_keywords: array of strings (use same as trigger_topics)
- seed_subreddits: array of strings (WITHOUT r/ prefix)
- excluded_subreddits: array of strings

## Example
If the config has Product Name: Days to Expiry, then return:
{"product_name": "Days to Expiry", "mention_stance": "RECOMMENDED", "trigger_topics": ["topic1"], "query_keywords": ["topic1"], "seed_subreddits": ["subreddit1"], "excluded_subreddits": []}

Do NOT return placeholder text like "<actual product name>".
Return ONLY the JSON object, starting with { and ending with }.
EOF
)

echo "=== Running Kimi ==="
echo "Prompt length: ${#PROMPT} chars"
echo "Working dir: $PROJECT_PATH"
echo ""

# Run Kimi exactly as the Rust code does
OUTPUT=$(kimi -p "$PROMPT" --print --output-format text --final-message-only --no-thinking --work-dir "$PROJECT_PATH" 2>&1)

echo "=== Raw Output (${#OUTPUT} chars) ==="
echo "$OUTPUT"
echo ""

# Check if output starts with ```json or is raw JSON
if echo "$OUTPUT" | head -1 | grep -q '^```json'; then
    echo "Output starts with \`\`\`json block"
    JSON=$(echo "$OUTPUT" | sed -n '/^```json$/,/^```$/p' | sed '1d;$d')
elif echo "$OUTPUT" | head -1 | grep -q '^```'; then
    echo "Output starts with \`\`\` block (no language)"
    JSON=$(echo "$OUTPUT" | sed -n '/^```$/,/^```$/p' | sed '1d;$d')
else
    echo "Output appears to be raw JSON"
    JSON="$OUTPUT"
fi

echo ""
echo "=== Extracted JSON (${#JSON} chars) ==="
echo "$JSON"
echo ""

echo "=== JSON Validation ==="
echo "$JSON" | python3 -m json.tool > /dev/null 2>&1 && {
    echo "✅ Valid JSON"
    echo ""
    echo "=== Parsed Values ==="
    echo "$JSON" | python3 -c "import sys, json; d=json.load(sys.stdin); print(f\"product_name: {d.get('product_name')}\"); print(f\"mention_stance: {d.get('mention_stance')}\"); print(f\"trigger_topics: {len(d.get('trigger_topics', []))} items\"); print(f\"query_keywords: {len(d.get('query_keywords', []))} items\"); print(f\"seed_subreddits: {len(d.get('seed_subreddits', []))} items\"); print(f\"excluded_subreddits: {len(d.get('excluded_subreddits', []))} items\")"
} || {
    echo "❌ Invalid JSON"
    echo ""
    echo "=== First 500 chars of raw output (visible chars) ==="
    echo "$OUTPUT" | head -c 500 | cat -A
}
