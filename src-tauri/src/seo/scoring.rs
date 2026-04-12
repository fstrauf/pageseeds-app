use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;
use crate::seo::keywords::KeywordIdea;
use crate::seo::intent::IntentClassification;

/// Multi-factor opportunity score for a keyword.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OpportunityScore {
    pub keyword: String,
    pub total_score: f64,        // 0.0–1.0
    pub tier: String,            // "high" | "medium" | "low"
    pub factor_scores: HashMap<String, f64>,
}

/// Score a keyword opportunity using the multi-factor model.
/// 
/// Factors and weights:
/// - Volume: 25%
/// - KD (inverted): 20%
/// - Intent alignment: 20%
/// - Competition: 15%
/// - Content gap: 10%
/// - CPC signal: 5%
/// - Freshness: 5%
pub fn score_opportunity(
    keyword: &KeywordIdea,
    intent: Option<&IntentClassification>,
    existing_slugs: &[String],
) -> OpportunityScore {
    let mut factor_scores = HashMap::new();
    
    // 1. Volume score (25%)
    let volume_score = calculate_volume_score(keyword);
    factor_scores.insert("volume".to_string(), volume_score);
    
    // 2. KD score (20%) - inverted so lower KD = higher score
    let kd_score = calculate_kd_score(keyword);
    factor_scores.insert("keyword_difficulty".to_string(), kd_score);
    
    // 3. Intent alignment (20%)
    let intent_score = calculate_intent_score(intent);
    factor_scores.insert("intent_alignment".to_string(), intent_score);
    
    // 4. Competition (15%)
    let competition_score = calculate_competition_score(keyword);
    factor_scores.insert("competition".to_string(), competition_score);
    
    // 5. Content gap (10%)
    let gap_score = calculate_gap_score(keyword, existing_slugs);
    factor_scores.insert("content_gap".to_string(), gap_score);
    
    // 6. CPC signal (5%)
    let cpc_score = calculate_cpc_score(keyword);
    factor_scores.insert("cpc".to_string(), cpc_score);
    
    // 7. Freshness (5%) - default to 0.5 when no data
    let freshness_score = 0.5;
    factor_scores.insert("freshness".to_string(), freshness_score);
    
    // Calculate weighted total
    let total_score = 
        volume_score * 0.25 +
        kd_score * 0.20 +
        intent_score * 0.20 +
        competition_score * 0.15 +
        gap_score * 0.10 +
        cpc_score * 0.05 +
        freshness_score * 0.05;
    
    // Determine tier
    let tier = if total_score >= 0.7 {
        "high"
    } else if total_score >= 0.4 {
        "medium"
    } else {
        "low"
    }.to_string();
    
    OpportunityScore {
        keyword: keyword.keyword.clone(),
        total_score,
        tier,
        factor_scores,
    }
}

/// Batch score multiple keywords.
pub fn score_opportunities(
    keywords: &[KeywordIdea],
    intents: &[IntentClassification],
    existing_slugs: &[String],
) -> Vec<OpportunityScore> {
    keywords.iter()
        .map(|kw| {
            let intent = intents.iter().find(|i| i.keyword == kw.keyword);
            score_opportunity(kw, intent, existing_slugs)
        })
        .collect()
}

// ─── Individual Factor Scoring ────────────────────────────────────────────────

/// Calculate volume score (0-1).
fn calculate_volume_score(keyword: &KeywordIdea) -> f64 {
    // If we have exact volume from DataForSEO
    if let Some(exact) = keyword.volume_exact {
        // Scale: 0 = 0, 100k+ = 1.0
        let normalized = (exact as f64 / 100000.0).min(1.0);
        return normalized;
    }
    
    // Otherwise use Ahrefs categorical volume
    match keyword.volume.as_deref() {
        Some("MoreThanTenThousand") => 0.9,
        Some("MoreThanThousand") => 0.8,
        Some("MoreThanFiveHundred") => 0.7,
        Some("MoreThanOneHundred") => 0.6,
        Some("HundredToThousand") => 0.5,
        Some("LessThanOneHundred") => 0.3,
        Some("LessThanTen") => 0.1,
        _ => 0.5, // Unknown
    }
}

/// Calculate keyword difficulty score (inverted, 0-1).
fn calculate_kd_score(keyword: &KeywordIdea) -> f64 {
    // Parse difficulty from label or use default
    let difficulty = match keyword.difficulty.as_deref() {
        Some("Low") => 20.0,
        Some("Easy") => 20.0,
        Some("Medium") => 50.0,
        Some("Hard") => 80.0,
        Some("Very Hard") => 95.0,
        _ => 50.0, // Unknown = medium
    };
    
    // Invert: lower difficulty = higher score
    1.0 - (difficulty / 100.0)
}

/// Calculate intent alignment score (0-1).
fn calculate_intent_score(intent: Option<&IntentClassification>) -> f64 {
    match intent {
        Some(i) => match i.intent.as_str() {
            "transactional" => 1.0,
            "commercial" => 1.0,
            "informational" => 0.5,
            "navigational" => 0.2,
            _ => 0.5,
        },
        None => 0.5, // Unknown intent
    }
}

/// Calculate competition score (0-1).
fn calculate_competition_score(keyword: &KeywordIdea) -> f64 {
    // If we have competition score from DataForSEO (0-1)
    if let Some(comp) = keyword.competition {
        // Invert: lower competition = higher score
        return 1.0 - comp;
    }
    
    // Otherwise derive from difficulty
    let difficulty = match keyword.difficulty.as_deref() {
        Some("Low") => 0.2,
        Some("Easy") => 0.2,
        Some("Medium") => 0.5,
        Some("Hard") => 0.8,
        Some("Very Hard") => 0.95,
        _ => 0.5,
    };
    
    1.0 - difficulty
}

/// Calculate content gap score (0-1).
fn calculate_gap_score(keyword: &KeywordIdea, existing_slugs: &[String]) -> f64 {
    let kw_normalized = keyword.keyword.to_lowercase().replace(" ", "-");
    
    // Check if keyword exists in existing slugs
    for slug in existing_slugs {
        let slug_lower = slug.to_lowercase();
        if slug_lower.contains(&kw_normalized) || kw_normalized.contains(&slug_lower) {
            return 0.0; // Already covered
        }
    }
    
    // Check for partial matches (related content)
    let keyword_lower = keyword.keyword.to_lowercase();
    let keyword_words: Vec<&str> = keyword_lower.split_whitespace().collect();
    for slug in existing_slugs {
        let slug_lower = slug.to_lowercase().replace("-", " ");
        let matching_words = keyword_words.iter()
            .filter(|word| slug_lower.contains(**word))
            .count();
        
        if matching_words > 0 && matching_words >= keyword_words.len() / 2 {
            return 0.5; // Partially covered
        }
    }
    
    1.0 // New keyword
}

/// Calculate CPC score (0-1).
fn calculate_cpc_score(keyword: &KeywordIdea) -> f64 {
    // If we have CPC from DataForSEO
    if let Some(cpc) = keyword.cpc {
        // Scale: $0 = 0, $10+ = 1.0
        let normalized = (cpc / 10.0).min(1.0);
        return normalized;
    }
    
    // No CPC data available (Ahrefs)
    0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keyword() -> KeywordIdea {
        KeywordIdea {
            keyword: "test keyword".to_string(),
            idea_type: "regular".to_string(),
            difficulty: Some("Low".to_string()),
            kd: None,
            intent: None,
            volume: Some("MoreThanThousand".to_string()),
            volume_exact: Some(5000),
            cpc: Some(2.5),
            competition: Some(0.3),
            country: Some("us".to_string()),
        }
    }

    #[test]
    fn test_score_opportunity() {
        let keyword = test_keyword();
        let intent = IntentClassification {
            keyword: "test keyword".to_string(),
            intent: "informational".to_string(),
            confidence: None,
        };
        
        let score = score_opportunity(&keyword, Some(&intent), &[]);
        
        assert_eq!(score.keyword, "test keyword");
        assert!(score.total_score > 0.0 && score.total_score <= 1.0);
        assert!(score.factor_scores.contains_key("volume"));
        assert!(score.factor_scores.contains_key("keyword_difficulty"));
    }

    #[test]
    fn test_tier_thresholds() {
        let high_score = OpportunityScore {
            keyword: "high".to_string(),
            total_score: 0.8,
            tier: "high".to_string(),
            factor_scores: HashMap::new(),
        };
        assert_eq!(high_score.tier, "high");
        
        let medium_score = OpportunityScore {
            keyword: "medium".to_string(),
            total_score: 0.5,
            tier: "medium".to_string(),
            factor_scores: HashMap::new(),
        };
        assert_eq!(medium_score.tier, "medium");
        
        let low_score = OpportunityScore {
            keyword: "low".to_string(),
            total_score: 0.3,
            tier: "low".to_string(),
            factor_scores: HashMap::new(),
        };
        assert_eq!(low_score.tier, "low");
    }

    #[test]
    fn test_content_gap_scoring() {
        let keyword = KeywordIdea {
            keyword: "new topic".to_string(),
            idea_type: "regular".to_string(),
            difficulty: Some("Low".to_string()),
            kd: None,
            intent: None,
            volume: Some("MoreThanThousand".to_string()),
            volume_exact: Some(5000),
            cpc: Some(2.5),
            competition: Some(0.3),
            country: Some("us".to_string()),
        };
        
        // New keyword should score 1.0
        let score_new = calculate_gap_score(&keyword, &["existing-article".to_string()]);
        assert_eq!(score_new, 1.0);
        
        // Covered keyword should score 0.0
        let score_covered = calculate_gap_score(&keyword, &["new-topic".to_string()]);
        assert_eq!(score_covered, 0.0);
    }
}
