#!/bin/bash
# Capture FULL Kimi output to see what's in those 37009 bytes

PROJECT_PATH="/Users/fstrauf/01_code/call-analyzer"
AUTOMATION_DIR="$PROJECT_PATH/.github/automation"

REDDIT_CONFIG=$(cat "$AUTOMATION_DIR/reddit_config.md")
PROJECT_SUMMARY=$(cat "$AUTOMATION_DIR/project_summary.md")
BRANDVOICE=$(cat "$AUTOMATION_DIR/brandvoice.md")

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

echo "=== Running Kimi from pageseeds-app directory ==="
echo "Prompt length: ${#PROMPT} chars"
echo ""

cd /Users/fstrauf/01_code/pageseeds-app

OUTPUT=$(kimi -p "$PROMPT" --print --output-format text --final-message-only --no-thinking --work-dir "$PROJECT_PATH" 2>&1)

echo "=== Output size: ${#OUTPUT} bytes ==="
echo ""
echo "=== First 1000 chars ==="
echo "$OUTPUT" | head -c 1000
echo ""
echo ""
echo "=== Last 1000 chars ==="
echo "$OUTPUT" | tail -c 1000
echo ""

# Save full output to file for inspection
echo "$OUTPUT" > /tmp/kimi_full_output.txt
echo "=== Full output saved to /tmp/kimi_full_output.txt ==="
