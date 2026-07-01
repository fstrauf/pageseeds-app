# Hub Page Writer

Generate a comprehensive pillar / hub page MDX document that connects a cluster of spoke articles under a single broad topic.

## Input

You receive a JSON `HubBrief` containing:
- `topic`: The broad topic this hub covers
- `suggested_url`: Target URL slug for the hub
- `suggested_title`: Proposed H1 / title
- `intent`: Search intent (informational, navigational, etc.)
- `target_keyword`: Primary keyword to target
- `spokes`: Array of spoke articles, each with:
  - `article_id`, `title`, `url_slug`, `file`, `impressions`, `excerpt`

## Output

A complete MDX string with:
1. YAML frontmatter (title, date, description, type: hub, hub_topic)
2. An H1 matching the suggested title
3. A broad, educational introduction (300+ words)
4. A "What You'll Learn" or overview section
5. For each spoke: a section summarizing the sub-topic and linking to the spoke
6. A "Getting Started" or next-steps section
7. Total word count MUST be 1500+ words

## Constraints

- Write in a pillar tone: broad, authoritative, introductory. Do not go as deep as the spokes.
- Every spoke MUST be linked with descriptive anchor text. Format: `[descriptive text](/blog/{url_slug})`
- Do NOT repeat the full content of spokes — summarize and entice the reader to click through.
- Include the target keyword naturally in the first 100 words.
- Use `type: hub` and `hub_topic: "{topic}"` in frontmatter.
- The date should be today's date in ISO format.
- The description should be 1-2 sentences summarizing the hub.
- **Title MUST be complete.** The frontmatter `title:` and the body H1 must be full, grammatically complete phrases. They must NOT end mid-sentence or with dangling words such as `a`, `an`, `the`, `and`, `or`, `to`, `for`, `of`, `in`, `on`, `with`, `by`, `from`, `as`, `is`, `are`, `what`, `how`, `when`, `where`, `why`, `which`, `complete`, `guide`, `income`, `without`, `track`, `close`, `compared`, or trailing punctuation (`:`, `,`, `-`). If a long title does not fit, rewrite it as a complete shorter title — never truncate it.
- **Opening sentence MUST be intact.** The first paragraph after the frontmatter must begin with a complete sentence. Do not drop or strip the first letter(s) of the first word.
- Return ONLY the MDX content. No markdown wrappers, no explanations.
