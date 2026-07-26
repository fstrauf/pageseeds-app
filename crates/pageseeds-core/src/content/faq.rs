//! Deterministic FAQ source assessment for MDX articles.
//!
//! Product SoT for FAQ schema is frontmatter `faq:` (Q/A list). The theme owns
//! FAQPage JSON-LD emission from that list. Markdown `## FAQ` alone is visible
//! prose, not a machine-readable schema source.
//!
//! Prefer [`assess_faq_source`] when you need the full verdict (machine_readable
//! + reasons). Hot paths that only need frontmatter or inline JSON-LD should use
//! the focused helpers ([`frontmatter_faq_count`], [`has_frontmatter_faq`],
//! [`has_inline_json_ld_faq`]) so they do not re-run the full assessment.

use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

/// Result of inspecting an MDX document for FAQ sources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaqSourceAssessment {
    /// True when a machine-readable FAQ source exists: valid frontmatter Q/A
    /// and/or a parseable inline FAQPage JSON-LD with at least one entity.
    pub machine_readable: bool,
    /// Count of valid non-empty frontmatter `{question, answer}` pairs.
    pub frontmatter_count: usize,
    /// Whether a visible FAQ heading exists in the body.
    pub has_visible_section: bool,
    /// Whether a parseable inline FAQPage JSON-LD with ≥1 mainEntity was found.
    pub has_inline_faqpage: bool,
    /// Soft/diagnostic reasons (only when applicable).
    pub reasons: Vec<String>,
}

/// Assess FAQ sources in an MDX document (frontmatter + body). Pure, no I/O.
///
/// Use this for the complete machine-readable verdict and diagnostic reasons.
/// Prefer [`frontmatter_faq_count`] / [`has_inline_json_ld_faq`] on hot paths
/// that only need one source.
pub fn assess_faq_source(mdx: &str) -> FaqSourceAssessment {
    let mut reasons = Vec::new();

    let (frontmatter_count, fm_state) = assess_frontmatter_faq(mdx);
    match fm_state {
        FrontmatterFaqState::Missing => {
            // Always keep this diagnostic when frontmatter `faq:` is absent —
            // including when machine-readable via JSON-LD alone (preferred SoT
            // is still frontmatter). Callers treat reasons as soft signals.
            reasons.push("missing_frontmatter_faq".to_string());
        }
        FrontmatterFaqState::EmptyOrInvalid => {
            reasons.push("empty_qa".to_string());
        }
        FrontmatterFaqState::Valid => {
            if !(3..=5).contains(&frontmatter_count) {
                reasons.push("count_out_of_band".to_string());
            }
        }
    }

    let has_visible_section = has_visible_faq_section(mdx);

    let (has_inline_faqpage, invalid_json_ld) = assess_inline_json_ld(mdx);
    if invalid_json_ld {
        reasons.push("invalid_inline_json_ld".to_string());
    }

    let machine_readable = frontmatter_count > 0 || has_inline_faqpage;

    if has_visible_section && !machine_readable {
        reasons.push("visible_without_machine_readable".to_string());
    }

    FaqSourceAssessment {
        machine_readable,
        frontmatter_count,
        has_visible_section,
        has_inline_faqpage,
        reasons,
    }
}

/// Count valid non-empty frontmatter `{question, answer}` pairs only.
///
/// Frontmatter parse only — does not scan the body or JSON-LD. Prefer this
/// (or [`has_frontmatter_faq`]) over [`assess_faq_source`] on CTR hot paths.
pub fn frontmatter_faq_count(mdx: &str) -> usize {
    assess_frontmatter_faq(mdx).0
}

/// True when frontmatter `faq:` has at least one valid non-empty Q/A pair.
pub fn has_frontmatter_faq(mdx: &str) -> bool {
    frontmatter_faq_count(mdx) > 0
}

/// True when the body has parseable inline FAQPage JSON-LD with ≥1 mainEntity.
///
/// Inline script extraction + JSON parse only — does not parse frontmatter or
/// scan for visible FAQ headings.
pub fn has_inline_json_ld_faq(content: &str) -> bool {
    assess_inline_json_ld(content).0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrontmatterFaqState {
    /// No `faq` key in parseable frontmatter (or no frontmatter / parse fail).
    Missing,
    /// `faq` present but zero valid non-empty Q/A pairs.
    EmptyOrInvalid,
    /// At least one valid Q/A pair.
    Valid,
}

fn assess_frontmatter_faq(mdx: &str) -> (usize, FrontmatterFaqState) {
    let Some((fm_raw, _)) = crate::content::frontmatter::split_mdx(mdx) else {
        return (0, FrontmatterFaqState::Missing);
    };
    let Ok(fm) = crate::content::frontmatter::parse(fm_raw) else {
        return (0, FrontmatterFaqState::Missing);
    };
    let Some(faq) = fm.parsed.get("faq") else {
        return (0, FrontmatterFaqState::Missing);
    };
    let Some(seq) = faq.as_sequence() else {
        // Present but not a list — treat as empty/invalid.
        return (0, FrontmatterFaqState::EmptyOrInvalid);
    };
    if seq.is_empty() {
        return (0, FrontmatterFaqState::EmptyOrInvalid);
    }

    let count = seq.iter().filter(|item| is_valid_qa_map(item)).count();
    if count == 0 {
        (0, FrontmatterFaqState::EmptyOrInvalid)
    } else {
        (count, FrontmatterFaqState::Valid)
    }
}

fn is_valid_qa_map(item: &YamlValue) -> bool {
    let Some(map) = item.as_mapping() else {
        return false;
    };
    let question = yaml_nonempty_string(map.get(YamlValue::from("question")));
    let answer = yaml_nonempty_string(map.get(YamlValue::from("answer")));
    question && answer
}

fn yaml_nonempty_string(v: Option<&YamlValue>) -> bool {
    match v {
        Some(YamlValue::String(s)) => !s.trim().is_empty(),
        // YAML may parse unquoted scalars; accept non-null non-empty string forms.
        Some(YamlValue::Number(n)) => !n.to_string().is_empty(),
        Some(YamlValue::Bool(_)) => true,
        Some(YamlValue::Null) | None => false,
        Some(other) => other.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false),
    }
}

/// Visible FAQ heading heuristics (markdown body). Single source of truth.
pub fn has_visible_faq_section(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim().to_lowercase();
        trimmed.starts_with("# faq")
            || trimmed.starts_with("## faq")
            || trimmed.starts_with("### faq")
            || trimmed.starts_with("# frequently asked questions")
            || trimmed.starts_with("## frequently asked questions")
            || trimmed.starts_with("### frequently asked questions")
    })
}

/// Returns `(has_valid_faqpage_with_entities, had_invalid_json_ld)`.
///
/// `had_invalid_json_ld` is true when at least one `application/ld+json` block
/// fails to parse as JSON (diagnostic; does not imply FAQ intent by itself).
fn assess_inline_json_ld(content: &str) -> (bool, bool) {
    let blocks = extract_json_ld_blocks(content);
    if blocks.is_empty() {
        return (false, false);
    }

    let mut has_faqpage = false;
    let mut invalid = false;

    for block in &blocks {
        match serde_json::from_str::<JsonValue>(block) {
            Ok(json) => {
                if let Some(entities) = faqpage_entity_count(&json) {
                    if entities >= 1 {
                        has_faqpage = true;
                    }
                }
            }
            Err(_) => {
                invalid = true;
            }
        }
    }

    (has_faqpage, invalid)
}

/// Extract raw JSON text from `<script type="…application/ld+json…">…</script>` blocks.
fn extract_json_ld_blocks(content: &str) -> Vec<String> {
    let lower = content.to_lowercase();
    let mut blocks = Vec::new();
    let mut search_from = 0usize;

    while let Some(type_rel) = lower[search_from..].find("application/ld+json") {
        let type_abs = search_from + type_rel;
        // Ensure this sits inside a script tag-ish context (look backward for "<script").
        let prefix = &lower[..type_abs];
        let script_open = prefix.rfind("<script");
        let Some(script_at) = script_open else {
            search_from = type_abs + 1;
            continue;
        };
        // Reject if another `>` between `<script` and type would close a prior tag wrongly —
        // simple: require no `</script` between script_at and type_abs.
        if lower[script_at..type_abs].contains("</script") {
            search_from = type_abs + 1;
            continue;
        }

        let after_type = &content[type_abs..];
        let Some(gt_rel) = after_type.find('>') else {
            break;
        };
        let json_start = type_abs + gt_rel + 1;
        let close_rel = lower[json_start..].find("</script>");
        let Some(close_rel) = close_rel else {
            break;
        };
        let json = content[json_start..json_start + close_rel].trim();
        if !json.is_empty() {
            blocks.push(json.to_string());
        }
        search_from = json_start + close_rel + "</script>".len();
    }

    blocks
}

/// If `json` contains a FAQPage (root, @graph, or array root), return mainEntity length.
fn faqpage_entity_count(json: &JsonValue) -> Option<usize> {
    if is_faq_page_type(json) {
        return Some(main_entity_len(json));
    }
    if let Some(graph) = json.get("@graph").and_then(|g| g.as_array()) {
        for item in graph {
            if is_faq_page_type(item) {
                return Some(main_entity_len(item));
            }
        }
    }
    if let Some(arr) = json.as_array() {
        for item in arr {
            if is_faq_page_type(item) {
                return Some(main_entity_len(item));
            }
            if let Some(n) = faqpage_entity_count(item) {
                return Some(n);
            }
        }
    }
    None
}

fn is_faq_page_type(v: &JsonValue) -> bool {
    match v.get("@type") {
        Some(JsonValue::String(s)) => s.eq_ignore_ascii_case("FAQPage"),
        Some(JsonValue::Array(arr)) => arr.iter().any(|t| {
            t.as_str()
                .map(|s| s.eq_ignore_ascii_case("FAQPage"))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn main_entity_len(v: &JsonValue) -> usize {
    v.get("mainEntity")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_faq_not_machine_readable() {
        let mdx = r#"---
title: "Plain"
description: "No FAQ here at all for testing purposes only really"
date: "2024-01-01"
---

# Plain

Just body text.
"#;
        let a = assess_faq_source(mdx);
        assert!(!a.machine_readable);
        assert_eq!(a.frontmatter_count, 0);
        assert!(!a.has_visible_section);
        assert!(!a.has_inline_faqpage);
        assert!(a.reasons.iter().any(|r| r == "missing_frontmatter_faq"));
        assert!(!a.reasons.iter().any(|r| r == "visible_without_machine_readable"));
    }

    #[test]
    fn visible_only_not_machine_readable() {
        let mdx = r#"---
title: "Markdown FAQ"
description: "An article with markdown FAQ only"
date: "2024-01-01"
---

# Markdown FAQ

## FAQ

### What is this?
Answer prose only.
"#;
        let a = assess_faq_source(mdx);
        assert!(!a.machine_readable);
        assert!(a.has_visible_section);
        assert_eq!(a.frontmatter_count, 0);
        assert!(a.reasons.iter().any(|r| r == "missing_frontmatter_faq"));
        assert!(a
            .reasons
            .iter()
            .any(|r| r == "visible_without_machine_readable"));
    }

    #[test]
    fn valid_frontmatter_3_to_5_machine_readable() {
        let mdx = r#"---
title: "With FAQ"
description: "Article with frontmatter FAQ"
date: "2024-01-01"
faq:
  - question: "Q1?"
    answer: "A1"
  - question: "Q2?"
    answer: "A2"
  - question: "Q3?"
    answer: "A3"
---

# With FAQ

## FAQ

### Q1?
A1
"#;
        let a = assess_faq_source(mdx);
        assert!(a.machine_readable);
        assert_eq!(a.frontmatter_count, 3);
        assert!(a.has_visible_section);
        assert!(!a.reasons.iter().any(|r| r == "count_out_of_band"));
        assert!(!a.reasons.iter().any(|r| r == "missing_frontmatter_faq"));
    }

    #[test]
    fn empty_qa_not_counted() {
        let mdx = r#"---
title: "Bad FAQ"
description: "Empty entries"
date: "2024-01-01"
faq:
  - question: ""
    answer: "A1"
  - question: "Q2?"
    answer: ""
  - question: "  "
    answer: "  "
---

# Bad FAQ
"#;
        let a = assess_faq_source(mdx);
        assert!(!a.machine_readable);
        assert_eq!(a.frontmatter_count, 0);
        assert!(a.reasons.iter().any(|r| r == "empty_qa"));
        assert!(!a.reasons.iter().any(|r| r == "missing_frontmatter_faq"));
    }

    #[test]
    fn count_out_of_band_still_machine_readable() {
        let mdx = r#"---
title: "Two FAQ"
description: "Only two pairs"
date: "2024-01-01"
faq:
  - question: "Q1?"
    answer: "A1"
  - question: "Q2?"
    answer: "A2"
---

# Two FAQ
"#;
        let a = assess_faq_source(mdx);
        assert!(a.machine_readable);
        assert_eq!(a.frontmatter_count, 2);
        assert!(a.reasons.iter().any(|r| r == "count_out_of_band"));
    }

    #[test]
    fn parseable_inline_faqpage_machine_readable() {
        let mdx = r#"---
title: "JSON-LD FAQ"
description: "Inline schema"
date: "2024-01-01"
---

# JSON-LD FAQ

<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "What is this?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "This is a test."
      }
    }
  ]
}
</script>
"#;
        let a = assess_faq_source(mdx);
        assert!(a.machine_readable);
        assert!(a.has_inline_faqpage);
        assert_eq!(a.frontmatter_count, 0);
        assert!(a.reasons.iter().any(|r| r == "missing_frontmatter_faq"));
    }

    #[test]
    fn invalid_inline_json_ld_reason() {
        let mdx = r#"---
title: "Broken JSON-LD"
description: "Invalid JSON"
date: "2024-01-01"
---

# Broken

<script type="application/ld+json">
{ "@type": "FAQPage", mainEntity: [ not valid json ]
</script>
"#;
        let a = assess_faq_source(mdx);
        assert!(!a.machine_readable);
        assert!(!a.has_inline_faqpage);
        assert!(a.reasons.iter().any(|r| r == "invalid_inline_json_ld"));
    }

    #[test]
    fn crude_faqpage_string_without_parseable_json_not_machine_readable() {
        // String contains "FAQPage" but not in a parseable ld+json script.
        let mdx = r#"---
title: "Fake"
description: "Mentions FAQPage in prose"
date: "2024-01-01"
---

# Fake

We should emit FAQPage schema someday.
"#;
        let a = assess_faq_source(mdx);
        assert!(!a.machine_readable);
        assert!(!a.has_inline_faqpage);
    }

    #[test]
    fn partial_valid_pairs_count_only_good_ones() {
        let mdx = r#"---
title: "Mixed"
description: "Some bad entries"
date: "2024-01-01"
faq:
  - question: "Good?"
    answer: "Yes"
  - question: ""
    answer: "No question"
  - question: "Also good?"
    answer: "Yep"
  - question: "Third?"
    answer: "Three"
---

# Mixed
"#;
        let a = assess_faq_source(mdx);
        assert!(a.machine_readable);
        assert_eq!(a.frontmatter_count, 3);
        assert!(!a.reasons.iter().any(|r| r == "empty_qa"));
        assert!(!a.reasons.iter().any(|r| r == "count_out_of_band"));
    }

    #[test]
    fn focused_helpers_match_full_assessment_fields() {
        let with_fm = r#"---
title: "With FAQ"
description: "Article with frontmatter FAQ only for focused helper parity"
date: "2024-01-01"
faq:
  - question: "Q1?"
    answer: "A1"
  - question: "Q2?"
    answer: "A2"
---

# With FAQ
"#;
        let a = assess_faq_source(with_fm);
        assert_eq!(frontmatter_faq_count(with_fm), a.frontmatter_count);
        assert_eq!(has_frontmatter_faq(with_fm), a.frontmatter_count > 0);
        assert_eq!(has_inline_json_ld_faq(with_fm), a.has_inline_faqpage);

        let with_inline = r#"---
title: "JSON-LD FAQ"
description: "Inline schema only for focused helper parity check"
date: "2024-01-01"
---

# JSON-LD FAQ

<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "What is this?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "This is a test."
      }
    }
  ]
}
</script>
"#;
        let b = assess_faq_source(with_inline);
        assert_eq!(frontmatter_faq_count(with_inline), b.frontmatter_count);
        assert!(!has_frontmatter_faq(with_inline));
        assert_eq!(has_inline_json_ld_faq(with_inline), b.has_inline_faqpage);
        assert!(has_inline_json_ld_faq(with_inline));
    }
}
