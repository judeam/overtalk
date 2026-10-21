//! Keep edits across late reads, overlapping saves, navigation and failures.
use std::collections::HashMap;
#[derive(Default)]
pub(crate) struct Drafts(HashMap<String, Draft>);
#[derive(Default)]
struct Draft {
    text: String,
    revision: u64,
    saved: u64,
    saving: bool,
}
impl Drafts {
    pub(crate) fn edit(&mut self, id: &str, text: String) {
        let d = self.0.entry(id.into()).or_default();
        d.text = text;
        d.revision += 1;
    }
    pub(crate) fn unsaved(&self, id: &str) -> Option<&str> {
        self.0
            .get(id)
            .filter(|d| d.revision > d.saved)
            .map(|d| d.text.as_str())
    }
    pub(crate) fn text(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(|d| d.text.as_str())
    }
    pub(crate) fn clear_saved(&mut self, id: &str) {
        if self
            .0
            .get(id)
            .is_some_and(|d| d.revision == d.saved && !d.saving)
        {
            self.0.remove(id);
        }
    }
    pub(crate) fn begin_save(&mut self, id: &str) -> Option<(u64, String)> {
        let d = self.0.get_mut(id)?;
        if d.saving || d.revision == d.saved {
            return None;
        }
        d.saving = true;
        Some((d.revision, d.text.clone()))
    }
    pub(crate) fn finish_save(&mut self, id: &str, revision: u64, success: bool) {
        if let Some(d) = self.0.get_mut(id) {
            d.saving = false;
            if success {
                d.saved = d.saved.max(revision);
            }
        }
    }
    pub(crate) fn dirty_ids(&self) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, d)| d.revision > d.saved)
            .map(|(id, _)| id.clone())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_save_preserves_newer_edits_and_failed_save_can_retry() {
        let mut d = Drafts::default();
        d.edit("a", "first".into());
        let (r, _) = d.begin_save("a").unwrap();
        d.edit("a", "second".into());
        d.finish_save("a", r, true);
        assert_eq!(d.unsaved("a"), Some("second"));
        let (r, _) = d.begin_save("a").unwrap();
        d.finish_save("a", r, false);
        assert_eq!(d.unsaved("a"), Some("second"));
        let (r, _) = d.begin_save("a").unwrap();
        d.finish_save("a", r, true);
        assert!(d.unsaved("a").is_none());
    }
    #[test]
    fn navigation_keeps_independent_drafts_and_coalesces_saves() {
        let mut d = Drafts::default();
        d.edit("a", "a".into());
        d.edit("b", "b".into());
        d.begin_save("a").unwrap();
        assert!(d.begin_save("a").is_none());
        assert_eq!(d.unsaved("b"), Some("b"));
        assert_eq!(d.dirty_ids().len(), 2);
    }
}
