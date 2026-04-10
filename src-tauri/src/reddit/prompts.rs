/// Prompt-building logic for Reddit reply drafting.
///
/// Business logic extracted from `commands/reddit.rs` so the command layer stays thin.

use crate::reddit::config::{MentionStance, RedditProjectConfig};
use crate::models::reddit::RedditOpportunity;

/// Build the full draft-reply prompt from project config and opportunity data.
///
/// # Arguments
/// * `project_context` — contents of `automation/project.md` (identity + brand voice + clusters)
/// * `guardrails`      — contents of `automation/reddit/_reply_guardrails.md`
/// * `skill_content`   — SKILL.md text for `reddit-reply-drafting` (empty string if not found)
/// * `cfg`             — parsed reddit_config
/// * `opp`             — the opportunity being drafted for
pub fn build_draft_reply_prompt(
    project_context: &str,
    guardrails: &str,
    skill_content: &str,
    cfg: &RedditProjectConfig,
    opp: &RedditOpportunity,
) -> String {
    let product_name = cfg.product_name.as_deref().unwrap_or("the product");
    let mention_stance = cfg.mention_stance.as_str();

    let stance_instruction = match cfg.mention_stance {
        MentionStance::Required => format!(
            "REQUIRED: The reply MUST contain the exact product name \"{}\" — no vague substitutes like 'a tool' or 'the app'.",
            product_name
        ),
        MentionStance::Recommended => format!(
            "RECOMMENDED: Mention \"{}\" by name if the topic is a natural fit.",
            product_name
        ),
        MentionStance::Optional => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally. Not required.",
            product_name
        ),
        MentionStance::Omit => {
            "OMIT: Do NOT mention any product name in this reply.".to_string()
        }
    };

    let vague_phrases_block = if cfg.mention_stance == MentionStance::Required
        || cfg.mention_stance == MentionStance::Recommended
    {
        format!(
            "FORBIDDEN VAGUE PHRASES (replace all with \"{}\"): 'a dedicated tool', 'a platform', 'the app', 'a tracker', 'my tool', 'a tool I built'",
            product_name
        )
    } else {
        String::new()
    };

    let post_title = opp.title.as_deref().unwrap_or("(no title)");
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
