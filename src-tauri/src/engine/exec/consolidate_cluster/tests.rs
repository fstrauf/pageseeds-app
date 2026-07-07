use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_headings() {
        let body = r#"# Title

## Section One
Some text here.

### Subsection
More text.

## Section Two
Final text.
"#;
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Section One");
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[1].text, "Section Two");
    }

    #[test]
    fn test_extract_tables() {
        let body = r#"# Title

| Col A | Col B |
|-------|-------|
| 1     | 2     |

Some text.
"#;
        let tables = extract_tables(body);
        assert_eq!(tables.len(), 1);
        assert!(tables[0].markdown.contains("Col A"));
    }

    #[test]
    fn test_extract_examples() {
        let body = r#"# Title

```python
print("hello")
```

Some text.
"#;
        let examples = extract_examples(body);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].language.as_deref(), Some("python"));
        assert!(examples[0].code.contains("hello"));
    }

    #[test]
    fn test_extract_faqs() {
        let body = r#"# Title

Q: What is this?
A: It is a test.

Q: Why?
A: Because.
"#;
        let faqs = extract_faqs(body);
        assert_eq!(faqs.len(), 2);
        assert_eq!(faqs[0].question, "What is this?");
        assert_eq!(faqs[0].answer, "It is a test.");
    }

    #[test]
    fn test_merge_preflight_report_roundtrip() {
        let report = MergePreflightReport {
            keeper_file_exists: true,
            keeper_is_indexable: true,
            redirect_files_exist: vec!["/blog/a".to_string()],
            redirect_files_missing: vec![],
            redirect_cycles_detected: vec![],
            can_proceed: true,
            notes: vec!["ok".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: MergePreflightReport = serde_json::from_str(&json).unwrap();
        assert!(decoded.can_proceed);
    }
}
