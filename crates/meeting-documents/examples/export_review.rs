use overtalk_documents::{MeetingDocument, Passage};
fn main() {
    let path = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&path).unwrap();
    let mut notes = String::from("## Decisions\n\nWe will ship **search** on Friday, after the final review. [Source](https://example.org)\n\n| Owner | Action | Due |\n| --- | --- | --- |\n");
    for i in 0..80 {
        notes.push_str(&format!(
            "| Léna {i} | Review the Portuguese release notes and search results | Friday |\n"
        ));
    }
    notes.push_str("\n## Follow-up\n\n- Keep the meeting data local.\n- [x] Export reviewed.\n\n```text\nLocal data stays on your device.\n```\n");
    let doc = MeetingDocument {
        title: "Reunião: lançamento e próximos passos".into(),
        date: "14 October 2026".into(),
        notice: "Participants were notified at 10:02 (notice copied to chat)".into(),
        notes,
        passages: vec![Passage {
            time: "00:01:30".into(),
            speaker: "Léna".into(),
            text: "Ship the search feature after the review.".into(),
        }],
    };
    for format in ["pdf", "docx", "text"] {
        std::fs::write(
            std::path::Path::new(&path).join(format!("review.{format}")),
            doc.render(format).unwrap(),
        )
        .unwrap();
    }
}
