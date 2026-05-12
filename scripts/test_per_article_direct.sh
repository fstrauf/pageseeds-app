#!/bin/bash
# Test per-article extraction with direct backend

BRIDGE_URL="http://localhost:8080"

build_prompt() {
    local id=$1
    local title=$2
    local excerpt=$3
    cat <<EOF
Analyze the following article and generate specific, actionable SEO recommendations.

Input context:
{
  "article_id": $id,
  "article_title": "$title",
  "article_file": "./posts/article-$id.mdx",
  "url_slug": "article-$id",
  "target_keyword": "sample keyword $id",
  "published_date": "2026-03-15",
  "gsc_snapshot": {"avg_position": 12.5, "impressions": 450, "ctr": 0.02},
  "failed_checks": [
    {"check_id": "title_keyword", "label": "Title missing exact keyword"},
    {"check_id": "meta_length", "label": "Meta description too long"},
    {"check_id": "intro_keyword", "label": "Intro missing target keyword"}
  ],
  "source_excerpt": "$excerpt"
}

Examine:
1. Title and H1 quality — keyword presence, clarity, length
2. Meta description — presence, length (50-155 chars), keyword inclusion
3. Introduction — engagement, keyword placement
4. Content structure — H2 headings, readability
5. Internal links — quantity, relevance
6. EEAT signals — credibility, authoritativeness
7. Call-to-action — clarity and placement
8. Year freshness — compare any year mentioned in the title or H1 against the published_date

For each suggestion, use one of these categories: title, meta_description, intro, h1, internal_links, faq, eeat, cta, date.

Requirements:
- 4-8 actionable suggestions for THIS article only.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement.
EOF
}

EXCERPT="You need content for your website. Not someday—now. Your product pages need rewriting, your blog hasn't been updated in months, and your competitors are publishing circles around you. But here's the problem: you don't know how to hire a content writer for your website small business needs. You've read the gig-economy horror stories. You've seen the portfolios that look like they were written by three different people. You've received quotes ranging from \$15 per article to \$5,000 per month. This guide cuts through the noise. We cover exactly how to hire a content writer who understands your business, speaks to your customers, and delivers work you don't have to rewrite. You'll learn where to find writers, what to look for in a portfolio, how to structure a trial assignment, and the red flags that separate professionals from amateurs. By the end, you'll have a repeatable hiring process that works whether you need one article or fifty."

# Make excerpt ~2600 chars
LONG_EXCERPT=""
for i in {1..3}; do
    LONG_EXCERPT="${LONG_EXCERPT}${EXCERPT} "
done

echo "Excerpt length: ${#LONG_EXCERPT} chars"
echo ""

TOTAL_START=$(date +%s)
SUCCESS=0
FAILED=0

for i in {1..5}; do
    ARTICLE_START=$(date +%s)
    PROMPT=$(build_prompt $i "Test Article $i" "$LONG_EXCERPT")
    PROMPT_LEN=${#PROMPT}
    
    REQUEST=$(cat <<EOF
{
  "model": "kimi-k2.5",
  "messages": [
    {"role": "user", "content": $(echo "$PROMPT" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')}
  ],
  "response_format": {"type": "json_object"}
}
EOF
)

    echo "=== Article $i ($PROMPT_LEN chars prompt) ==="
    
    time curl -s -X POST "$BRIDGE_URL/v1/chat/completions" \
      -H "Content-Type: application/json" \
      -H "X-Kimi-Backend: direct" \
      -d "$REQUEST" \
      -w "\nHTTP_CODE: %{http_code}\nTIME_TOTAL: %{time_total}s\n" \
      -o /tmp/test_article_$i.json \
      --max-time 130
    
    ARTICLE_END=$(date +%s)
    ARTICLE_DURATION=$((ARTICLE_END - ARTICLE_START))
    
    HTTP_CODE=$(grep "HTTP_CODE:" /tmp/test_article_$i.json | cut -d: -f2 | tr -d ' ')
    
    if [ "$HTTP_CODE" = "200" ]; then
        SUCCESS=$((SUCCESS + 1))
        CONTENT_LEN=$(python3 -c "
import json
with open('/tmp/test_article_$i.json') as f:
    data = json.load(f)
    content = data['choices'][0]['message']['content']
    print(len(content))
" 2>/dev/null || echo "0")
        echo "✓ Article $i: ${ARTICLE_DURATION}s, ${CONTENT_LEN} chars output"
    else
        FAILED=$((FAILED + 1))
        ERROR=$(python3 -c "
import json
with open('/tmp/test_article_$i.json') as f:
    data = json.load(f)
    print(data.get('error', {}).get('message', 'Unknown error'))
" 2>/dev/null || cat /tmp/test_article_$i.json | head -1)
        echo "✗ Article $i: ${ARTICLE_DURATION}s — $ERROR"
    fi
    echo ""
done

TOTAL_END=$(date +%s)
TOTAL_DURATION=$((TOTAL_END - TOTAL_START))

echo "========================================"
echo "RESULTS: $SUCCESS succeeded, $FAILED failed"
echo "Total time: ${TOTAL_DURATION}s"
echo "Per-article average: $((TOTAL_DURATION / 5))s"
