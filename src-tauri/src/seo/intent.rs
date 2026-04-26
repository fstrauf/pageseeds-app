use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Search intent classification for a keyword.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IntentClassification {
    pub keyword: String,
    pub intent: String,           // "informational" | "navigational" | "transactional" | "commercial"
    pub confidence: Option<f64>,  // DataForSEO only
}

/// Check if a keyword contains a pattern with word-boundary awareness.
/// Multi-word patterns use substring matching; single-word patterns match whole words only.
fn keyword_matches_pattern(keyw: &str, pattern: &str) -> bool {
    if pattern.contains(' ') {
        keyw.contains(pattern)
    } else {
        keyw.split(|c: char| !c.is_alphanumeric())
            .any(|word| word == pattern)
    }
}

/// Classify search intent using keyword pattern matching (Ahrefs fallback).
/// This is a deterministic mapping based on keyword patterns.
pub fn classify_by_pattern(keyword: &str) -> IntentClassification {
    let kw_lower = keyword.to_lowercase();

    // Informational patterns
    let informational_patterns = [
        "how to", "what is", "what are", "guide", "tutorial", "why ", "tips",
        "explain", "meaning", "definition", "examples", "learn", "understand",
        "beginner", "beginners", "introduction", "overview", "basics",
    ];

    // Transactional patterns
    let transactional_patterns = [
        "buy", "price", "discount", "coupon", "deal", "cheap", "order",
        "purchase", "shop", "sale", "free shipping", "add to cart",
        "subscription", "subscribe", "sign up", "register", "download",
    ];

    // Commercial patterns
    let commercial_patterns = [
        "best", "top", "review", "reviews", "vs", "versus", "comparison",
        "compare", "alternative", "alternatives", "recommendation",
        "recommended", "rated", "rating", "pros and cons",
    ];

    // Navigational patterns (brand names, login, specific sites)
    let navigational_patterns = [
        "login", "sign in", "log in", "signup", "register", "account",
        "customer service", "contact", "phone number", "address",
        "hours", "location", "directions",
    ];

    // Check patterns in order of specificity
    for pattern in &transactional_patterns {
        if keyword_matches_pattern(&kw_lower, pattern) {
            return IntentClassification {
                keyword: keyword.to_string(),
                intent: "transactional".to_string(),
                confidence: None,
            };
        }
    }

    for pattern in &commercial_patterns {
        if keyword_matches_pattern(&kw_lower, pattern) {
            return IntentClassification {
                keyword: keyword.to_string(),
                intent: "commercial".to_string(),
                confidence: None,
            };
        }
    }

    for pattern in &navigational_patterns {
        if keyword_matches_pattern(&kw_lower, pattern) {
            return IntentClassification {
                keyword: keyword.to_string(),
                intent: "navigational".to_string(),
                confidence: None,
            };
        }
    }

    for pattern in &informational_patterns {
        if keyword_matches_pattern(&kw_lower, pattern) {
            return IntentClassification {
                keyword: keyword.to_string(),
                intent: "informational".to_string(),
                confidence: None,
            };
        }
    }
    
    // Check for question words at the start
    if kw_lower.starts_with("how ") 
        || kw_lower.starts_with("what ")
        || kw_lower.starts_with("why ")
        || kw_lower.starts_with("when ")
        || kw_lower.starts_with("where ")
        || kw_lower.starts_with("who ")
        || kw_lower.starts_with("which ")
        || kw_lower.starts_with("can ")
        || kw_lower.starts_with("is ")
        || kw_lower.starts_with("are ")
        || kw_lower.starts_with("does ")
        || kw_lower.starts_with("do ")
    {
        return IntentClassification {
            keyword: keyword.to_string(),
            intent: "informational".to_string(),
            confidence: None,
        };
    }
    
    // Default to informational for unknown keywords
    IntentClassification {
        keyword: keyword.to_string(),
        intent: "informational".to_string(),
        confidence: None,
    }
}

/// Batch classify keywords using pattern matching.
pub fn classify_batch_by_pattern(keywords: &[String]) -> Vec<IntentClassification> {
    keywords.iter()
        .map(|kw| classify_by_pattern(kw))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_informational_patterns() {
        let cases = [
            ("how to cook pasta", "informational"),
            ("what is seo", "informational"),
            ("beginner guide to rust", "informational"),
            ("tips for better sleep", "informational"),
        ];
        
        for (keyword, expected) in cases {
            let result = classify_by_pattern(keyword);
            assert_eq!(result.intent, expected, "Failed for: {}", keyword);
        }
    }

    #[test]
    fn test_transactional_patterns() {
        let cases = [
            ("buy iphone 15", "transactional"),
            ("best price for laptop", "transactional"),
            ("order pizza online", "transactional"),
            ("discount codes", "transactional"),
        ];
        
        for (keyword, expected) in cases {
            let result = classify_by_pattern(keyword);
            assert_eq!(result.intent, expected, "Failed for: {}", keyword);
        }
    }

    #[test]
    fn test_commercial_patterns() {
        let cases = [
            ("best running shoes", "commercial"),
            ("top 10 smartphones", "commercial"),
            ("iphone vs samsung review", "commercial"),
            ("alternative to photoshop", "commercial"),
        ];
        
        for (keyword, expected) in cases {
            let result = classify_by_pattern(keyword);
            assert_eq!(result.intent, expected, "Failed for: {}", keyword);
        }
    }

    #[test]
    fn test_question_words() {
        let cases = [
            ("is coffee bad for you", "informational"),
            ("can dogs eat chocolate", "informational"),
            ("what time is it", "informational"),
        ];
        
        for (keyword, expected) in cases {
            let result = classify_by_pattern(keyword);
            assert_eq!(result.intent, expected, "Failed for: {}", keyword);
        }
    }
}
