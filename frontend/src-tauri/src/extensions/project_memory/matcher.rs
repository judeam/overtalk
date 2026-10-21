//! Deterministic project scoring. No model is involved: every point comes from
//! a visible signal, so each decision can say why it was made, and a close call
//! between two projects is left to the person.
use super::signals::{count_phrase, has_phrase, normalise, Event, Own};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap};

/// Titles that say nothing about which project a meeting belongs to.
const GENERIC_TITLES: &[&str] = &[
    "call",
    "catch up",
    "check in",
    "imported audio",
    "meeting",
    "new meeting",
    "quick call",
    "recording",
    "sync",
    "untitled event",
    "untitled meeting",
];

struct ProjectInfo {
    id: String,
    name: String,
    phrase: String,
    vocabulary: Vec<(String, String)>,
    people: Vec<(String, String)>,
    titles: Vec<String>,
}

/// Everything the scorer needs, loaded once per decision batch.
pub struct Knowledge {
    projects: Vec<ProjectInfo>,
    /// (kind, value) → projects with that signal, and its readable label.
    rules: HashMap<(String, String), Vec<(String, String)>>,
    titles: Vec<(String, String)>,
}

impl Knowledge {
    pub async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let error = |e: sqlx::Error| e.to_string();
        let projects: Vec<(String, String)> =
            sqlx::query_as("SELECT id,name FROM projects WHERE archived=0 ORDER BY name")
                .fetch_all(pool)
                .await
                .map_err(error)?;
        let rules: Vec<(String, String, String, String)> = sqlx::query_as("SELECT r.project_id,r.kind,r.value,r.label FROM project_match_rules r JOIN projects p ON p.id=r.project_id WHERE r.enabled=1 AND p.archived=0")
            .fetch_all(pool).await.map_err(error)?;
        let terms: Vec<(String, String)> =
            sqlx::query_as("SELECT project_id,term FROM project_vocabulary")
                .fetch_all(pool)
                .await
                .map_err(error)?;
        let people: Vec<(String, String)> =
            sqlx::query_as("SELECT project_id,name FROM project_people")
                .fetch_all(pool)
                .await
                .map_err(error)?;
        let titles: Vec<(String, String)> = sqlx::query_as("SELECT pm.project_id,m.title FROM project_meetings pm JOIN meetings m ON m.id=pm.meeting_id WHERE pm.origin='manual' ORDER BY m.created_at DESC LIMIT 2000")
            .fetch_all(pool).await.map_err(error)?;
        let mut by_project: BTreeMap<String, ProjectInfo> = projects
            .into_iter()
            .map(|(id, name)| {
                let info = ProjectInfo {
                    id: id.clone(),
                    phrase: normalise(&name),
                    name,
                    vocabulary: Vec::new(),
                    people: Vec::new(),
                    titles: Vec::new(),
                };
                (id, info)
            })
            .collect();
        for (project, term) in terms {
            if let Some(p) = by_project.get_mut(&project) {
                let phrase = normalise(&term);
                if phrase.chars().count() >= 3 {
                    p.vocabulary.push((phrase, term));
                }
            }
        }
        for (project, name) in people {
            if let Some(p) = by_project.get_mut(&project) {
                let phrase = normalise(&name);
                if phrase.chars().count() >= 3 {
                    p.people.push((phrase, name));
                }
            }
        }
        for (project, title) in titles {
            let title = normalise(&title);
            if let Some(p) = by_project.get_mut(&project) {
                if specific(&title) && p.titles.len() < 50 && !p.titles.contains(&title) {
                    p.titles.push(title);
                }
            }
        }
        let mut map: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
        let mut title_rules = Vec::new();
        for (project, kind, value, label) in rules {
            if kind == "title" {
                title_rules.push((project.clone(), value.clone()));
            }
            map.entry((kind, value)).or_default().push((project, label));
        }
        Ok(Knowledge {
            projects: by_project.into_values().collect(),
            rules: map,
            titles: title_rules,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }
    fn known(&self, project: &str) -> bool {
        self.projects.iter().any(|p| p.id == project)
    }
    fn rule(&self, kind: &str, value: &str) -> &[(String, String)] {
        self.rules
            .get(&(kind.to_owned(), value.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

fn specific(normalised_title: &str) -> bool {
    normalised_title.split(' ').count() >= 2 && !GENERIC_TITLES.contains(&normalised_title)
}

fn people_phrase(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} others", rest.len()),
    }
}

#[derive(Default, Debug)]
struct Tally {
    score: f32,
    reasons: Vec<(f32, String)>,
}

#[derive(Default, Debug)]
pub struct Scores(HashMap<String, Tally>);

impl Scores {
    fn add(&mut self, project: &str, points: f32, reason: String) {
        if points <= 0.0 {
            return;
        }
        let tally = self.0.entry(project.to_owned()).or_default();
        tally.score += points;
        tally.reasons.push((points, reason));
    }
    #[cfg(test)]
    pub fn of(&self, project: &str) -> f32 {
        self.0.get(project).map_or(0.0, |t| t.score)
    }
}

/// Calendar evidence: series, invitees, their organisations, account and title.
pub fn score_event(k: &Knowledge, event: &Event, own: &Own, scores: &mut Scores) {
    if !event.series_id.is_empty() {
        for (project, label) in k.rule("series", &event.series_id) {
            let name = if label.is_empty() {
                &event.title
            } else {
                label
            };
            scores.add(project, 10.0, format!("Part of the “{name}” series"));
        }
    }
    let mut attendees: HashMap<&str, (f32, Vec<String>)> = HashMap::new();
    for guest in event.guests() {
        let projects = k.rule("email", &guest.email);
        for (project, _) in projects {
            let entry = attendees.entry(project).or_default();
            entry.0 += 3.0 / projects.len() as f32;
            entry.1.push(guest.label());
        }
    }
    for (project, (points, names)) in attendees {
        scores.add(
            project,
            points.min(9.0),
            format!(
                "{} usually {}",
                people_phrase(&names),
                if names.len() == 1 {
                    "attends"
                } else {
                    "attend"
                }
            ),
        );
    }
    let mut organisations: HashMap<&str, (f32, Vec<String>)> = HashMap::new();
    for domain in event.domains(own) {
        let projects = k.rule("domain", &domain);
        for (project, _) in projects {
            let entry = organisations.entry(project).or_default();
            entry.0 += 2.0 / projects.len() as f32;
            entry.1.push(domain.clone());
        }
    }
    for (project, (points, domains)) in organisations {
        scores.add(
            project,
            points.min(4.0),
            format!("Invitees from {}", domains.join(" and ")),
        );
    }
    // Which calendar an invite is on only breaks ties between projects.
    for (project, label) in k.rule("account", &event.source) {
        let label = if label.is_empty() { "this" } else { label };
        scores.add(project, 0.5, format!("From your {label} calendar"));
    }
    score_title(k, &event.title, scores);
}

pub fn score_title(k: &Knowledge, title: &str, scores: &mut Scores) {
    let title = normalise(title);
    if title.is_empty() {
        return;
    }
    for (project, phrase) in &k.titles {
        if k.known(project) && has_phrase(&title, phrase) {
            scores.add(project, 3.0, format!("Title includes “{phrase}”"));
        }
    }
    for p in &k.projects {
        if p.phrase.chars().count() >= 3 && has_phrase(&title, &p.phrase) {
            scores.add(&p.id, 3.0, format!("Title names {}", p.name));
        } else if specific(&title) && p.titles.contains(&title) {
            scores.add(
                &p.id,
                3.0,
                format!("Same title as earlier {} meetings", p.name),
            );
        }
    }
}

/// Transcript evidence, used only after a meeting: the project's name, its
/// vocabulary and the people who spoke or were mentioned.
pub fn score_transcript(k: &Knowledge, transcript: &str, speakers: &[String], scores: &mut Scores) {
    let text = normalise(transcript);
    let speakers: Vec<String> = speakers.iter().map(|s| normalise(s)).collect();
    for p in &k.projects {
        let mentions = count_phrase(&text, &p.phrase);
        if p.phrase.chars().count() >= 3 && mentions >= 2 {
            scores.add(
                &p.id,
                2.0,
                format!("{} is mentioned {mentions} times", p.name),
            );
        }
        let terms: Vec<&str> = p
            .vocabulary
            .iter()
            .filter(|(phrase, _)| has_phrase(&text, phrase))
            .map(|(_, term)| term.as_str())
            .take(3)
            .collect();
        if !terms.is_empty() {
            let list = people_phrase(&terms.iter().map(|t| t.to_string()).collect::<Vec<_>>());
            scores.add(&p.id, terms.len() as f32, format!("Mentions {list}"));
        }
        let spoke: Vec<String> = p
            .people
            .iter()
            .filter(|(phrase, _)| speakers.contains(phrase))
            .map(|(_, name)| name.clone())
            .take(2)
            .collect();
        if !spoke.is_empty() {
            scores.add(
                &p.id,
                2.0 * spoke.len() as f32,
                format!("{} spoke", people_phrase(&spoke)),
            );
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    Confident { project: String, reason: String },
    Suggest { project: String, reason: String },
    Nothing,
}

/// `mode` is the person's setting: `confident`, `suggest` or `off`.
pub fn decide(scores: &Scores, mode: &str) -> Decision {
    if mode == "off" {
        return Decision::Nothing;
    }
    let mut ranked: Vec<(&String, &Tally)> = scores.0.iter().collect();
    ranked.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(a.0.cmp(b.0)));
    let Some((project, top)) = ranked.first() else {
        return Decision::Nothing;
    };
    let second = ranked.get(1).map_or(0.0, |(_, t)| t.score);
    let mut reasons: Vec<&(f32, String)> = top.reasons.iter().collect();
    reasons.sort_by(|a, b| b.0.total_cmp(&a.0));
    let reason = reasons
        .iter()
        .take(2)
        .map(|(_, r)| r.as_str())
        .collect::<Vec<_>>()
        .join(" · ");
    let project = (*project).clone();
    let confident = top.score >= 6.0 && top.score - second >= 3.0 && top.score >= 2.0 * second;
    if confident && mode == "confident" {
        Decision::Confident { project, reason }
    } else if top.score >= 3.0 && top.score - second >= 1.0 {
        Decision::Suggest { project, reason }
    } else {
        Decision::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::super::signals::Person;
    use super::*;

    fn knowledge() -> Knowledge {
        let project = |id: &str, name: &str| ProjectInfo {
            id: id.into(),
            name: name.into(),
            phrase: normalise(name),
            vocabulary: vec![("kestrel".into(), "Kestrel".into())],
            people: vec![("maya chen".into(), "Maya Chen".into())],
            titles: vec!["release review".into()],
        };
        let mut rules = HashMap::new();
        let mut rule = |kind: &str, value: &str, project: &str| {
            rules
                .entry((kind.to_owned(), value.to_owned()))
                .or_insert_with(Vec::new)
                .push((project.to_owned(), String::new()));
        };
        rule("email", "maya@acme.example", "orion");
        rule("email", "jonas@acme.example", "orion");
        rule("email", "lena@north.example", "atlas");
        rule("email", "sam@shared.example", "orion");
        rule("email", "sam@shared.example", "atlas");
        rule("email", "kim@gmail.com", "orion");
        rule("domain", "acme.example", "orion");
        rule("series", "weekly-1", "atlas");
        Knowledge {
            projects: vec![project("orion", "Orion"), project("atlas", "Atlas")],
            rules,
            titles: Vec::new(),
        }
    }
    fn event(title: &str, series: &str, emails: &[&str]) -> Event {
        Event {
            title: title.into(),
            series_id: series.into(),
            people: emails
                .iter()
                .map(|e| Person {
                    email: (*e).into(),
                    name: String::new(),
                    ..Default::default()
                })
                .chain(std::iter::once(Person {
                    email: "me@mine.example".into(),
                    own: true,
                    ..Default::default()
                }))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn known_invitees_and_their_organisation_file_confidently_with_a_reason() {
        let k = knowledge();
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("Planning", "", &["maya@acme.example", "jonas@acme.example"]),
            &Own::default(),
            &mut scores,
        );
        match decide(&scores, "confident") {
            Decision::Confident { project, reason } => {
                assert_eq!(project, "orion");
                assert!(
                    reason.contains("maya@acme.example and jonas@acme.example usually attend"),
                    "{reason}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            decide(&scores, "suggest"),
            Decision::Suggest { .. }
        ));
        assert_eq!(decide(&scores, "off"), Decision::Nothing);
    }

    #[test]
    fn a_shared_person_alone_is_not_enough_and_ties_are_left_to_the_person() {
        let k = knowledge();
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("Planning", "", &["sam@shared.example"]),
            &Own::default(),
            &mut scores,
        );
        assert_eq!(decide(&scores, "confident"), Decision::Nothing);
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("Planning", "", &["kim@gmail.com", "lena@north.example"]),
            &Own::default(),
            &mut scores,
        );
        assert_eq!(decide(&scores, "confident"), Decision::Nothing);
        // A mixed meeting leaning one way is only a suggestion.
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("Planning", "", &["maya@acme.example", "lena@north.example"]),
            &Own::default(),
            &mut scores,
        );
        assert!(
            matches!(decide(&scores, "confident"), Decision::Suggest { ref project, .. } if project == "orion")
        );
    }

    #[test]
    fn a_known_series_decides_and_titles_and_transcripts_add_evidence() {
        let k = knowledge();
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("Weekly", "weekly-1", &["sam@shared.example"]),
            &Own::default(),
            &mut scores,
        );
        assert!(
            matches!(decide(&scores, "confident"), Decision::Confident { ref project, .. } if project == "atlas")
        );
        let mut scores = Scores::default();
        score_title(&k, "Orion launch prep", &mut scores);
        assert_eq!(scores.of("orion"), 3.0);
        score_transcript(
            &k,
            "Kestrel is ready. Orion ships, Orion is green.",
            &["Maya Chen".into()],
            &mut scores,
        );
        assert!(
            matches!(decide(&scores, "confident"), Decision::Confident { ref project, .. } if project == "orion")
        );
        // Own organisation never identifies a client.
        let own = Own {
            domains: ["acme.example".to_owned()].into(),
            ..Default::default()
        };
        let mut scores = Scores::default();
        score_event(
            &k,
            &event("x", "", &["new@acme.example"]),
            &own,
            &mut scores,
        );
        assert_eq!(scores.of("orion"), 0.0);
    }

    #[test]
    fn reasons_name_up_to_three_people_and_count_the_rest() {
        let names = |n: usize| -> Vec<String> {
            ["Maya", "Leila", "Jonas", "Nina"][..n]
                .iter()
                .map(|s| s.to_string())
                .collect()
        };
        assert_eq!(people_phrase(&names(2)), "Maya and Leila");
        assert_eq!(people_phrase(&names(3)), "Maya, Leila and Jonas");
        assert_eq!(people_phrase(&names(4)), "Maya, Leila and 2 others");
    }
}
