//! Deterministic malformed-input exercise for memory checkers, without libtest.
//! This is a bounded mutation corpus, not coverage-guided fuzzing.
use overtalk_documents::{append_docx, notes_markdown, read_docx, MeetingDocument, Passage};

fn main() {
    let document = MeetingDocument {
        title: "Synthetic audit — नमस्ते 日本語".into(),
        date: "2026-10-18".into(),
        notice: "Participants were notified at 10:02 (notice copied to chat)".into(),
        notes: "## Decisions\n\nKeep <private> & quoted text.\n\n| Item | Owner |\n| --- | --- |\n| Review | Alex |".into(),
        passages: vec![Passage {
            time: "00:00:01".into(),
            speaker: "Alex & Sam".into(),
            text: "A synthetic parser fixture; no real meeting content.".into(),
        }],
    };
    for format in ["markdown", "text", "json", "docx", "pdf"] {
        assert!(!document.render(format).unwrap().is_empty());
    }
    let word = document.render("docx").unwrap();
    assert!(read_docx(&word).unwrap().contains("Alex & Sam"));
    let updated = append_docx(&word, "Appendix", "Synthetic update <&>").unwrap();
    assert!(read_docx(&updated)
        .unwrap()
        .contains("Synthetic update <&>"));
    let mut state = 0x6a09e667f3bcc909u64;
    for case in 0..512 {
        let mut bytes = word.clone();
        for _ in 0..1 + case % 8 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = state as usize % bytes.len();
            bytes[index] ^= (state >> 32) as u8 | 1;
        }
        if case % 3 == 0 {
            bytes.truncate(state as usize % bytes.len());
        }
        if let Ok(text) = read_docx(&bytes) {
            assert!(text.len() <= 16 * 1024 * 1024);
        }
        let _ = append_docx(&bytes, "Appendix", "Synthetic update");
    }
    // Exercise structured document shape/recursion validation too.
    for depth in [1, 16, 64, 128] {
        let mut value = serde_json::json!("synthetic");
        for _ in 0..depth {
            value = serde_json::json!({"content":[value]});
        }
        let _ = notes_markdown(&value);
    }
    println!("Checked 512 mutated archives, five export formats and four nested documents");
}
