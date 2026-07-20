/// Prompt-building logic for Reddit reply drafting.
///
/// Business logic extracted from `commands/reddit.rs` so the command layer stays thin.
use crate::models::reddit::RedditOpportunity;

/// Build the full draft-reply prompt from project config and opportunity data.
///
/// # Arguments
/// * `project_context` — contents of `automation/project.md` (identity + brand voice + clusters)
/// * `guardrails`      — contents of `automation/reddit/_reply_guardrails.md`
/// * `skill_content`   — SKILL.md text for `reddit-reply-drafting` (empty string if not found)
/// * `product_name`    — product name to mention (e.g., "PageSeeds")
/// * `mention_stance`  — one of: REQUIRED, RECOMMENDED, OPTIONAL, OMIT
/// * `opp`             — the opportunity being drafted for
pub fn build_draft_reply_prompt(
    project_context: &str,
    guardrails: &str,
    skill_content: &str,
    product_name: &str,
    mention_stance: &str,
    opp: &RedditOpportunity,
) -> String {
    let stance_instruction = match mention_stance {
        "REQUIRED" => format!(
            "REQUIRED: The reply MUST contain the exact product name \"{}\" — no vague substitutes like 'a tool' or 'the app'.",
            product_name
        ),
        "RECOMMENDED" => format!(
            "RECOMMENDED: Mention \"{}\" by name if the topic is a natural fit.",
            product_name
        ),
        "OPTIONAL" => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally. Not required.",
            product_name
        ),
        "OMIT" => {
            "OMIT: Do NOT mention any product name in this reply.".to_string()
        }
        _ => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally. Not required.",
            product_name
        ),
    };

    let vague_phrases_block = if mention_stance == "REQUIRED" || mention_stance == "RECOMMENDED" {
        format!(
            "FORBIDDEN VAGUE PHRASES (replace all with \"{}\"): 'a dedicated tool', 'a platform', 'the app', 'a tracker', 'my tool', 'a tool I built'",
            product_name
        )
    } else {
        String::new()
    };

    let post_title = opp.title.as_deref().unwrap_or("(no title)");
    let post_body = opp
        .selftext
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();
    let post_subreddit = opp.subreddit.as_deref().unwrap_or("");
    let why_relevant = opp.why_relevant.as_deref().unwrap_or("");
    let pain_points = opp.key_pain_points.join(", ");
    let website_fit = opp.website_fit.as_deref().unwrap_or("");

    format!(
        r#"You are drafting a Reddit reply for the following post.

## PRODUCT CONTEXT
Product name: {product_name}
Mention stance: {mention_stance}
{stance_instruction}
{vague_phrases_block}

## PROJECT CONTEXT
{project_context}

## REPLY GUARDRAILS
{guardrails}

## POST DETAILS
Title: {post_title}
Subreddit: r/{post_subreddit}
Post body: {post_body}
Why relevant: {why_relevant}
Pain points: {pain_points}
Website fit: {website_fit}

## YOUR TASK
Write a Reddit reply following this formula:
  Acknowledge → Educate → Product mention (per stance above) → Engage

Rules:
- 3–5 sentences, plain text only, no URLs, no markdown links
- Conversational tone — write like you'd talk to a friend over coffee
- Do NOT use bullet points or headers
- Vary phrasing — don't sound like marketing copy

After drafting, run this critique pass:
  Act as a copy editor at a respected newspaper who believes in respecting your reader's time.
  Ask: Is every sentence earning its place? Can any words be cut? Is the tone conversational?
  Revise based on this critique before finalizing.

{skill_content}

Return ONLY the final reply text — no preamble, no explanation, no metadata.
"#
    )
}
