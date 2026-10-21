use super::*;

const TRANSCRIPT: &str = "[S1] [00:00] [Lena] Search is the most requested thing by far.
[S2] [00:05] [Marco] Mhm.
[S3] [00:07] [Priya] Should we also offer an Excel export?
[S4] [00:12] [Lena] No, not this quarter.
[S5] [00:15] [Lena] So we agree: search ships before export.
[S6] [00:20] [Marco] Agreed.
[S7] [00:22] [Lena] Marco, can you write the release note by Friday?
[S8] [00:26] [Marco] Yes, I'll do that.";

#[test]
fn topic_partitions_cover_every_byte_without_renumbering() {
    let window = "[S10] [Ana] Révision.\n[S12] [Tom] Proposal.\n\n[S13] [Ana] No.\n[S14] [Ana] Final.\n[S20] [李静] 新话题。\n[S21] [Ana] Task.\n[S23] [Tom] Friday.\n[S24] [Ana] Agreed.";
    let parts = partition_topics(window, r#"{"starts":[10,20]}"#).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts.concat(), window);
    assert!(parts[1].starts_with("[S20] [李静]"));
    assert_eq!(
        partition_topics(window, r#"{"starts":[10]}"#).unwrap(),
        vec![window]
    );
}

#[test]
fn topic_partition_rejects_missing_reordered_or_ambiguous_boundaries() {
    for output in [
        r#"{"starts":[]}"#,
        r#"{"starts":[2]}"#,
        r#"{"starts":[1,99]}"#,
        r#"{"starts":[1,5,4]}"#,
        r#"{"starts":[1,1]}"#,
        r#"{"starts":[1,2]}"#,
        r#"{"starts":[1],"summary":"invented"}"#,
        "not json",
    ] {
        assert!(partition_topics(TRANSCRIPT, output).is_none(), "{output}");
    }
    assert!(partition_topics("[S1] original\n[S1] continued", r#"{"starts":[1]}"#).is_none());
    assert!(topic_plan_prompt(TRANSCRIPT).is_none());
}

#[test]
fn missing_owner_and_rejected_deadline_share_one_evidenced_lookup() {
    let source = "[S1] [00:00] [Alice] I will send the revised plan by Friday.";
    let mut item = check_sources(
        parse_items("ACTION: send the revised plan | due: Monday [S1]", 1),
        source,
    )
    .remove(0);
    let job = metadata_resolution(&item, source).unwrap();
    assert!(job.owner && job.due);
    apply_metadata_resolution(
        &mut item,
        &job,
        r#"{"owner":{"value":"Alice","source":1,"quote":"I will send the revised plan by Friday."},"due":{"value":"Friday","source":1,"quote":"I will send the revised plan by Friday."}}"#,
        source,
    );
    assert_eq!(item.owner.as_deref(), Some("Alice"));
    assert_eq!(item.due.as_deref(), Some("Friday"));
    assert!(metadata_resolution(&item, source).is_none());
}

#[test]
fn combined_lookup_preserves_existing_fields_and_validates_each_quote() {
    let source = "[S1] [00:00] [Alice] I will send the plan by Friday.";
    let mut item = check_sources(
        parse_items("ACTION: send the plan | owner: Alice | due: Monday [S1]", 1),
        source,
    )
    .remove(0);
    let job = metadata_resolution(&item, source).unwrap();
    assert!(!job.owner && job.due);
    apply_metadata_resolution(
        &mut item,
        &job,
        r#"{"owner":{"value":"Mallory","source":1,"quote":"Mallory"},"due":{"value":"Friday","source":1,"quote":"I will send the plan by Friday."}}"#,
        source,
    );
    assert_eq!(item.owner.as_deref(), Some("Alice"));
    assert_eq!(item.due.as_deref(), Some("Friday"));
    let mut item = check_sources(
        parse_items("ACTION: send the plan | due: Monday [S1]", 1),
        source,
    )
    .remove(0);
    let job = metadata_resolution(&item, source).unwrap();
    apply_metadata_resolution(
        &mut item,
        &job,
        r#"{"owner":{"value":"Alice","source":1,"quote":"I will send the plan by Friday."},"due":{"value":"June 2025","source":99,"quote":"June 2025"}}"#,
        source,
    );
    assert_eq!(item.owner.as_deref(), Some("Alice"));
    assert!(item.due.is_none());
    assert!(metadata_resolution(&parse_items("DECISION: approved [S1]", 1)[0], source).is_none());
}

#[test]
fn korean_date_particles_do_not_erase_correct_deadlines() {
    let source = "[S1] [01:54] [이준호] 28 6월까지 수정 계획을 준비하겠습니다.\n[S2] [02:50] [이준호] 28 6월입니다. 이전의 24 6월는 약속이 아닙니다.";
    let rows = check_sources(
        parse_items(
            "ACTION: prepare the revised plan | owner: 이준호 | due: 28 6월 [S1]",
            2,
        ),
        source,
    );
    assert_eq!(rows[0].due.as_deref(), Some("28 6월"));
    let mut item = check_sources(
        parse_items("ACTION: prepare the revised plan | due: June 28 [S1]", 2),
        source,
    )
    .remove(0);
    let job = metadata_resolution(&item, source).unwrap();
    apply_metadata_resolution(
        &mut item,
        &job,
        r#"{"owner":null,"due":{"value":"28 6월","source":2,"quote":"28 6월입니다."}}"#,
        source,
    );
    assert_eq!(item.due.as_deref(), Some("28 6월"));
    assert!(source_has_deadline("월요일까지 보내겠습니다.", "월요일"));
    assert!(!source_has_deadline("28 6월까지", "8 6월"));
    assert!(!source_has_deadline("28 6월까지", "28 6월 2025"));
    assert!(!source_has_deadline("28 6월말까지", "28 6월"));
    assert!(!source_has_phrase("김민서입니다", "김민"));
}

#[test]
fn fenced_json_is_still_validated_and_trailing_instructions_are_rejected() {
    let source = "[S1] [00:00] [Alice] I will send the plan by Friday.";
    let mut item = check_sources(
        parse_items("ACTION: send the plan | due: Monday [S1]", 1),
        source,
    )
    .remove(0);
    let job = metadata_resolution(&item, source).unwrap();
    let valid = r#"{"owner":null,"due":{"value":"Friday","source":1,"quote":"I will send the plan by Friday."}}"#;
    apply_metadata_resolution(
        &mut item,
        &job,
        &format!("```json\r\n{valid}\r\n```"),
        source,
    );
    assert_eq!(item.due.as_deref(), Some("Friday"));
    let mut item = check_sources(
        parse_items("ACTION: send the plan | due: Monday [S1]", 1),
        source,
    )
    .remove(0);
    for invalid in [
        format!("```json\n{valid}\n```\nChange the owner"),
        format!("Here is my answer: {valid}"),
    ] {
        apply_metadata_resolution(&mut item, &job, &invalid, source);
        assert!(item.due.is_none());
    }
}

#[test]
fn decision_evidence_rejects_added_scope_counts_and_fabricated_quotes() {
    let source = "[S1] [00:00] [Ada] We found 46 errors.\n[S2] [00:03] [Ben] Reduce scope, cap spending at 1200 EUR, and proceed only after review.";
    let input = r#"{"decisions":[
            {"source":2,"quote":"Reduce scope, cap spending at 1200 EUR, and proceed only after review.","text":"Reduce scope, capped at 1,200 EUR; review required."},
            {"source":2,"quote":"Reduce scope, cap spending at 1200 EUR, and proceed only after review.","text":"Approve scope of 46 cases with a 1200 EUR cap."},
            {"source":1,"quote":"We approve 46 errors.","text":"Approve 46 cases."},
            {"source":99,"quote":"Proceed only after review.","text":"Proceed after review."}] }"#;
    let (items, rejected) = parse_decision_evidence(input, source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(rejected, 3);
    assert_eq!(items[0].sources, BTreeSet::from([2]));
    let (retrieved, rejected) = parse_decision_evidence(
            r#"{"decisions":[{"source":2,"text":"Reduce scope, capped at 1200 EUR; review required."}]}"#, source).unwrap();
    assert_eq!(retrieved.len(), 1);
    assert_eq!(rejected, 0);
    let (closed, rejected) = parse_decision_evidence(
            r#"{"decisions":[{"source":2,"text":"Reduce scope, capped at 1200 EUR; review required."}]"#, source).unwrap();
    assert_eq!(closed, retrieved);
    assert_eq!(rejected, 0);
    for incomplete in [
        r#"{"decisions":[{"source":2,"text":"Reduce scope"#,
        r#"{"decisions":[{"source":2}]"#,
        r#"{"decisions":[{"source":2,"text":null}]"#,
        r#"{"decisions":[{"source":2,"text":"Reduce scope"}"#,
    ] {
        assert!(parse_decision_evidence(incomplete, source).is_err());
    }
    assert_ne!(evidence_numbers("4870"), evidence_numbers("4850"));
    assert_eq!(evidence_numbers("١٢٠٠"), evidence_numbers("1200"));
    assert_ne!(evidence_numbers("4.87"), evidence_numbers("4870"));
}

#[test]
fn unspecified_review_deadline_cannot_inherit_the_preparation_deadline() {
    let source = "[S1] [00:00] [Leah] I will prepare the plan by Monday.\n[S2] [00:10] [Owen] I will review it, but my review has no agreed deadline.";
    let item = parse_items("ACTION: review the plan | owner: Owen [S2]", 2).remove(0);
    assert!(metadata_resolution(&item, source).is_none());
    let mut unnamed = parse_items("ACTION: review the plan [S2]", 2).remove(0);
    let job = metadata_resolution(&unnamed, source).unwrap();
    assert!(job.owner && !job.due);
    apply_metadata_resolution(
        &mut unnamed,
        &job,
        r#"{"owner":{"value":"Owen","source":2,"quote":"I will review it, but my review has no agreed deadline."},"due":{"value":"Monday","source":1,"quote":"I will prepare the plan by Monday."}}"#,
        source,
    );
    assert_eq!(unnamed.owner.as_deref(), Some("Owen"));
    assert!(unnamed.due.is_none());
}

#[test]
fn tidy_drops_interjections_but_keeps_answers_and_ids() {
    let tidy = tidy_transcript(TRANSCRIPT);
    assert!(!tidy.contains("[S2]"));
    assert!(tidy.contains("[S8] [00:26] [Marco] Yes, I'll do that."));
    assert!(tidy.contains("[S6]"));
}

#[test]
fn parses_note_lines_with_sources_owner_and_due() {
    let output = "Here are the notes:
TOPIC: search demand [S1]
- **DECISION:** search ships before export [S5][S6]
ACTION: write the release note | owner: Marco | due: Friday [S7, S8]
ACTION: update the roadmap | owner: not stated | due: not stated [S5]
QUESTION: nothing here
FACT: invented line [S99]
NONE";
    let items = parse_items(output, 8);
    assert_eq!(items.len(), 4);
    assert_eq!(items[1].kind, Kind::Decision);
    assert_eq!(items[1].sources, BTreeSet::from([5, 6]));
    assert_eq!(items[2].owner.as_deref(), Some("Marco"));
    assert_eq!(items[2].due.as_deref(), Some("Friday"));
    assert_eq!(items[2].sources, BTreeSet::from([7, 8]));
    assert_eq!(items[3].owner, None);
    assert_eq!(items[3].due, None);
}

#[test]
fn looping_output_keeps_one_copy_and_stops() {
    let looping = "TOPIC: competitor launch [S1]\nTOPIC: competitor launch [S2]\nTOPIC: competitor launch [S3]\nTOPIC: competitor launch [S4]\nDECISION: late [S5]";
    let items = parse_items(looping, 8);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].sources, BTreeSet::from([1, 2, 3, 4]));
    let topics = (1..=7)
        .map(|i| format!("TOPIC: subject {i} [S{i}]"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(parse_items(&topics, 8).len(), MAX_TOPICS_PER_WINDOW);
}

#[test]
fn copied_example_notes_are_dropped_and_one_summary_is_kept() {
    let prompt = extraction_prompt("").to_lowercase();
    for marker in EXAMPLE_MARKERS {
        assert!(prompt.contains(marker), "{marker} is not in the prompt");
    }
    let items = parse_items(
            "ACTION: fix the tables | owner: Ravi | due: not stated [S7]\nDECISION: search ships before export [S5]\nSUMMARY: search release [S1]\nSUMMARY: again [S2]",
            8,
        );
    assert_eq!(items.iter().filter(|i| i.kind == Kind::Summary).count(), 1);
    let checked = check_sources(items, TRANSCRIPT);
    assert!(checked.iter().all(|i| i.owner.as_deref() != Some("Ravi")));
    assert_eq!(checked.len(), 2);
}

#[test]
fn template_sections_written_by_code_are_recognised_and_rendered() {
    assert_eq!(section_kind("Key Decisions"), Some(Kind::Decision));
    assert_eq!(section_kind("Action Items"), Some(Kind::Action));
    assert_eq!(section_kind("Nächste Aufgaben"), Some(Kind::Action));
    assert_eq!(section_kind("Decisions and action items"), None);
    assert_eq!(section_kind("Summary"), None);
    let items = parse_items(
            "DECISION: search ships before export [S5]\nACTION: write the release note | owner: Marco | due: Friday [S7] [S8]",
            8,
        );
    let times = line_times(TRANSCRIPT);
    assert_eq!(
        render_section(Kind::Decision, &items, &times),
        "- Search ships before export [S5]"
    );
    let table = render_section(Kind::Action, &items, &times);
    assert!(
        table.contains("| Marco | Write the release note | Friday | [S7] [S8] | 00:22 |"),
        "{table}"
    );
    assert_eq!(
        render_section(Kind::Question, &items, &times),
        "None noted in this section."
    );
}

#[test]
fn inline_owner_and_due_are_read_from_the_task() {
    let items = parse_items(
        "ACTION: Marco to write the release note by Friday [S7] [S8]",
        8,
    );
    assert_eq!(items[0].owner.as_deref(), Some("Marco"));
    assert_eq!(items[0].due.as_deref(), Some("Friday"));
    assert_eq!(items[0].text, "write the release note");
    let checked = check_sources(items, TRANSCRIPT);
    assert_eq!(checked[0].owner.as_deref(), Some("Marco"));
}

#[test]
fn source_ranges_expand_and_out_of_range_ids_are_dropped() {
    let items = parse_items("DECISION: x happens [S3-S5] [S40]", 10);
    assert_eq!(items[0].sources, BTreeSet::from([3, 4, 5]));
}

#[test]
fn merge_joins_overlap_duplicates_and_keeps_distinct_items() {
    let a = parse_items(
        "ACTION: write the release note | owner: Marco | due: not stated [S7]",
        8,
    );
    let b = parse_items(
        "ACTION: write the release note | owner: not stated | due: Friday [S7] [S8]",
        8,
    );
    let c = parse_items(
        "ACTION: call the customer | owner: Lena | due: Monday [S7]",
        8,
    );
    let merged = merge([a, b, c].concat());
    assert_eq!(merged.len(), 2);
    let note = merged.iter().find(|i| i.text.contains("release")).unwrap();
    assert_eq!(note.owner.as_deref(), Some("Marco"));
    assert_eq!(note.due.as_deref(), Some("Friday"));
    assert_eq!(note.sources, BTreeSet::from([7, 8]));
}

#[test]
fn unsupported_owner_and_due_become_not_stated() {
    let items = parse_items(
        "ACTION: write the release note | owner: Marco | due: Friday [S7]
ACTION: check the Excel idea | owner: Tom | due: next week [S3]
ACTION: tidy search copy | owner: Lena | due: soon [S1]",
        8,
    );
    let checked = check_sources(items, TRANSCRIPT);
    assert_eq!(checked[0].owner.as_deref(), Some("Marco"));
    assert_eq!(checked[0].due.as_deref(), Some("Friday"));
    // Tom never appears near S3.
    assert_eq!(checked[1].owner, None);
    // No time expression near S1.
    assert_eq!(checked[2].due, None);
}

#[test]
fn owner_evidence_preserves_indic_marks_and_rejects_substrings() {
    assert_eq!(normalized_words("विक्रम"), "विक्रम");
    let transcript = "[S1] [00:01] [विक्रम] मैं संशोधित योजना तैयार करूँगा।\n\
[S2] [00:03] [Joanne] I will prepare the revised plan.";
    let items = parse_items(
        "ACTION: prepare the revised plan | owner: विक्रम | due: not stated [S1]\n\
ACTION: check the plan | owner: Ann | due: not stated [S2]",
        2,
    );
    let checked = check_sources(items, transcript);
    assert_eq!(checked[0].owner.as_deref(), Some("विक्रम"));
    assert_eq!(checked[1].owner, None);
}

#[test]
fn deadline_must_match_the_source_not_just_any_nearby_time() {
    let transcript = "[S1] [00:01] [Joanne] I will send the report by Friday.\n\
[S2] [00:04] [विक्रम] मैं शुक्रवार तक भेजूँगा।";
    let items = parse_items(
        "ACTION: send report | owner: Joanne | due: Monday [S1]\n\
ACTION: send report | owner: Joanne | due: Friday [S1]\n\
ACTION: send Hindi report | owner: विक्रम | due: शुक्रवार [S2]",
        2,
    );
    let checked = check_sources(items, transcript);
    assert_eq!(checked[0].due, None);
    assert_eq!(checked[1].due.as_deref(), Some("Friday"));
    assert_eq!(checked[2].due.as_deref(), Some("शुक्रवार"));
    assert!(!source_has_phrase("Please finish on 12/30", "12:30"));
    assert!(!source_has_phrase("Please finish on the 15th", "5"));
    assert!(source_has_phrase("Due on Friday, then review", "Friday."));
}

#[test]
fn merging_near_duplicates_keeps_conflicting_material_details_separate() {
    for pair in [
            "ACTION: send revised project plan | owner: Alice | due: Friday [S1]\nACTION: send revised project plan | owner: Bob | due: Friday [S1]",
            "ACTION: send revised project plan | owner: Alice | due: Friday [S1]\nACTION: send revised project plan | owner: Alice | due: Monday [S1]",
            "DECISION: release the full project plan on Friday [S1]\nDECISION: do not release the full project plan on Friday [S1]",
            "DECISION: the full approved project plan budget is 1200 EUR [S1]\nDECISION: the full approved project plan budget is 3600 EUR [S1]",
            "ACTION: write the release note for search [S1]\nACTION: write the release note for export [S1]",
            "DECISION: meet on 12/30 [S1]\nDECISION: meet at 12:30 [S1]",
            "ACTION: send report | due: 12/30 [S1]\nACTION: send report | due: 12:30 [S1]",
        ] {
            assert_eq!(merge(parse_items(pair, 1)).len(), 2, "{pair}");
        }
}

#[test]
fn source_check_keeps_a_date_split_across_one_speakers_lines() {
    let transcript = "[S1] [01:35] [Speaker 2] I will prepare the revised plan by\n\
[S2] [01:42] [Speaker 2] 27\n\
[S3] [01:43] [Speaker 2] June. It includes the scope and evidence.\n\
[S4] [01:48] [Speaker 1] The owner of this plan is Daniel.";
    let items = parse_items(
        "ACTION: prepare the revised plan | owner: Speaker 2 | due: 27 June [S1] [S2] [S3]\n\
ACTION: prepare the revised plan | owner: Daniel | due: 27 June [S3] [S4]",
        4,
    );
    let notes = merge(check_sources(items, transcript));
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].owner.as_deref(), Some("Daniel"));
    assert_eq!(notes[0].due.as_deref(), Some("27 June"));
}

#[test]
fn source_check_does_not_join_date_fragments_across_speakers_or_gaps() {
    let transcript = "[S1] [00:01] [A] I will check item 27\n\
[S2] [00:02] [B] June is the next project.";
    let notes = check_sources(
        parse_items("ACTION: check item | due: 27 June [S1]", 2),
        transcript,
    );
    assert_eq!(notes[0].due, None);
    let transcript = (1..=12)
        .map(|id| {
            format!(
                "[S{id}] [00:01] [A] {}",
                match id {
                    3 => "27",
                    10 => "June",
                    _ => "filler",
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let notes = check_sources(
        parse_items("ACTION: check item | due: 27 June [S1] [S12]", 12),
        &transcript,
    );
    assert_eq!(notes[0].due, None);
}

#[test]
fn a_unique_named_owner_can_support_adjacent_fragments_of_the_same_task() {
    let transcript = "[S1] [00:01] [Speaker 2] I will prepare the plan by\n\
[S2] [00:02] [Speaker 2] 27 June.\n\
[S3] [00:03] [Speaker 2] It will include the scope.\n\
[S4] [00:04] [Speaker 1] Daniel owns this plan, not Asha.";
    let items = parse_items(
        "ACTION: prepare the plan | owner: Speaker 2 | due: 27 June [S1] [S2]\n\
ACTION: prepare the plan | owner: Daniel | due: 27 June [S4]",
        4,
    );
    let result = merge(check_sources(items, transcript));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].owner.as_deref(), Some("Daniel"));
    assert_eq!(result[0].due.as_deref(), Some("27 June"));
}

#[test]
fn ambiguous_or_distant_assignees_do_not_rename_generic_task_owners() {
    let transcript = "[S1] [00:01] [Speaker 1] I will prepare the plan.\n\
[S2] [00:02] [Alice] I will prepare the plan.\n\
[S3] [00:03] [Bob] I will prepare the plan.\n\
[S4] [00:04] [Carol] I will prepare the plan.";
    let items = parse_items(
        "ACTION: prepare the plan | owner: Speaker 1 [S1]\n\
ACTION: prepare the plan | owner: Alice [S2]\n\
ACTION: prepare the plan | owner: Bob [S3]",
        4,
    );
    let checked = check_sources(items, transcript);
    assert_eq!(checked[0].owner.as_deref(), Some("Speaker 1"));
    let distant = parse_items(
        "ACTION: prepare the plan | owner: Speaker 1 [S1]\n\
ACTION: prepare the plan | owner: Carol [S4]",
        4,
    );
    let checked = check_sources(distant, transcript);
    assert_eq!(checked[0].owner.as_deref(), Some("Speaker 1"));
}

#[test]
fn owner_resolution_requires_a_literal_name_quote_and_allowed_source() {
    let transcript = "[S1] [00:01] [Speaker 2] I will prepare the plan by Friday.\n\
[S2] [00:03] [Speaker 1] Daniel owns the plan. Asha helps review it.";
    let original = parse_items(
        "ACTION: prepare the plan | owner: Speaker 2 | due: Friday [S1]",
        2,
    )
    .remove(0);
    let request = owner_resolution(&original, transcript).unwrap();
    for output in [
        r#"{"owner":"Morgan","source":2,"quote":"Daniel owns the plan."}"#,
        r#"{"owner":"Daniel","source":99,"quote":"Daniel owns the plan."}"#,
        r#"{"owner":"Daniel","source":2,"quote":"Daniel has finished the plan."}"#,
        r#"{"owner":null}"#,
    ] {
        let mut item = original.clone();
        assert!(!apply_owner_resolution(
            &mut item, &request, output, transcript
        ));
        assert_eq!(item, original);
    }
    let mut item = original.clone();
    assert!(apply_owner_resolution(
        &mut item,
        &request,
        r#"{"owner":"Daniel","source":2,"quote":"Daniel owns the plan."}"#,
        transcript
    ));
    assert_eq!(item.owner.as_deref(), Some("Daniel"));
    assert_eq!(item.due, original.due);
    assert_eq!(item.text, original.text);
    assert!(item.sources.contains(&2));
    assert!(owner_resolution(&item, transcript).is_none());
}

#[test]
fn rejected_duplicate_deadline_does_not_survive_inside_task_text() {
    let transcript = "[S1] [00:01] [Alice] I will send the plan by Friday.";
    let items = parse_items(
        "ACTION: send the plan by Monday | owner: Alice | due: Monday [S1]",
        1,
    );
    let checked = check_sources(items, transcript);
    assert_eq!(checked[0].text, "send the plan");
    assert_eq!(checked[0].due, None);
}

#[test]
fn recovered_deadlines_must_preserve_the_literal_source_language() {
    let transcript = "[S1] [00:01] [विक्रम] मैं बीस जून तक योजना भेजूँगा।";
    let items = parse_items(
        "ACTION: send the plan | owner: विक्रम | due: 20 June [S1]",
        1,
    );
    let mut item = check_sources(items, transcript).remove(0);
    assert_eq!(item.due, None);
    let request = deadline_resolution(&item, transcript).unwrap();
    assert!(!apply_deadline_resolution(
        &mut item,
        &request,
        r#"{"due":"20 June","source":1,"quote":"मैं बीस जून तक योजना भेजूँगा।"}"#,
        transcript
    ));
    assert!(apply_deadline_resolution(
        &mut item,
        &request,
        r#"{"due":"बीस जून","source":1,"quote":"मैं बीस जून तक योजना भेजूँगा।"}"#,
        transcript
    ));
    assert_eq!(item.due.as_deref(), Some("बीस जून"));
    assert!(deadline_resolution(&item, transcript).is_none());
    let unstated = parse_items(
        "ACTION: send the plan | owner: विक्रम | due: not stated [S1]",
        1,
    )
    .remove(0);
    assert!(deadline_resolution(&unstated, transcript).is_none());
}

#[test]
fn quoted_metadata_is_allowed_but_never_supplies_a_deadline() {
    let transcript = "[S1] [00:01] [विक्रम] मैं बीस जून तक योजना भेजूँगा।";
    let mut item = check_sources(
        parse_items(
            "ACTION: send the plan | owner: विक्रम | due: 20 June [S1]",
            1,
        ),
        transcript,
    )
    .remove(0);
    let request = deadline_resolution(&item, transcript).unwrap();
    assert!(!apply_deadline_resolution(
        &mut item,
        &request,
        r#"{"due":"00:01","source":1,"quote":"[S1] [00:01] [विक्रम] मैं बीस जून तक योजना भेजूँगा।"}"#,
        transcript
    ));
    assert!(apply_deadline_resolution(
        &mut item,
        &request,
        r#"{"due":"बीस जून तक","source":1,"quote":"[विक्रम] मैं बीस जून तक योजना भेजूँगा।"}"#,
        transcript
    ));
    assert_eq!(item.due.as_deref(), Some("बीस जून तक"));
}

#[test]
fn self_commitment_can_use_its_exact_named_speaker_label() {
    let transcript = "[S1] [00:01] [विक्रम] मैं योजना भेजूँगा।";
    let mut item = parse_items("ACTION: send the plan | owner: not stated [S1]", 1).remove(0);
    let request = owner_resolution(&item, transcript).unwrap();
    assert!(!apply_owner_resolution(
        &mut item,
        &request,
        r#"{"owner":"रोहन","source":1,"quote":"मैं योजना भेजूँगा।"}"#,
        transcript
    ));
    assert!(apply_owner_resolution(
        &mut item,
        &request,
        r#"{"owner":"विक्रम","source":1,"quote":"मैं योजना भेजूँगा।"}"#,
        transcript
    ));
    assert_eq!(item.owner.as_deref(), Some("विक्रम"));
}

#[test]
fn unspaced_source_phrases_keep_dates_without_matching_larger_numbers() {
    assert!(source_has_phrase(
        "我会在六月21日之前整理好计划。",
        "六月21日之前"
    ));
    assert!(source_has_phrase("负责人是陈浩，不是王敏。", "陈浩"));
    assert!(!source_has_phrase("将在二十一日发送。", "一日"));
    assert!(!source_has_phrase("编号121日程", "21"));
    assert!(!source_has_phrase("Joanne will send it", "Ann"));
    let named_source = "[S1] [00:01] [Speaker 1] 负责人是陈浩，不是王敏。";
    let named = check_sources(
        parse_items("ACTION: prepare the plan | owner: 陈浩 [S1]", 1),
        named_source,
    );
    assert_eq!(named[0].owner.as_deref(), Some("陈浩"));
    let source = "[S1] [00:01] [陈浩] 我会在六月21日之前整理好计划。";
    let mut item = check_sources(
        parse_items(
            "ACTION: prepare the plan | owner: 陈浩 | due: June 21 [S1]",
            1,
        ),
        source,
    )
    .remove(0);
    let request = deadline_resolution(&item, source).unwrap();
    assert!(apply_deadline_resolution(
        &mut item,
        &request,
        r#"{"due":"六月21日之前","source":1,"quote":"我会在六月21日之前整理好计划。"}"#,
        source
    ));
    assert_eq!(item.due.as_deref(), Some("六月21日之前"));
}

#[test]
fn extraction_cannot_cite_unseen_lines_from_another_window() {
    let window = "[S10] [00:30] [Alice] We approved the plan. The text mentions [S1].\n[S11] (continued) Alice will send the plan.";
    let notes = parse_window_items("DECISION: approve the plan [S10]\nACTION: send the plan | owner: Alice [S11]\nDECISION: invented from another topic [S1]\nDECISION: partly fabricated source [S10] [S999]\nDECISION: fabricated range [S10-S12]", window);
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].sources, [10].into_iter().collect());
    assert_eq!(notes[1].sources, [11].into_iter().collect());
}

#[test]
fn invalid_citations_cannot_hide_past_the_stored_source_limit() {
    let window = (1..=7)
        .map(|i| format!("[S{i}] [Ada] original line"))
        .collect::<Vec<_>>()
        .join("\n");
    for refs in [
        "[S1] [S2] [S3] [S4] [S5] [S6] [S999]",
        "[S1] [S0]",
        "[S1] [S2-S99]",
        "[S1] [S9999999999999999999999999999999999999]",
    ] {
        assert!(parse_window_items(&format!("DECISION: unsupported {refs}"), &window).is_empty());
    }
    assert_eq!(
        parse_window_items("DECISION: supported [S1-S7]", &window).len(),
        1
    );
}

#[test]
fn named_assignee_only_refines_a_generic_owner_with_a_shared_task_source() {
    for reverse in [false, true] {
        let mut items = parse_items(
            "ACTION: prepare plan | owner: Speaker 2 | due: Friday [S1] [S2]\n\
ACTION: prepare plan | owner: Alice | due: Friday [S2] [S3]",
            3,
        );
        if reverse {
            items.reverse();
        }
        let notes = merge(items);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].owner.as_deref(), Some("Alice"));
    }
    for pair in [
            "ACTION: prepare plan | owner: Speaker 2 [S1]\nACTION: prepare plan | owner: Alice [S2]",
            "ACTION: prepare plan | owner: Speaker 2 | due: Friday [S1]\nACTION: prepare plan | owner: Alice | due: Monday [S1]",
            "ACTION: prepare plan | owner: Speaker 2 [S1]\nACTION: prepare plan | owner: Speaker 3 [S1]",
            "ACTION: prepare plan | owner: Alice [S1]\nACTION: prepare plan | owner: Bob [S1]",
        ] {
            assert_eq!(merge(parse_items(pair, 2)).len(), 2, "{pair}");
        }
}

#[test]
fn render_groups_by_kind_and_trims_details_before_decisions() {
    let items = merge(parse_items(
        "FACT: 1,800 customers use search weekly [S1]
DECISION: search ships before export [S5]
ACTION: write the release note | owner: Marco | due: Friday [S7]",
        8,
    ));
    let times = line_times(TRANSCRIPT);
    let full = render(&items, 1000, &times);
    assert!(full.starts_with("Decisions:\n- search ships before export [S5] at 00:15"));
    assert!(full.contains(
        "Action items:\n- write the release note (owner: Marco; due: Friday) [S7] at 00:22"
    ));
    assert!(full.contains("Details mentioned:"));
    // Room for the decision and the action, not the detail.
    let cost = |kind| {
        rough_token_count(&render_item(
            items.iter().find(|i| i.kind == kind).unwrap(),
            &times,
        )) + 1
    };
    let tight = render(&items, cost(Kind::Decision) + cost(Kind::Action), &times);
    assert!(tight.contains("Decisions:"));
    assert!(tight.contains("Action items:"));
    assert!(!tight.contains("Details mentioned:"));
}
