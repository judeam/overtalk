//! Offline help. Chapter data is bundled, searchable, and never sent to a provider.
use super::*;
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Step {
    title: String,
    body: String,
}
#[derive(Deserialize)]
struct Lesson {
    id: String,
    group: String,
    title: String,
    summary: String,
    steps: Vec<Step>,
    #[serde(default)]
    tips: Vec<String>,
    #[serde(default)]
    keys: Vec<String>,
    action: String,
    action_label: String,
}
static LESSONS: LazyLock<Vec<Lesson>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/guide.json"))
        .expect("bundled guide must be valid")
});

fn matches(lesson: &Lesson, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || std::iter::once(lesson.title.as_str())
            .chain([lesson.group.as_str(), lesson.summary.as_str()])
            .chain(
                lesson
                    .steps
                    .iter()
                    .flat_map(|step| [step.title.as_str(), step.body.as_str()]),
            )
            .chain(lesson.tips.iter().chain(&lesson.keys).map(String::as_str))
            .any(|text| text.to_lowercase().contains(&query))
}

impl Controller {
    pub(super) fn guide_action(&mut self, ui: &AppWindow, action: &str, value: &str) {
        let st = ui.global::<State>();
        match action {
            "guide" => {
                if st.get_view() != 4 {
                    st.set_guide_return_view(st.get_view());
                }
                self.save_notes(ui);
                st.set_view(4);
                self.render_guide(ui);
            }
            "guide-close" => st.set_view(st.get_guide_return_view()),
            "guide-search" => {
                st.set_guide_query(value.into());
                if let Some(index) = LESSONS.iter().position(|l| matches(l, value)) {
                    st.set_guide_index(index as i32);
                }
                self.render_guide(ui);
            }
            "guide-chapter" => {
                if let Ok(index) = value.parse::<usize>() {
                    if index < LESSONS.len() {
                        st.set_guide_index(index as i32);
                        st.set_guide_query("".into());
                        self.render_guide(ui);
                    }
                }
            }
            "guide-read" => {
                if let Some(lesson) = LESSONS.get(st.get_guide_index() as usize) {
                    let mut read = st.get_guide_read().iter().collect::<Vec<_>>();
                    if !read.iter().any(|id| id == &lesson.id) {
                        read.push(lesson.id.as_str().into());
                        st.set_guide_read(model(read));
                        st.invoke_preferences_changed();
                    }
                    self.render_guide(ui);
                }
            }
            "guide-open" => {
                let Some(lesson) = LESSONS.get(st.get_guide_index() as usize) else {
                    return;
                };
                match lesson.action.as_str() {
                    "sound" => self.action(ui, "settings", "1"),
                    "settings" => self.action(ui, "settings", "0"),
                    "library" => self.action(ui, "navigate", "2"),
                    "recorder" => self.action(ui, "home", ""),
                    _ if !self.active.is_empty() => {
                        st.set_view(1);
                        self.action(
                            ui,
                            "meeting-tab",
                            match lesson.action.as_str() {
                                "notes" => "1",
                                "speakers" => "2",
                                _ => "0",
                            },
                        );
                    }
                    _ => {
                        self.action(ui, "home", "");
                        st.set_message(
                            "Open a saved meeting from the sidebar to try this feature.".into(),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn render_guide(&self, ui: &AppWindow) {
        let st = ui.global::<State>();
        let read: HashSet<String> = st
            .get_guide_read()
            .iter()
            .map(|id| id.to_string())
            .collect();
        let mut previous_group = "";
        let rows = LESSONS
            .iter()
            .enumerate()
            .filter(|(_, l)| matches(l, &st.get_guide_query()))
            .map(|(index, l)| {
                let starts_group = previous_group != l.group;
                previous_group = &l.group;
                crate::GuideChapter {
                    index: index as i32,
                    title: l.title.as_str().into(),
                    summary: l.summary.as_str().into(),
                    group: l.group.as_str().into(),
                    starts_group,
                    read: read.contains(&l.id),
                }
            })
            .collect();
        st.set_guide_chapters(model(rows));
        st.set_guide_titles(model(
            LESSONS.iter().map(|l| l.title.as_str().into()).collect(),
        ));
        st.set_guide_read_count(LESSONS.iter().filter(|l| read.contains(&l.id)).count() as i32);
        let lesson = &LESSONS[st.get_guide_index().clamp(0, LESSONS.len() as i32 - 1) as usize];
        st.set_guide_title(lesson.title.as_str().into());
        st.set_guide_summary(lesson.summary.as_str().into());
        st.set_guide_steps(model(
            lesson
                .steps
                .iter()
                .map(|step| crate::GuideStep {
                    title: step.title.as_str().into(),
                    body: step.body.as_str().into(),
                })
                .collect(),
        ));
        st.set_guide_tip(lesson.tips.join("\n").into());
        st.set_guide_keys(lesson.keys.join("\n").into());
        st.set_guide_action_label(lesson.action_label.as_str().into());
        st.set_guide_is_read(read.contains(&lesson.id));
        st.set_guide_revision(st.get_guide_revision().wrapping_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_bundled_chapter_has_unique_identity_and_supported_action() {
        assert_eq!(LESSONS.len(), 13);
        let ids: HashSet<_> = LESSONS.iter().map(|l| &l.id).collect();
        assert_eq!(ids.len(), LESSONS.len());
        for lesson in LESSONS.iter() {
            assert!(!lesson.steps.is_empty());
            assert!(matches!(
                lesson.action.as_str(),
                "sound" | "settings" | "library" | "recorder" | "meeting" | "notes" | "speakers"
            ));
        }
    }
    #[test]
    fn search_includes_instructions_and_is_case_insensitive() {
        assert!(matches(&LESSONS[0], " HEADPHONES "));
        assert!(matches(&LESSONS[7], "recovery"));
        assert!(matches(&LESSONS[10], "CTRL+K"));
        assert!(!matches(&LESSONS[0], "xyz-no-match"));
    }
}
