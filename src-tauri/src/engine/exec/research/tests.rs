//! Tests for `final_selection` — extracted into a sibling file to keep the
//! implementation under the size threshold, following the
//! `engine/exec/keywords/tests.rs` precedent (wired via `mod tests;` in `mod.rs`).

#[cfg(test)]
mod tests {
    use crate::engine::exec::research::final_selection::*;
    use crate::models::research::{KeywordPipelineOutput, ScoredKeyword, SelectedKeyword};

    fn kw(
        keyword: &str,
        volume: i64,
        kd: f64,
        intent: &str,
    ) -> ScoredKeyword {
        ScoredKeyword {
            keyword: keyword.to_string(),
            volume: Some(volume),
            kd: Some(kd),
            intent: Some(intent.to_string()),
            traffic: None,
            has_data: Some(true),
            intent_confidence: None,
            gap_score: None,
            cpc: None,
        }
    }

    fn selected(
        keyword: &str,
        volume: i64,
        kd: i64,
        winnability: Option<&str>,
        gap_score: Option<f64>,
    ) -> SelectedKeyword {
        SelectedKeyword {
            keyword: keyword.to_string(),
            volume,
            difficulty: kd,
            traffic: None,
            selection_reason: String::new(),
            recommended_title: String::new(),
            intent: Some("informational".to_string()),
            winnability: winnability.map(|s| s.to_string()),
            winnability_reason: None,
            gap_score,
        }
    }

    fn picker_output(results: Vec<SelectedKeyword>) -> KeywordPickerOutput {
        let total = results.len();
        KeywordPickerOutput {
            landing_page_candidates: Vec::new(),
            difficulty: Some(DifficultyWrapper {
                total,
                successful: total,
                results,
            }),
            total_candidates: total,
            filtered_out: 0,
        }
    }

    fn result_keywords(output: &KeywordPickerOutput) -> Vec<String> {
        output
            .difficulty
            .as_ref()
            .unwrap()
            .results
            .iter()
            .map(|r| r.keyword.clone())
            .collect()
    }

    fn build_pipeline(keywords: Vec<ScoredKeyword>) -> KeywordPipelineOutput {
        KeywordPipelineOutput {
            keywords,
            themes: vec!["covered calls".to_string()],
            competitors: vec![],
            competitor_insights: vec![],
            total_candidates: 0,
            with_data_count: 0,
        }
    }

    #[test]
    fn blog_selection_prefers_informational_intent() {
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 25.0, "informational"),
            kw("covered call tracker", 800, 25.0, "commercial"),
            kw("what is a covered call", 3000, 20.0, "informational"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, false, 10).unwrap();
        let results = output.difficulty.unwrap().results;
        assert!(
            results.iter().any(|r| r.keyword == "how to sell covered calls"),
            "informational keyword should be selected for blog"
        );
        assert!(
            !results.iter().any(|r| r.keyword == "covered call tracker"),
            "commercial keyword should not be selected for blog"
        );
    }

    #[test]
    fn landing_page_selection_prefers_commercial_intent() {
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 25.0, "informational"),
            kw("covered call tracker", 800, 25.0, "commercial"),
            kw("best covered call screener", 600, 30.0, "commercial"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, true, 10).unwrap();
        let candidates = output.landing_page_candidates;
        assert!(
            candidates.iter().any(|c| c.keyword == "covered call tracker"),
            "commercial keyword should be selected for landing page"
        );
        assert!(
            !candidates.iter().any(|c| c.keyword == "how to sell covered calls"),
            "informational keyword should not be selected for landing page"
        );
    }

    #[test]
    fn selection_fails_when_nothing_matches_filters() {
        // All keywords exceed KD 30 — no fallback, the function should fail
        // with an actionable error rather than silently relaxing the bar.
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 55.0, "informational"),
            kw("covered call strike selection", 400, 50.0, "informational"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let result = select_keywords_deterministic(&json, false, 10);
        assert!(
            result.is_err(),
            "should fail (not fallback) when no keywords met the KD bar"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("No keywords met the quality bar"),
            "error should explain the failure: {}",
            err
        );
    }

    #[test]
    fn title_generation_matches_workflow() {
        let year = chrono::Datelike::year(&chrono::Utc::now());
        assert_eq!(
            generate_title("how to sell covered calls", false),
            "How to Sell Covered Calls: A Step-by-Step Guide"
        );
        // No site-specific branding or stale years in generated titles.
        assert_eq!(
            generate_title("covered call tracker", true),
            "Covered Call Tracker"
        );
        assert_eq!(
            generate_title("best covered call screener", true),
            format!("Best Covered Call Screener ({})", year)
        );
        assert_eq!(
            generate_title("optionstrat vs tastytrade", true),
            "Optionstrat vs Tastytrade: Which is Right for You?"
        );
    }

    #[test]
    fn landing_selection_ranks_by_volume_times_cpc() {
        // Lower volume but much higher CPC must outrank raw volume for
        // landing pages — commercial value is the primary ranking signal.
        let mut high_value = kw("options profit calculator", 400, 20.0, "commercial");
        high_value.cpc = Some(5.0); // 400 × $5.00 = 2000
        let mut high_volume = kw("stock screener app", 3000, 20.0, "commercial");
        high_volume.cpc = Some(0.30); // 3000 × $0.30 = 900
        let pipeline = build_pipeline(vec![high_volume, high_value]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, true, 10).unwrap();
        let candidates = output.landing_page_candidates;
        assert_eq!(candidates[0].keyword, "options profit calculator");
        assert_eq!(candidates[0].cpc, Some(5.0));
        assert_eq!(candidates[1].keyword, "stock screener app");
    }

    #[test]
    fn landing_opportunity_score_derives_from_commercial_value() {
        let mut top = kw("top keyword", 1000, 20.0, "commercial");
        top.cpc = Some(4.0); // 4000
        let mut mid = kw("mid keyword", 1000, 20.0, "commercial");
        mid.cpc = Some(2.0); // 2000 → ratio 0.5 → medium
        let mut low = kw("low keyword", 1000, 20.0, "commercial");
        low.cpc = Some(0.5); // 500 → ratio 0.125 → low
        let pipeline = build_pipeline(vec![low, mid, top]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, true, 10).unwrap();
        let candidates = output.landing_page_candidates;
        assert_eq!(candidates[0].opportunity_score, "high");
        assert_eq!(candidates[1].opportunity_score, "medium");
        assert_eq!(candidates[2].opportunity_score, "low");
        // The reason surfaces CPC, not just KD/volume.
        assert!(candidates[0].opportunity_reason.contains("$4.00 CPC"));
    }

    #[test]
    fn landing_winnability_sort_demotes_avoid_despite_higher_value() {
        let mut target = kw("winnable tool", 500, 20.0, "commercial");
        target.cpc = Some(2.0);
        let mut avoid = kw("authority dominated tool", 5000, 10.0, "commercial");
        avoid.cpc = Some(9.0);
        let pipeline = build_pipeline(vec![target, avoid]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (mut output, _) = select_keywords_deterministic(&json, true, 10).unwrap();
        // Simulate enrichment: the high-value keyword is SERP-dominated.
        output.landing_page_candidates[0].winnability = Some("avoid".to_string());
        output.landing_page_candidates[1].winnability = Some("target".to_string());
        sort_by_winnability(&mut output);
        assert_eq!(
            output.landing_page_candidates[0].keyword,
            "winnable tool",
            "avoid bucket must sink below target despite higher commercial value"
        );
    }

    #[test]
    fn off_domain_filter_removes_flagged_case_insensitively() {
        let pipeline = build_pipeline(vec![
            kw("what is iv crush", 260, 0.0, "informational"),
            kw("assignment risk ao3", 140, 0.0, "informational"),
            kw("iv crush meaning", 210, 0.0, "informational"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (mut output, _) = select_keywords_deterministic(&json, false, 10).unwrap();

        // Production lowercases flagged keywords before building the set.
        let off_domain: std::collections::HashSet<String> =
            ["assignment risk ao3".to_string()].into_iter().collect();
        let removed = apply_off_domain_filter(&mut output, &off_domain);

        assert_eq!(removed, 1);
        let results = output.difficulty.unwrap().results;
        assert_eq!(results.len(), 2);
        assert!(!results.iter().any(|r| r.keyword == "assignment risk ao3"));
    }

    #[test]
    fn off_domain_filter_empty_set_is_noop() {
        let pipeline = build_pipeline(vec![kw("what is iv crush", 260, 0.0, "informational")]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (mut output, _) = select_keywords_deterministic(&json, false, 10).unwrap();

        let removed = apply_off_domain_filter(&mut output, &std::collections::HashSet::new());
        assert_eq!(removed, 0);
        assert_eq!(output.difficulty.unwrap().results.len(), 1);
    }

    #[test]
    fn trim_to_final_caps_both_output_shapes() {
        let kws: Vec<ScoredKeyword> = (0..15)
            .map(|i| {
                let name = format!("kw {}", i);
                kw(&name, 1000 - i as i64, 10.0, "informational")
            })
            .collect();
        let pipeline = build_pipeline(kws);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (mut output, _) = select_keywords_deterministic(&json, false, 15).unwrap();
        assert_eq!(selected_count(&output), 15);

        trim_to_final(&mut output, FINAL_RESULTS);
        assert_eq!(selected_count(&output), FINAL_RESULTS);
        let d = output.difficulty.unwrap();
        assert_eq!(d.total, FINAL_RESULTS);
        // Highest-volume entries survive the trim.
        assert!(d.results.iter().any(|r| r.keyword == "kw 0"));
        assert!(!d.results.iter().any(|r| r.keyword == "kw 14"));
    }

    #[test]
    fn selection_uses_gap_score_as_final_tiebreak() {
        // Same volume and KD: the thinner-cluster keyword sorts first, and the
        // gap score survives into the picker artifact.
        let mut thin = kw("thin cluster keyword", 1000, 10.0, "informational");
        thin.gap_score = Some(80.0);
        let mut covered = kw("covered cluster keyword", 1000, 10.0, "informational");
        covered.gap_score = Some(20.0);
        let pipeline = build_pipeline(vec![covered, thin]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, false, 10).unwrap();
        let results = output.difficulty.unwrap().results;
        assert_eq!(results[0].keyword, "thin cluster keyword");
        assert_eq!(results[0].gap_score, Some(80.0));
        assert_eq!(results[1].keyword, "covered cluster keyword");
    }

    #[test]
    fn winnability_sort_demotes_avoid_despite_higher_volume() {
        let mut output = picker_output(vec![
            selected("avoid high volume", 5000, 10, Some("avoid"), None),
            selected("target mid volume", 1000, 20, Some("target"), None),
            selected("differentiate low", 500, 15, Some("differentiate"), None),
            selected("unscored", 800, 25, None, None),
        ]);
        sort_by_winnability(&mut output);
        // Missing bucket ranks as target-equivalent; avoid sinks to the bottom.
        assert_eq!(
            result_keywords(&output),
            vec![
                "target mid volume",
                "unscored",
                "differentiate low",
                "avoid high volume"
            ]
        );
    }

    #[test]
    fn winnability_sort_preserves_volume_kd_gap_order_within_a_bucket() {
        let mut output = picker_output(vec![
            selected("low gap", 1000, 10, Some("target"), Some(20.0)),
            selected("high volume", 2000, 25, Some("target"), Some(50.0)),
            selected("high gap", 1000, 10, Some("target"), Some(80.0)),
            selected("lower kd", 1000, 5, Some("target"), None),
        ]);
        sort_by_winnability(&mut output);
        // Volume desc first, then KD asc, then gap desc.
        assert_eq!(
            result_keywords(&output),
            vec!["high volume", "lower kd", "high gap", "low gap"]
        );
    }

    #[test]
    fn trim_after_sort_drops_avoid_when_enough_better_candidates_exist() {
        // 11 candidates for 10 slots: the Avoid keyword has the highest volume
        // but must still fall out after sort + trim.
        let mut results: Vec<SelectedKeyword> = (0..10)
            .map(|i| {
                let name = format!("target {}", i);
                selected(&name, 1000 - i as i64, 10, Some("target"), None)
            })
            .collect();
        results.push(selected("avoid keyword", 9000, 5, Some("avoid"), None));
        let mut output = picker_output(results);

        sort_by_winnability(&mut output);
        trim_to_final(&mut output, FINAL_RESULTS);

        let keywords = result_keywords(&output);
        assert_eq!(keywords.len(), FINAL_RESULTS);
        assert!(!keywords.iter().any(|k| k == "avoid keyword"));
        assert!(keywords.iter().any(|k| k == "target 9"));
    }

    #[test]
    fn avoid_survives_trim_when_not_enough_better_candidates() {
        let mut output = picker_output(vec![
            selected("target one", 1000, 10, Some("target"), None),
            selected("avoid keyword", 9000, 5, Some("avoid"), None),
        ]);
        sort_by_winnability(&mut output);
        trim_to_final(&mut output, FINAL_RESULTS);
        assert_eq!(result_keywords(&output), vec!["target one", "avoid keyword"]);
    }

    #[test]
    fn winnability_sort_is_deterministic_for_identical_inputs() {
        let build = || {
            picker_output(vec![
                selected("a", 1000, 10, Some("target"), Some(80.0)),
                selected("b", 1000, 10, Some("avoid"), None),
                selected("c", 1000, 10, None, Some(50.0)),
                selected("d", 2000, 20, Some("differentiate"), None),
                selected("e", 500, 5, Some("target"), None),
            ])
        };
        let mut first = build();
        let mut second = build();
        sort_by_winnability(&mut first);
        sort_by_winnability(&mut second);
        assert_eq!(result_keywords(&first), result_keywords(&second));
    }
}
