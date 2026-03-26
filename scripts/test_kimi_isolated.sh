#!/bin/bash
# Isolated test for Kimi JSON extraction with realistic config

set -e

TEST_DIR=$(mktemp -d)
trap "rm -rf $TEST_DIR" EXIT

cat > "$TEST_DIR/reddit_config.md" << 'EOF'
# Reddit Opportunity Search Configuration

## Product Name
Days to Expiry

## Mention Stance
RECOMMENDED

## Trigger Topics
- "expiration date tracking" (for users complaining about forgetting expiry dates)
- "food waste" (for users discussing throwing away expired food)
- "productivity app" (for users looking for apps to manage deadlines)

## Query Keywords
- "expiration date tracking"
- "food waste"
- "productivity app"

## Seed Subreddits
- r/productivity — People looking for productivity tools
- r/food — General food discussions
- r/EatCheapAndHealthy — Budget-conscious food planning

## Excluded Subreddits
- r/wallstreetbets
EOF

cat > "$TEST_DIR/project_summary.md" << 'EOF'
# Project Summary

Days to Expiry is a macOS menu bar app that helps users track expiration dates for food, medications, and other time-sensitive items.
EOF

cat > "$TEST_DIR/brandvoice.md" << 'EOF'
# Brand Voice

Friendly, helpful, and practical. Focus on reducing waste and saving money.
EOF

# Build the prompt exactly as the Rust code does
REDDIT_CONFIG=$(cat "$TEST_DIR/reddit_config.md")
PROJECT_SUMMARY=$(cat "$TEST_DIR/project_summary.md")
BRANDVOICE=$(cat "$TEST_DIR/brandvoice.md")

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
echo ""

# Run Kimi exactly as the Rust code does
OUTPUT=$(kimi -p "$PROMPT" --print --output-format text --final-message-only --no-thinking 2>&1)

echo "=== Raw Output (${#OUTPUT} chars) ==="
echo "$OUTPUT"
echo ""
echo "=== Output in hex (first 200 chars) ==="
echo "$OUTPUT" | head -c 200 | xxd
echo ""

# Extract JSON using the same logic as Rust
echo "=== JSON Extraction Test ==="

# Look for ```json ... ``` pattern
if echo "$OUTPUT" | grep -q '^```json'; then
    echo "Found \`\`\`json block"
    JSON=$(echo "$OUTPUT" | sed -n '/^```json$/,/^```$/p' | sed '1d;$d')
elif echo "$OUTPUT" | grep -q '^```'; then
    echo "Found \`\`\` block (no language)"
    JSON=$(echo "$OUTPUT" | sed -n '/^```$/,/^```$/p' | sed '1d;$d')
else
    echo "Looking for raw { ... }"
    JSON=$(echo "$OUTPUT" | grep -o '{.*}')
fi

echo ""
echo "=== Extracted JSON ==="
echo "$JSON"
echo ""

echo "=== JSON Validation ==="
echo "$JSON" | python3 -m json.tool > /dev/null 2>&1 && echo "✅ Valid JSON" || {
    echo "❌ Invalid JSON"
    echo ""
    echo "=== First 100 chars of extracted 'JSON' ==="
    echo "$JSON" | head -c 100 | cat -A
}
