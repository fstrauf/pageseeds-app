/// Search Intent Classifier
///
/// Ported from SEO Machine's search_intent_analyzer.py
/// Classifies keywords into search intent categories using pattern matching.
///
/// # Search Intent Types
/// - **Informational**: User wants to learn something (what, why, how, guide, tutorial)
/// - **Commercial**: User is researching before buying (best, top, vs, comparison, review)
/// - **Transactional**: User wants to make a purchase (buy, price, discount, order)
/// - **Navigational**: User wants to go somewhere (login, website, official, dashboard)
use serde::{Deserialize, Serialize};

/// Search intent classification result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SearchIntent {
    Informational,
    Navigational,
    Transactional,
    Commercial,
}

impl SearchIntent {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchIntent::Informational => "informational",
            SearchIntent::Navigational => "navigational",
            SearchIntent::Transactional => "transactional",
            SearchIntent::Commercial => "commercial",
        }
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            SearchIntent::Informational => "Informational",
            SearchIntent::Navigational => "Navigational",
            SearchIntent::Transactional => "Transactional",
            SearchIntent::Commercial => "Commercial",
        }
    }

    /// Get UI color hint
    pub fn color_hint(&self) -> &'static str {
        match self {
            SearchIntent::Informational => "blue",   // Blog posts, guides
            SearchIntent::Commercial => "green",     // Comparison, review pages
            SearchIntent::Transactional => "orange", // Landing pages, product pages
            SearchIntent::Navigational => "gray",    // Usually not content targets
        }
    }
}

/// Intent scores for each category
#[derive(Debug, Clone, Default)]
struct IntentScores {
    informational: f64,
    navigational: f64,
    transactional: f64,
    commercial: f64,
}

impl IntentScores {
    /// Get the primary intent (highest score)
    fn primary(&self) -> SearchIntent {
        let mut max_score = self.informational;
        let mut intent = SearchIntent::Informational;

        if self.navigational > max_score {
            max_score = self.navigational;
            intent = SearchIntent::Navigational;
        }
        if self.transactional > max_score {
            max_score = self.transactional;
            intent = SearchIntent::Transactional;
        }
        if self.commercial > max_score {
            intent = SearchIntent::Commercial;
        }

        intent
    }

    /// Calculate confidence percentage for the primary intent
    fn confidence(&self) -> f64 {
        let total = self.informational + self.navigational + self.transactional + self.commercial;
        if total == 0.0 {
            return 25.0; // Default equal distribution
        }

        let primary_score = match self.primary() {
            SearchIntent::Informational => self.informational,
            SearchIntent::Navigational => self.navigational,
            SearchIntent::Transactional => self.transactional,
            SearchIntent::Commercial => self.commercial,
        };

        (primary_score / total * 100.0).round()
    }
}

/// Informational intent signal keywords
const INFORMATIONAL_SIGNALS: &[&str] = &[
    "what",
    "why",
    "how",
    "when",
    "where",
    "who",
    "guide",
    "tutorial",
    "learn",
    "tips",
    "best practices",
    "explained",
    "definition",
    "meaning",
    "examples",
    "introduction",
    "overview",
    "basics",
    "fundamentals",
    "beginner",
];

/// Navigational intent signal keywords
const NAVIGATIONAL_SIGNALS: &[&str] = &[
    "login",
    "sign in",
    "signin",
    "website",
    "official",
    "home page",
    "homepage",
    "account",
    "dashboard",
    "portal",
    "app",
    "platform",
    "download",
];

/// Transactional intent signal keywords
const TRANSACTIONAL_SIGNALS: &[&str] = &[
    "buy",
    "purchase",
    "order",
    "download",
    "get",
    "pricing",
    "price",
    "cost",
    "free trial",
    "free",
    "sign up",
    "signup",
    "subscribe",
    "install",
    "coupon",
    "deal",
    "discount",
    "cheap",
    "affordable",
    "for sale",
];

/// Commercial intent signal keywords
const COMMERCIAL_SIGNALS: &[&str] = &[
    "best",
    "top",
    "review",
    "reviews",
    "rated",
    "vs",
    "versus",
    "compare",
    "comparison",
    "alternative",
    "alternatives",
    "like",
    "similar",
    "better than",
    "instead of",
    "or",
    "option",
    "options",
    "choice",
    "features",
];

/// Classify the search intent of a keyword
///
/// # Arguments
/// * `keyword` - The search keyword/phrase to analyze
///
/// # Returns
/// Tuple of (primary_intent, confidence_percentage)
///
/// # Examples
/// ```
/// use pageseeds_lib::engine::exec::intent_classifier::{classify_intent, SearchIntent};
///
/// let (intent, confidence) = classify_intent("how to start a podcast");
/// assert_eq!(intent, SearchIntent::Informational);
///
/// let (intent, confidence) = classify_intent("best podcast hosting");
/// assert_eq!(intent, SearchIntent::Commercial);
/// ```
pub fn classify_intent(keyword: &str) -> (SearchIntent, f64) {
    let keyword_lower = keyword.to_lowercase();
    let mut scores = IntentScores::default();

    // Score based on signal words
    for signal in INFORMATIONAL_SIGNALS {
        if keyword_lower.contains(signal) {
            scores.informational += 2.0;
        }
    }

    for signal in NAVIGATIONAL_SIGNALS {
        if keyword_lower.contains(signal) {
            scores.navigational += 3.0; // Stronger signal for navigational
        }
    }

    for signal in TRANSACTIONAL_SIGNALS {
        if keyword_lower.contains(signal) {
            scores.transactional += 2.0;
        }
    }

    for signal in COMMERCIAL_SIGNALS {
        if keyword_lower.contains(signal) {
            scores.commercial += 2.0;
        }
    }

    // Pattern-based scoring

    // Questions are typically informational
    if is_question(&keyword_lower) {
        scores.informational += 3.0;
    }

    // Lists and comparisons are commercial
    if is_listicle(&keyword_lower) {
        scores.commercial += 3.0;
    }

    // Brand + generic term patterns suggest navigational
    // e.g., "slack login", "github pricing"
    let words: Vec<&str> = keyword_lower.split_whitespace().collect();
    if words.len() == 2 {
        // Check if second word is a navigational signal
        let second_word = words[1];
        if NAVIGATIONAL_SIGNALS.iter().any(|s| s.contains(second_word)) {
            scores.navigational += 2.0;
        }
    }

    // Long-tail question patterns (very specific informational)
    if words.len() >= 4 && is_question(&keyword_lower) {
        scores.informational += 1.0;
    }

    let intent = scores.primary();
    let confidence = scores.confidence();

    (intent, confidence)
}

/// Check if keyword is a question
fn is_question(keyword: &str) -> bool {
    let question_starters = [
        "what ", "why ", "how ", "when ", "where ", "who ", "can ", "should ", "is ", "are ",
        "does ", "do ", "what's ", "whats ", "which ", "will ", "would ",
    ];

    question_starters
        .iter()
        .any(|starter| keyword.starts_with(starter))
}

/// Check if keyword is a listicle/comparison pattern
fn is_listicle(keyword: &str) -> bool {
    // Patterns like: "10 best", "top 5", "best 10"
    let list_patterns = [
        r"^\d+\s+(best|top)", // "10 best", "5 top"
        r"(best|top)\s+\d+",  // "best 10", "top 5"
    ];

    list_patterns.iter().any(|pattern| {
        regex::Regex::new(pattern)
            .map(|re| re.is_match(keyword))
            .unwrap_or(false)
    })
}

/// Get content recommendations based on intent
///
/// Returns actionable advice for creating content targeting this intent
pub fn get_intent_recommendations(intent: &SearchIntent) -> Vec<&'static str> {
    match intent {
        SearchIntent::Informational => vec![
            "Create comprehensive, educational content",
            "Include step-by-step instructions or explanations",
            "Answer common questions (People Also Ask)",
            "Use FAQ sections and definition boxes",
            "Target featured snippet optimization",
        ],
        SearchIntent::Navigational => vec![
            "Optimize for brand-related searches",
            "Ensure homepage/key pages rank well",
            "Include site navigation and clear CTAs",
            "Strengthen brand presence and awareness",
            "May not need traditional content marketing",
        ],
        SearchIntent::Transactional => vec![
            "Focus on product/service pages",
            "Include clear pricing and purchase options",
            "Add trust signals (reviews, testimonials)",
            "Optimize for conversion, not just traffic",
            "Include strong, action-oriented CTAs",
        ],
        SearchIntent::Commercial => vec![
            "Create comparison and review content",
            "Include pros/cons and alternatives",
            "Add detailed feature breakdowns",
            "Include data tables and comparisons",
            "Show 'best for' categories",
        ],
    }
}

/// Determine recommended content type based on intent
pub fn get_recommended_content_type(intent: &SearchIntent) -> &'static str {
    match intent {
        SearchIntent::Informational => "Guide/Article",
        SearchIntent::Commercial => "Comparison/Review",
        SearchIntent::Transactional => "Landing Page",
        SearchIntent::Navigational => "Homepage/Product Page",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_informational_intent() {
        let test_cases = vec![
            ("how to start a podcast", SearchIntent::Informational),
            ("what is kubernetes", SearchIntent::Informational),
            ("guide to seo", SearchIntent::Informational),
            ("beginner tutorial", SearchIntent::Informational),
            ("why do cats purr", SearchIntent::Informational),
        ];

        for (keyword, expected) in test_cases {
            let (intent, _) = classify_intent(keyword);
            assert_eq!(
                intent, expected,
                "Expected {:?} for '{}'",
                expected, keyword
            );
        }
    }

    #[test]
    fn test_commercial_intent() {
        let test_cases = vec![
            ("best podcast hosting", SearchIntent::Commercial),
            ("top 10 crm software", SearchIntent::Commercial),
            ("hubspot vs salesforce", SearchIntent::Commercial),
            ("alternatives to mailchimp", SearchIntent::Commercial),
            ("review of asana", SearchIntent::Commercial),
        ];

        for (keyword, expected) in test_cases {
            let (intent, _) = classify_intent(keyword);
            assert_eq!(
                intent, expected,
                "Expected {:?} for '{}'",
                expected, keyword
            );
        }
    }

    #[test]
    fn test_transactional_intent() {
        let test_cases = vec![
            ("buy airpods pro", SearchIntent::Transactional),
            ("discount code", SearchIntent::Transactional),
            ("pricing plans", SearchIntent::Transactional),
            ("free trial", SearchIntent::Transactional),
            ("cheap web hosting", SearchIntent::Transactional),
        ];

        for (keyword, expected) in test_cases {
            let (intent, _) = classify_intent(keyword);
            assert_eq!(
                intent, expected,
                "Expected {:?} for '{}'",
                expected, keyword
            );
        }
    }

    #[test]
    fn test_navigational_intent() {
        let test_cases = vec![
            ("github login", SearchIntent::Navigational),
            ("slack download", SearchIntent::Navigational),
            ("notion app", SearchIntent::Navigational),
            ("gmail signin", SearchIntent::Navigational),
        ];

        for (keyword, expected) in test_cases {
            let (intent, _) = classify_intent(keyword);
            assert_eq!(
                intent, expected,
                "Expected {:?} for '{}'",
                expected, keyword
            );
        }
    }

    #[test]
    fn test_confidence_calculation() {
        let (intent, confidence) = classify_intent("how to start a podcast");
        assert_eq!(intent, SearchIntent::Informational);
        assert!(
            confidence > 50.0,
            "Confidence should be > 50%, got {}%",
            confidence
        );
    }

    #[test]
    fn test_is_question() {
        assert!(is_question("what is docker"));
        assert!(is_question("how to cook rice"));
        assert!(is_question("why is the sky blue"));
        assert!(!is_question("docker tutorial"));
        assert!(!is_question("best laptops"));
    }

    #[test]
    fn test_is_listicle() {
        assert!(is_listicle("10 best laptops"));
        assert!(is_listicle("best 10 laptops"));
        assert!(is_listicle("top 5 restaurants"));
        assert!(!is_listicle("how to cook"));
        assert!(!is_listicle("what is kubernetes"));
    }
}
