#!/bin/bash
# Test content review prompt size and direct backend timing

BRIDGE_URL="http://localhost:8080"
PROMPT_FILE="/tmp/test_content_review_prompt.json"

# Build a realistic prompt (~20K chars, 5 articles, 2600-char excerpts)
cat > "$PROMPT_FILE" <<'PROMPT'
Analyze the following articles and generate specific, actionable SEO recommendations.

Input context:
{
  "articles": [
    {
      "article_id": 1,
      "article_title": "How to Hire a Content Writer for Your Website: Small Business Hiring Guide",
      "article_file": "./posts/hire-content-writer-small-business.mdx",
      "url_slug": "hire-content-writer-small-business",
      "target_keyword": "hire content writer for website small business",
      "published_date": "2026-03-15",
      "gsc_snapshot": {"avg_position": 12.5, "impressions": 450, "ctr": 0.02},
      "failed_checks": [
        {"check_id": "title_keyword", "label": "Title missing exact keyword"},
        {"check_id": "meta_length", "label": "Meta description too long"},
        {"check_id": "intro_keyword", "label": "Intro missing target keyword"}
      ],
      "source_excerpt": "You need content for your website. Not someday—now. Your product pages need rewriting, your blog hasn't been updated in months, and your competitors are publishing circles around you. But here's the problem: you don't know how to hire a content writer for your website small business needs. You've read the gig-economy horror stories. You've seen the portfolios that look like they were written by three different people. You've received quotes ranging from $15 per article to $5,000 per month. This guide cuts through the noise. We cover exactly how to hire a content writer who understands your business, speaks to your customers, and delivers work you don't have to rewrite. You'll learn where to find writers, what to look for in a portfolio, how to structure a trial assignment, and the red flags that separate professionals from amateurs. By the end, you'll have a repeatable hiring process that works whether you need one article or fifty."
    },
    {
      "article_id": 2,
      "article_title": "Website Content Writing Service Pricing: What Small Businesses Actually Pay in 2026",
      "article_file": "./posts/website-content-writing-service-pricing.mdx",
      "url_slug": "website-content-writing-service-pricing",
      "target_keyword": "website content writing service pricing guide",
      "published_date": "2026-02-26",
      "gsc_snapshot": {"avg_position": 8.3, "impressions": 1200, "ctr": 0.025},
      "failed_checks": [
        {"check_id": "title_keyword", "label": "Title missing exact keyword"},
        {"check_id": "meta_length", "label": "Meta description too long"},
        {"check_id": "h1_keyword", "label": "H1 missing target keyword"}
      ],
      "source_excerpt": "You've searched \"content writing service pricing\" and found everything from $15 per article to $15,000 per month. Both prices exist. Both have customers. The question is what that range actually means for a small business that just wants good content without getting taken advantage of. This guide breaks down what small businesses actually pay for website content writing services in 2026. We look at real pricing tiers, what's included at each level, and how to evaluate whether a quote is fair. We also cover the hidden costs that don't show up in the initial quote: revision rounds, scope creep, and the time you'll spend managing writers who don't understand your business. Whether you're budgeting for a single landing page or a year of blog posts, this guide gives you the numbers you need to make an informed decision."
    },
    {
      "article_id": 3,
      "article_title": "B2B Content Writing Service for Startups: Build Pipeline With Content That Educates Before It Sells",
      "article_file": "./posts/b2b-content-writing-service-startups.mdx",
      "url_slug": "b2b-content-writing-service-startups",
      "target_keyword": "B2B content writing service for startups",
      "published_date": "2026-01-10",
      "gsc_snapshot": {"avg_position": 15.2, "impressions": 320, "ctr": 0.018},
      "failed_checks": [
        {"check_id": "meta_length", "label": "Meta description too long"},
        {"check_id": "intro_keyword", "label": "Intro missing target keyword"},
        {"check_id": "keyword_density", "label": "Keyword density below 0.2%"}
      ],
      "source_excerpt": "You close the quarter with a handful of deals and a sales team that swears the pipeline is \"looking healthy.\" Three months later, it isn't. The leads dried up. The inbound volume dropped. And your competitors—the ones who've been publishing consistently—are now ranking for every keyword your prospects search for. This is the B2B content trap: you know content matters, but you don't have the time or expertise to produce it at scale. A B2B content writing service for startups can fix this by creating educational content that builds trust before your sales team ever makes contact. This article covers how to choose a service that understands startup constraints, what types of content actually move the pipeline, and how to measure ROI without a complex attribution model."
    },
    {
      "article_id": 4,
      "article_title": "Product Description Writing Service for Ecommerce: SEO-Optimized Copy That Actually Sells",
      "article_file": "./posts/product-description-writing-service-ecommerce.mdx",
      "url_slug": "product-description-writing-service-ecommerce",
      "target_keyword": "product description writing service for ecommerce",
      "published_date": "2026-04-05",
      "gsc_snapshot": {"avg_position": 9.1, "impressions": 890, "ctr": 0.022},
      "failed_checks": [
        {"check_id": "h1_keyword", "label": "H1 missing target keyword"},
        {"check_id": "meta_length", "label": "Meta description too long"},
        {"check_id": "intro_keyword", "label": "Intro missing target keyword"}
      ],
      "source_excerpt": "Your products are great. Your store is live. But if your product pages are using manufacturer copy—or barely a sentence of original text—you're leaving a significant amount of organic traffic on the table, and probably losing sales to competitors who put the same products into better words. A product description writing service for ecommerce can transform thin, duplicate copy into SEO-optimized product pages that rank and convert. This guide explains what makes ecommerce product copy effective, how to brief a writer for product descriptions, what SEO elements matter most for product pages, and how to evaluate whether a writing service understands ecommerce conversion principles. You'll also see before-and-after examples of product descriptions that moved the needle on both rankings and revenue."
    },
    {
      "article_id": 5,
      "article_title": "A Complete Guide to SEO for Small Businesses",
      "article_file": "./posts/seo-guide-small-businesses.mdx",
      "url_slug": "seo-guide-small-businesses",
      "target_keyword": null,
      "published_date": "2026-05-07",
      "gsc_snapshot": {"avg_position": 22.0, "impressions": 150, "ctr": 0.01},
      "failed_checks": [
        {"check_id": "source_file_found", "label": "Source file not found"},
        {"check_id": "meta_desc_present", "label": "Meta description missing"},
        {"check_id": "h2_structure", "label": "Fewer than 2 H2 headings"},
        {"check_id": "internal_links", "label": "Fewer than 3 internal links"},
        {"check_id": "word_count", "label": "Word count below 800"}
      ],
      "source_excerpt": ""
    }
  ]
}

For each article, examine:
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
- 4-8 actionable suggestions per article.
- Use only the provided context.
- Be specific: include the exact current text and proposed replacement.
PROMPT

PROMPT_CHARS=$(wc -c < "$PROMPT_FILE")
echo "Prompt size: $PROMPT_CHARS chars"

# Build JSON request
REQUEST=$(cat <<EOF
{
  "model": "kimi-k2.5",
  "messages": [
    {"role": "user", "content": $(cat "$PROMPT_FILE" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')}
  ],
  "response_format": {"type": "json_object"}
}
EOF
)

echo "Sending to bridge (backend=direct)..."
echo "Timeout: 120s"

time curl -s -X POST "$BRIDGE_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -H "X-Kimi-Backend: direct" \
  -d "$REQUEST" \
  -w "\nHTTP_CODE: %{http_code}\nTIME_TOTAL: %{time_total}s\nSIZE_DOWNLOAD: %{size_download}\n" \
  -o /tmp/test_content_review_response.json \
  --max-time 130

echo ""
echo "=== Response summary ==="
python3 -c "
import json
with open('/tmp/test_content_review_response.json') as f:
    data = json.load(f)
    if 'choices' in data:
        content = data['choices'][0]['message']['content']
        print(f'Content length: {len(content)} chars')
        try:
            parsed = json.loads(content)
            if 'recommendations' in parsed:
                print(f'Recommendations: {len(parsed[\"recommendations\"])} articles')
                for rec in parsed['recommendations']:
                    suggs = rec.get('suggestions', [])
                    print(f'  - {rec.get(\"article_title\", \"?\")[:50]}... ({len(suggs)} suggestions)')
            elif 'articles' in parsed:
                print(f'Articles: {len(parsed[\"articles\"])}')
        except:
            print('Response is not JSON')
    elif 'error' in data:
        print(f'Error: {data[\"error\"]}')
    else:
        print('Unexpected response format')
"
