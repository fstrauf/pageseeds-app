#!/bin/bash
# Isolated test for Kimi JSON extraction
# This mimics what the Rust code does but lets us see exactly what Kimi returns

set -e

# Create a test config file similar to reddit_config.md
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

echo "=== Test Configuration Files ==="
echo "reddit_config.md:"
cat "$TEST_DIR/reddit_config.md"
echo ""
echo "=== Calling Kimi ==="
echo ""

# Build the prompt exactly as the Rust code does
REDDIT_CONFIG=$(cat "$TEST_DIR/reddit_config.md")
PROJECT_SUMMARY=$(cat "$TEST_DIR/project_summary.md")
BRANDVOICE=$(cat "$TEST_DIR/brandvoice.md")

# Use kimi with a JSON-only prompt
# The key is to tell it we ONLY want JSON output
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

echo "Prompt length: ${#PROMPT} characters"
echo ""
echo "=== Kimi Output ==="

# Call kimi and capture output
# Using --no-interactive to ensure it just returns the result
OUTPUT=$(echo "$PROMPT" | kimi --no-interactive 2>&1 || true)

echo "$OUTPUT"
echo ""
echo "=== Output Analysis ==="
echo "Output length: ${#OUTPUT} characters"

# Try to extract JSON using the same logic as Rust
echo ""
echo "Looking for JSON in code blocks..."
JSON_EXTRACT=$(echo "$OUTPUT" | sed -n '/^```json$/,/^```$/p' | sed '1d;$d' || true)
if [ -z "$JSON_EXTRACT" ]; then
    JSON_EXTRACT=$(echo "$OUTPUT" | sed -n '/^```$/,/^```$/p' | sed '1d;$d' || true)
fi

if [ -n "$JSON_EXTRACT" ]; then
    echo "Found JSON in code block:"
    echo "$JSON_EXTRACT"
    echo ""
    echo "Validating JSON..."
    echo "$JSON_EXTRACT" | python3 -m json.tool > /dev/null 2>&1 && echo "✅ Valid JSON" || echo "❌ Invalid JSON"
else
    echo "No JSON in code blocks, looking for raw JSON object..."
    JSON_EXTRACT=$(echo "$OUTPUT" | grep -o '{.*}' || true)
    if [ -n "$JSON_EXTRACT" ]; then
        echo "Found potential JSON:"
        echo "$JSON_EXTRACT"
        echo ""
        echo "Validating JSON..."
        echo "$JSON_EXTRACT" | python3 -m json.tool > /dev/null 2>&1 && echo "✅ Valid JSON" || echo "❌ Invalid JSON"
    else
        echo "No JSON object found"
    fi
fi

echo ""
echo "=== First 200 chars of raw output ==="
echo "$OUTPUT" | head -c 200
echo ""
