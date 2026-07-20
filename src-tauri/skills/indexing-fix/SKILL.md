# Indexing Fix Skill

<!-- skill-version: 1 -->

Used by the `fix_indexing` / `fix_technical` agentic steps.

## Instructions

For `not_indexed_crawled` / `not_indexed_discovered` / `not_indexed_other`:
- Improve content depth and uniqueness (aim for 600+ words if currently thin)
- Add 3-5 relevant internal links to other pages on the site
- Ensure the H1 and title are specific and distinct from similar pages
- Add a clear meta description

For `robots_blocked` / `noindex` / `fetch_error` / `canonical_mismatch`:
- Fix the technical root cause in the MDX frontmatter or site config
- Explain what you changed and why

For `not_indexed_crawled` specifically (page is crawled but not indexed):
- This usually means Google sees the page but chooses not to index it.
- The page is already long and may have internal links — focus on DISTINCTIVENESS, not just length.
- Make the title, H1, and opening sections clearly different from cluster siblings listed above.
- Remove or merge sections that overlap with sibling articles.
- If the page cannot be made distinct enough, suggest a merge target instead.

## Output Contract

CRITICAL: You MUST actually write changes to the file. Do NOT just describe what you would do.
Do NOT create any markdown reports, summary files, or documentation.
Only edit the MDX file and return a brief text summary of what you changed.
