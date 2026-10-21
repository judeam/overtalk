use super::*;

#[test]
fn qwen_4b_short_meetings_keep_source_checked_decisions_and_actions() {
    let text = "[S1] [00:01] [Alice] I will send the plan by Friday.";
    assert!(checked_reading_window(&LLMProvider::BuiltInAI, "qwen3.5:4b", text, "").is_some());
    assert!(checked_reading_window(&LLMProvider::BuiltInAI, "qwen3.5:4b", " ", "").is_none());
    assert!(checked_reading_window(
        &LLMProvider::BuiltInAI,
        "qwen3.5:4b",
        "[S1] [00:01] [Alice] um",
        ""
    )
    .is_none());
    assert!(checked_reading_window(
        &LLMProvider::BuiltInAI,
        "qwen3.5:4b",
        text,
        "Use my project context"
    )
    .is_none());
}

#[test]
fn generated_report_cannot_replace_checked_action_deadline() {
    let numbered = "[S1] [00:01] [Alice] I will send the plan by Friday.";
    let notes = crate::summary::compact::check_sources(
        crate::summary::compact::parse_items(
            "ACTION: send the plan | owner: Alice | due: Friday [S1]",
            1,
        ),
        numbered,
    );
    let notes = CompactNotes {
        rendered: String::new(),
        windows: 1,
        notes,
        times: crate::summary::compact::line_times(numbered),
    };
    let template = crate::summary::templates::get_template("standard_meeting").unwrap();
    let kinds: Vec<_> = template
        .sections
        .iter()
        .map(|section| crate::summary::compact::section_kind(&section.title))
        .collect();
    let result = assemble_report(
        "# Meeting\n## Action Items\nAlice will finish on Friday, 2024.",
        &template,
        &kinds,
        &notes,
    );
    assert!(result.contains("Friday"));
    assert!(!result.contains("2024"));
}

#[test]
fn standard_report_preserves_pending_conditions_and_future_actions() {
    let numbered = "[S1] [00:01] [Alice] The plan may proceed only after review.\n\
[S2] [00:04] [Daniel] I will prepare the plan by Friday.";
    let notes = crate::summary::compact::check_sources(
        crate::summary::compact::parse_items(
            "SUMMARY: planning the release, with review still pending [S1]\n\
DECISION: the plan may proceed only after review [S1]\n\
ACTION: prepare the plan | owner: Daniel | due: Friday [S2]",
            2,
        ),
        numbered,
    );
    let notes = CompactNotes {
        rendered: String::new(),
        windows: 1,
        notes,
        times: crate::summary::compact::line_times(numbered),
    };
    let result = source_standard_report("standard_meeting", "", &notes).unwrap();
    assert!(result.contains("review still pending [S1]"));
    assert!(result.contains("only after review [S1]"));
    assert!(result.contains("| Daniel | Prepare the plan | Friday |"));
    assert!(!result.contains("has prepared"));
    assert!(!result.contains("was approved"));
    assert!(source_standard_report("custom", "", &notes).is_none());
    assert!(source_standard_report("standard_meeting", "Use my project context", &notes).is_none());
}

#[test]
fn chunk_text_preserves_content_after_early_sentence_boundary() {
    let marker = "LOST_MARKER";
    let text = format!("Intro. {marker} trailing content ensures chunking");

    let chunks = chunk_text(&text, 10, 1);

    assert!(
        chunks.iter().any(|chunk| chunk.contains(marker)),
        "marker was omitted from all chunks: {chunks:?}"
    );
}

#[test]
fn chunk_text_keeps_unicode_boundaries() {
    assert_eq!(chunk_text("é ab", 2, 0), vec!["é", " ab"]);
}

#[test]
fn chunk_text_progresses_when_overlap_matches_window() {
    assert_eq!(chunk_text("abcd", 1, 1), vec!["abc", "d"]);
}

#[test]
fn unicode_chunks_preserve_content_and_budget() {
    let text =
        "会議では東京の予定を検討しました。😀\nИван согласился.\nAction: follow up.".repeat(50);
    let chunks = chunk_text(&text, 90, 0);
    assert_eq!(chunks.concat(), text);
    assert!(chunks.iter().all(|chunk| rough_token_count(chunk) <= 90));
    assert!(rough_token_count("会議") >= 6);
    assert_eq!(
        with_source_ids("Alice: yes\nBob: no"),
        "[S1] Alice: yes\n[S2] Bob: no"
    );
}

#[test]
fn source_references_must_exist_in_the_transcript() {
    assert!(validate_source_references("Agreed [S2]", "Alice: yes\nBob: agreed").is_ok());
    assert!(validate_source_references("Agreed [S3]", "Alice: yes\nBob: agreed").is_err());
    assert!(validate_source_references("Agreed [S0]", "Alice: yes").is_err());
}

#[test]
fn oversized_single_turn_keeps_its_source_id_on_every_fragment() {
    let text = with_source_ids(&format!(
        "00:00 [Alice] {}",
        "Important evidence. ".repeat(100)
    ));
    let chunks = chunk_transcript_evidence(&text, 90, 5);
    assert!(chunks.len() > 2);
    assert!(chunks.iter().all(|chunk| chunk.starts_with("[S1]")));
    assert!(chunks.iter().all(|chunk| rough_token_count(chunk) <= 90));
    assert!(chunks.last().unwrap().contains("evidence."));
}

#[test]
fn evidence_reduction_rejects_lost_sources_but_allows_overlap_deduplication() {
    let before = "Approved beta [S2]. Review by Friday [S9].\n---\nApproved beta [S2].";
    assert!(
        validate_evidence_reduction(before, "Approved beta [S2]. Review by Friday [S9].").is_ok()
    );
    let error = validate_evidence_reduction(before, "Approved beta [S2].").unwrap_err();
    assert!(error.contains("[S9]"));
    assert!(validate_evidence_reduction(before, "Approved beta [S2]. Review [S10].").is_err());
}

fn minimal_template() -> Template {
    serde_json::from_value(serde_json::json!({
        "name": "Review", "description": "Meeting notes", "sections": [{
            "title": "Decisions", "format": "list", "instruction": "Record explicit agreements.",
            "item_format": "Decision with source reference"
        }]
    }))
    .unwrap()
}

#[tokio::test]
async fn pre_cancelled_pipeline_never_requests_a_model() {
    let token = CancellationToken::new();
    token.cancel();
    let result = generate_meeting_summary(
        &LLMProvider::CustomOpenAI,
        "test",
        "",
        "Alice agreed.",
        "",
        "test",
        &minimal_template(),
        4096,
        None,
        Some("invalid-url"),
        None,
        None,
        None,
        None,
        Some(&token),
        Some("en"),
        Some("en"),
        None,
    )
    .await;
    assert_eq!(result.unwrap_err(), "Summary generation was cancelled");
}

#[tokio::test]
async fn oversized_prompt_fails_before_requesting_a_model() {
    let result = generate_meeting_summary(
        &LLMProvider::CustomOpenAI,
        "test",
        "",
        "Alice agreed.",
        &"会議".repeat(5000),
        "test",
        &minimal_template(),
        4096,
        None,
        Some("invalid-url"),
        None,
        None,
        None,
        None,
        None,
        Some("en"),
        Some("en"),
        None,
    )
    .await;
    assert!(result.unwrap_err().contains("additional context exceed"));
}

#[test]
fn chunk_summary_prompt_forces_english_base_output() {
    let prompt = build_chunk_summary_user_prompt("会議の内容");

    assert!(prompt.contains(ENGLISH_BASE_SUMMARY_INSTRUCTION));
    assert!(prompt.contains("<transcript_chunk>"));
}

#[test]
fn combine_summary_prompt_forces_english_base_output() {
    let prompt = build_combine_summary_user_prompt("chunk one\n---\nchunk two");

    assert!(prompt.contains(ENGLISH_BASE_SUMMARY_INSTRUCTION));
    assert!(prompt.contains("<summaries>"));
}

#[test]
fn final_report_prompt_forces_english_base_output() {
    let prompt = build_final_report_system_prompt("Fill the section", "# <Add Title here>");

    assert!(prompt.contains(ENGLISH_BASE_SUMMARY_INSTRUCTION));
    assert!(prompt.contains("SECTION-SPECIFIC INSTRUCTIONS"));
}

#[test]
fn final_report_prompt_forbids_reasoning_output() {
    let prompt = build_final_report_system_prompt("Fill", "# Title");
    assert!(
        prompt.to_lowercase().contains("no reasoning")
            || prompt.contains("meta-commentary")
            || prompt.contains("self-correction")
    );
}

#[test]
fn chunk_prompt_forbids_reasoning_output() {
    let prompt = build_chunk_summary_user_prompt("x");
    assert!(
        prompt.contains("Do not include reasoning")
            || prompt.contains("meta-commentary")
            || prompt.contains("self-correction")
    );
}

#[test]
fn english_base_instruction_marks_non_english_prose_invalid_without_bloat() {
    assert!(ENGLISH_BASE_SUMMARY_INSTRUCTION.contains("non-English prose is invalid"));
    assert!(ENGLISH_BASE_SUMMARY_INSTRUCTION.len() <= 120);
}

#[test]
fn english_target_with_english_transcript_skips_normalization() {
    assert_eq!(
        resolve_final_language_action(Some("en"), Some("en")),
        FinalLanguageAction::ReturnEnglish
    );
}

#[test]
fn english_target_with_non_english_transcript_normalizes_to_english() {
    assert_eq!(
        resolve_final_language_action(Some("en"), Some("ja")),
        FinalLanguageAction::NormalizeEnglish
    );
}

#[test]
fn english_target_with_unknown_transcript_normalizes_to_english() {
    assert_eq!(
        resolve_final_language_action(Some("en"), None),
        FinalLanguageAction::NormalizeEnglish
    );
}

#[test]
fn non_english_target_uses_translation_flow() {
    assert_eq!(
        resolve_final_language_action(Some("fr"), Some("ja")),
        FinalLanguageAction::Translate("French")
    );
}

#[test]
fn normalization_fallback_preserves_markdown_and_observed_reasoning() {
    assert_eq!(
        english_markdown_after_normalization_result(
            "# Original",
            Ok(CleanedLlmMarkdown {
                markdown: String::new(),
                reasoning_stripped: true,
            }),
            None,
        )
        .unwrap(),
        (
            CleanedLlmMarkdown {
                markdown: "# Original".to_string(),
                reasoning_stripped: true,
            },
            true,
        )
    );
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();
    assert!(english_markdown_after_normalization_result(
        "# Original",
        Err("Summary generation was cancelled".to_string()),
        Some(&cancellation_token),
    )
    .is_err());
}

#[test]
fn chunk_retries_once_unless_cancelled() {
    assert!(should_retry_chunk_failure(1, None));
    assert!(!should_retry_chunk_failure(2, None));
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();
    assert!(!should_retry_chunk_failure(1, Some(&cancellation_token)));
}

// resolve_cached_english matrix -------------------------------------------

#[test]
fn no_cache_no_language_returns_none() {
    assert_eq!(resolve_cached_english(None, None), None);
}

#[test]
fn empty_cache_with_translation_target_returns_none() {
    assert_eq!(resolve_cached_english(Some(""), Some("fr")), None);
}

#[test]
fn whitespace_only_cache_returns_none() {
    assert_eq!(resolve_cached_english(Some("   \n"), Some("fr")), None);
}

#[test]
fn valid_cache_no_language_returns_none() {
    assert_eq!(resolve_cached_english(Some("body"), None), None);
}

#[test]
fn valid_cache_english_target_returns_none() {
    assert_eq!(resolve_cached_english(Some("body"), Some("en")), None);
}

#[test]
fn valid_cache_english_variant_returns_none() {
    // "en-GB" normalises to English — cache should not be used (re-run pass 1)
    assert_eq!(resolve_cached_english(Some("body"), Some("en-GB")), None);
}

#[test]
fn valid_cache_french_target_returns_cache() {
    assert_eq!(
        resolve_cached_english(Some("body"), Some("fr")),
        Some("body")
    );
}

#[test]
fn valid_cache_unknown_language_returns_none() {
    // Unknown code -> language_name_from_code returns None -> not a translation
    assert_eq!(
        resolve_cached_english(Some("body"), Some("zz-unknown")),
        None
    );
}

#[test]
fn uppercase_translation_code_returns_cache() {
    assert_eq!(
        resolve_cached_english(Some("body"), Some("FR")),
        Some("body")
    );
}

#[test]
fn uppercase_english_code_returns_none() {
    assert_eq!(resolve_cached_english(Some("body"), Some("EN")), None);
}

#[test]
fn underscore_locale_variant_returns_none() {
    // OS locale APIs (notably macOS) may emit "en_GB" with underscore.
    assert_eq!(resolve_cached_english(Some("body"), Some("en_GB")), None);
}

#[test]
fn token_estimate_counts_digits_and_brackets_as_tokens() {
    // Qwen 3.5 splits this prefix into about 16 tokens.
    assert!(rough_token_count("[S123] [12:34] [Lena] ") >= 16);
    assert_eq!(rough_token_count("abc"), 1);
}

#[test]
fn cleaner_removes_closed_reasoning_envelopes_everywhere() {
    let cleaned = clean_llm_markdown_detailed(
        "Intro\n<think>private</think>\n# Meeting\nHello\n<thinking>also private</thinking>\nTail",
    );
    assert!(cleaned.reasoning_stripped);
    assert!(!cleaned.markdown.contains("<think"));
    assert!(!cleaned.markdown.contains("<thinking"));
    assert!(cleaned.markdown.contains("Intro"));
    assert!(cleaned.markdown.contains("# Meeting"));
    assert!(cleaned.markdown.contains("Tail"));

    let fenced = clean_llm_markdown_detailed("```\n<think>private</think>\nvisible\n```");
    assert_eq!(fenced.markdown, "visible");
    assert!(fenced.reasoning_stripped);

    let gemma4 = clean_llm_markdown_detailed("<|channel>thought\nprivate<channel|># Meeting\nTail");
    assert!(gemma4.reasoning_stripped);
    assert_eq!(gemma4.markdown, "# Meeting\nTail");

    let attributed = clean_llm_markdown_detailed(
        "Visible\n<thinking class=\"internal\">private</thinking>\nTail",
    );
    assert!(attributed.reasoning_stripped);
    assert!(!attributed.markdown.contains("private"));
    assert!(attributed.markdown.contains("Visible"));
    assert!(attributed.markdown.contains("Tail"));

    let literal = clean_llm_markdown_detailed("<thinker>Visible</thinker>");
    assert!(!literal.reasoning_stripped);
    assert_eq!(literal.markdown, "<thinker>Visible</thinker>");
}

#[test]
fn cleaner_rejects_unterminated_reasoning_markers() {
    for raw in [
        "Visible\n<think>private",
        "Visible\n</thinking>",
        "Visible\n<think class=\"internal\">private",
    ] {
        let cleaned = clean_llm_markdown_detailed(raw);
        assert_eq!(
            require_visible_markdown("Final summary", &cleaned),
            Err("Final summary contained an unterminated reasoning marker".to_string())
        );
    }
}

#[test]
fn reasoning_only_and_empty_fences_fail_visible_content_guard() {
    let reasoning_only = clean_llm_markdown_detailed("<think>private</think>");
    assert_eq!(
        require_visible_markdown("Final summary", &reasoning_only),
        Err(
            "Final summary returned no visible summary content after reasoning removal".to_string()
        )
    );
    for raw in [
        "```\n```",
        "```markdown\n```",
        "```\r\n```",
        "```markdown\r\n```",
    ] {
        let empty_fence = clean_llm_markdown_detailed(raw);
        assert_eq!(empty_fence.markdown, "");
        assert_eq!(
            require_visible_markdown("Translation", &empty_fence),
            Err(
                "Translation returned no visible summary content after reasoning removal"
                    .to_string()
            )
        );
    }
}

#[test]
fn cleaner_strips_outer_crlf_fence_without_normalizing_inner_markdown() {
    let markdown = "# Heading\r\n\r\n```rust\r\nlet x = 1;\r\n```";
    let wrapped = format!("```\r\n{markdown}\r\n```");
    let cleaned = clean_llm_markdown_detailed(&wrapped);
    assert_eq!(cleaned.markdown, markdown);
    assert!(!cleaned.reasoning_stripped);
    assert_eq!(clean_llm_markdown_detailed(markdown).markdown, markdown);
}
