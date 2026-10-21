#!/usr/bin/env python3
"""Synthetic long meetings with an answer key, for scoring notes from small local models.

Every planted decision and action carries keywords that survive translation (names, codenames,
numbers), so English notes written from a German meeting can be scored too. Traps are proposals
that were rejected or left open; they must not appear as decisions.

    python3 generate.py            # writes meeting-*.json next to this file
"""
import json
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Each topic: intro noun phrase, facts, a decision, actions, and a trap.
# keywords: every entry must appear in one output line; "a|b" accepts either spelling.
# owner: index into the meeting's people; due: accepted spellings of the due date.
EN_TOPICS = [
    dict(topic="the Kestrel dashboard redesign",
         facts=["The current Kestrel dashboard loads in about 4.2 seconds on the median account.",
                "Around 1,800 customers open Kestrel every week."],
         decision=("So we agree: the Kestrel redesign ships after the trade fair in March, not before.", ["Kestrel", "March"]),
         actions=[("can you send the Kestrel mockups to the design review group", ["Kestrel", "mockups"], 1, ["Thursday"])],
         trap=("What if we dropped the dark theme from Kestrel entirely?",
               "I'd rather not decide that today, we need the survey data first.", ["dark theme"])),
    dict(topic="the invoice export for Halden Logistics",
         facts=["Halden Logistics sends us roughly 600 invoices a month.",
                "The PDF export breaks on tables longer than two pages."],
         decision=("Decision then: we fix the long-table PDF layout for Halden before any new export formats.", ["Halden", "PDF"]),
         actions=[("can you call Halden and confirm their invoice template", ["Halden", "template"], 2, ["Monday"]),
                  ("will you write the regression tests for the long tables", ["regression", "tables"], 3, ["end of the month", "end of month", "month end"])],
         trap=("Should we offer Halden an Excel export as well?",
               "No, not now. Excel export is out of scope this quarter.", ["Excel"])),
    dict(topic="the on-call rotation",
         facts=["We had 14 pages out of hours last month.",
                "Two of them were false alarms from the disk monitor."],
         decision=("Agreed: the on-call rotation moves to one-week shifts starting in November.", ["one-week", "November"]),
         actions=[("can you update the PagerDuty schedule", ["PagerDuty"], 0, ["Friday"])],
         trap=("Could we pay a weekend bonus for on-call?",
               "That needs HR, we can't agree it in this meeting.", ["weekend bonus"])),
    dict(topic="the Q3 hiring plan",
         facts=["We have budget for three engineering roles.",
                "The backend role has been open for 11 weeks."],
         decision=("Okay, we decide to hire the backend engineer first and the two frontend roles after.", ["backend engineer", "first"]),
         actions=[("can you rewrite the backend job description", ["job description"], 1, ["next Wednesday", "Wednesday"])],
         trap=("What about hiring a contractor in the meantime?",
               "Let's park that, I'm not convinced it's worth the onboarding time.", ["contractor"])),
    dict(topic="the Lisbon office move",
         facts=["The Lisbon lease ends on the 30th of January.",
                "The new space has 22 desks."],
         decision=("Fine, we sign the lease for the Alcântara space in Lisbon.", ["Alcântara|Alcantara"]),
         actions=[("can you get three quotes from moving companies", ["moving", "quotes"], 2, ["December 5", "5 December", "5th of December", "Dec 5"])],
         trap=("Maybe we should keep both offices for a while?",
               "That doubles the rent, so no.", ["both offices"])),
    dict(topic="the Orion API migration",
         facts=["Orion v1 still serves about 9 percent of API traffic.",
                "Seventeen integrations call the deprecated endpoints."],
         decision=("Decided: Orion v1 is switched off on the 1st of February.", ["Orion", "February"]),
         actions=[("can you email the seventeen integration owners about Orion", ["integration owners"], 3, ["tomorrow"])],
         trap=("Should we charge customers who stay on v1?",
               "No, that would only cause friction.", ["charge"])),
    dict(topic="the customer survey results",
         facts=["We got 412 responses to the survey.",
                "Search was the most requested feature, mentioned 133 times."],
         decision=("So the decision is that search goes on the roadmap ahead of offline mode.", ["search", "offline"]),
         actions=[("can you write a one-page summary of the survey for the board", ["survey", "board"], 0, ["Tuesday"])],
         trap=("Do we want to run a second survey in Spanish?",
               "Maybe later in the year, not now.", ["Spanish"])),
    dict(topic="the Bramble mobile release",
         facts=["Bramble 3.2 has 41 open bugs, 6 of them critical.",
                "App store review usually takes two days."],
         decision=("We agree to delay Bramble 3.2 by one week to fix the critical bugs.", ["Bramble", "one week|1 week"]),
         actions=[("can you triage the six critical Bramble bugs", ["critical", "Bramble"], 1, ["today", "end of day"]),
                  ("will you tell the support team about the new Bramble date", ["support team"], 2, ["Monday"])],
         trap=("Could we ship Bramble 3.2 without the tablet layout?",
               "No, the tablet layout is half the release.", ["tablet layout"])),
    dict(topic="the security audit findings",
         facts=["The auditors found 3 high and 9 medium findings.",
                "The audit report is due to the client by the 15th."],
         decision=("Agreed then: all three high findings get fixed before we answer the auditors.", ["high findings"]),
         actions=[("can you rotate the leaked staging keys", ["staging keys"], 3, ["tonight", "today"])],
         trap=("Should we hire a second audit firm for a re-test?",
               "Too expensive, let's not.", ["second audit"])),
    dict(topic="the pricing page experiment",
         facts=["The annual plan converts at 2.8 percent.",
                "The experiment ran for 19 days."],
         decision=("Decision: we keep the new pricing page with the annual plan shown first.", ["annual plan", "pricing page"]),
         actions=[("can you share the pricing experiment dashboard with sales", ["pricing", "sales"], 0, ["Thursday"])],
         trap=("What if we removed the free tier?",
               "That's a much bigger conversation, not today.", ["free tier"])),
    dict(topic="the data retention policy",
         facts=["We currently keep raw logs for 400 days.",
                "Legal wants a maximum of 90 days for personal data."],
         decision=("So we agree: raw logs with personal data are deleted after 90 days.", ["90 days"]),
         actions=[("can you draft the retention policy text for legal", ["retention policy"], 2, ["next Friday", "Friday"])],
         trap=("Could we anonymise instead of deleting?",
               "Legal said anonymisation isn't enough here.", ["anonymis|anonymiz"])),
    dict(topic="the Juniper search index",
         facts=["Juniper reindexing takes about 7 hours.",
                "The index is 340 gigabytes."],
         decision=("Decided: we move Juniper to incremental indexing this quarter.", ["Juniper", "incremental"]),
         actions=[("can you benchmark incremental indexing on the staging copy", ["benchmark"], 3, ["Wednesday"])],
         trap=("Should we switch to a hosted search service?",
               "Not until we know the incremental numbers.", ["hosted search"])),
    dict(topic="the partner webinar",
         facts=["Last year's webinar had 260 attendees.",
                "Two partners have confirmed so far."],
         decision=("Okay, the partner webinar is on the 12th of November.", ["webinar", "November"]),
         actions=[("can you send the webinar invitations to the partner list", ["invitations"], 1, ["Monday"])],
         trap=("What if we did it in person instead?",
               "Travel budget is frozen, so no.", ["in person"])),
    dict(topic="the support backlog",
         facts=["The support backlog is at 230 tickets.",
                "Median first response time is 19 hours."],
         decision=("We agree that engineers join the support rotation for two hours every Friday.", ["support rotation", "Friday"]),
         actions=[("can you set up the macro for the top ten questions", ["macro"], 0, ["next week"])],
         trap=("Should we outsource first-line support?",
               "I'm against that, quality would drop.", ["outsource"])),
    dict(topic="the Tamsin onboarding flow",
         facts=["Only 38 percent of new users finish Tamsin onboarding.",
                "The drop-off is on the import step."],
         decision=("Decision: the Tamsin import step becomes optional.", ["Tamsin", "optional"]),
         actions=[("can you write the copy for the skip button", ["skip button"], 2, ["Thursday"])],
         trap=("Could we remove onboarding completely?",
               "No, activation would collapse.", ["remove onboarding"])),
]

DE_TOPICS = [
    dict(topic="das Kestrel-Dashboard",
         facts=["Das Kestrel-Dashboard lädt im Schnitt in 4,2 Sekunden.",
                "Rund 1.800 Kunden öffnen Kestrel jede Woche."],
         decision=("Dann ist es beschlossen: Das neue Kestrel-Dashboard kommt erst nach der Messe im März.", ["Kestrel", "March|März"]),
         actions=[("kannst du die Kestrel-Entwürfe an die Design-Runde schicken", ["Kestrel"], 1, ["Thursday", "Donnerstag"])],
         trap=("Sollen wir das dunkle Design ganz weglassen?",
               "Das würde ich heute nicht entscheiden, erst brauchen wir die Umfrage.", ["dark|dunkle"])),
    dict(topic="den Rechnungsexport für Bauer GmbH",
         facts=["Bauer GmbH schickt uns ungefähr 600 Rechnungen im Monat.",
                "Der PDF-Export bricht bei Tabellen über zwei Seiten."],
         decision=("Beschluss: Wir reparieren zuerst das PDF-Layout für lange Tabellen, bevor wir neue Formate bauen.", ["PDF", "table|Tabelle"]),
         actions=[("kannst du Bauer anrufen und die Rechnungsvorlage bestätigen lassen", ["Bauer"], 2, ["Monday", "Montag"])],
         trap=("Sollen wir Bauer auch einen Excel-Export anbieten?",
               "Nein, Excel ist in diesem Quartal nicht drin.", ["Excel"])),
    dict(topic="die Rufbereitschaft",
         facts=["Wir hatten letzten Monat 14 Alarme außerhalb der Arbeitszeit.",
                "Zwei davon waren Fehlalarme vom Festplattenmonitor."],
         decision=("Einverstanden, die Rufbereitschaft wechselt ab November auf Wochenschichten.", ["November"]),
         actions=[("kannst du den PagerDuty-Plan anpassen", ["PagerDuty"], 0, ["Friday", "Freitag"])],
         trap=("Könnten wir einen Wochenendbonus zahlen?",
               "Das muss die Personalabteilung entscheiden, nicht wir hier.", ["bonus"])),
    dict(topic="den Umzug des Büros in Leipzig",
         facts=["Der Mietvertrag in Leipzig endet am 30. Januar.",
                "Das neue Büro hat 22 Arbeitsplätze."],
         decision=("Gut, wir unterschreiben den Mietvertrag für das Büro in der Plagwitzer Straße.", ["Plagwitz"]),
         actions=[("kannst du drei Angebote von Umzugsfirmen einholen", ["quote|Angebot|offer"], 2, ["December 5", "5 December", "5. Dezember", "Dec 5", "5th of December"])],
         trap=("Vielleicht behalten wir beide Büros noch eine Weile?",
               "Das verdoppelt die Miete, also nein.", ["both offices|beide Büros|two offices"])),
    dict(topic="die Orion-Schnittstelle",
         facts=["Orion v1 bedient noch etwa 9 Prozent des Verkehrs.",
                "Siebzehn Integrationen rufen die alten Endpunkte auf."],
         decision=("Beschlossen: Orion v1 wird am 1. Februar abgeschaltet.", ["Orion", "February|Februar"]),
         actions=[("kannst du die siebzehn Integrationspartner wegen Orion anschreiben", ["Orion", "17|seventeen|siebzehn"], 3, ["tomorrow", "morgen"])],
         trap=("Sollen wir Kunden, die auf v1 bleiben, etwas berechnen?",
               "Nein, das bringt nur Ärger.", ["charge|berechnen|fee"])),
    dict(topic="die Ergebnisse der Kundenumfrage",
         facts=["Wir haben 412 Antworten auf die Umfrage bekommen.",
                "Die Suche wurde 133 Mal gewünscht."],
         decision=("Also beschlossen: Die Suche kommt vor dem Offline-Modus auf die Roadmap.", ["search|Suche", "offline"]),
         actions=[("kannst du eine einseitige Zusammenfassung der Umfrage für den Vorstand schreiben", ["survey|Umfrage", "board|Vorstand"], 0, ["Tuesday", "Dienstag"])],
         trap=("Machen wir eine zweite Umfrage auf Spanisch?",
               "Vielleicht später im Jahr, jetzt nicht.", ["Spanish|Spanisch"])),
    dict(topic="die Bramble-App",
         facts=["Bramble 3.2 hat 41 offene Fehler, 6 davon kritisch.",
                "Die Prüfung im App Store dauert meistens zwei Tage."],
         decision=("Wir verschieben Bramble 3.2 um eine Woche, um die kritischen Fehler zu beheben.", ["Bramble", "week|Woche"]),
         actions=[("kannst du die sechs kritischen Bramble-Fehler priorisieren", ["Bramble", "critical|kritisch"], 1, ["today", "heute"])],
         trap=("Könnten wir Bramble ohne das Tablet-Layout veröffentlichen?",
               "Nein, das Tablet-Layout ist die halbe Version.", ["tablet"])),
    dict(topic="die Aufbewahrung von Daten",
         facts=["Wir speichern Rohdaten derzeit 400 Tage.",
                "Die Rechtsabteilung will höchstens 90 Tage für personenbezogene Daten."],
         decision=("Einverstanden: Rohdaten mit personenbezogenen Daten werden nach 90 Tagen gelöscht.", ["90"]),
         actions=[("kannst du den Text der Richtlinie für die Rechtsabteilung entwerfen", ["polic|Richtlinie"], 2, ["Friday", "Freitag"])],
         trap=("Könnten wir anonymisieren statt löschen?",
               "Die Rechtsabteilung sagt, Anonymisieren reicht hier nicht.", ["anonymis|anonymiz"])),
]

LANG = {
    "en": dict(
        people=["Lena", "Marco", "Priya", "Tom"],
        intro=["Next item: {t}.", "Okay, let's move on to {t}.", "{n}, you wanted to talk about {t}?", "Let's look at {t} now."],
        ask=["{o}, {a} by {d}?", "{o}, {a}? Ideally by {d}."],
        accept=["Yes, I'll do that by {d}.", "Sure, I can have that done by {d}.", "Okay, I'll take it, {d} works."],
        agree=["Agreed.", "Yes, that works for me.", "Fine by me.", "Okay, agreed."],
        back=["Mhm.", "Right.", "Uh-huh.", "Hmm.", "Mm-hm."],
        filler=[
            "I looked at the numbers again and they're roughly where we expected.",
            "The main risk is still the timeline, honestly.",
            "I talked to the design team yesterday and they had a few concerns.",
            "Can you share your screen for a second?",
            "Sure, one moment, let me find the right window.",
            "I think we should keep it simple for the first version.",
            "Let me check my notes, I think this came up in the last retro too.",
            "That's a fair point, but customers haven't really complained about it.",
            "We tried something similar two years ago and it didn't go well.",
            "I'm not sure the data supports that yet.",
            "From the support side, this is one of the top complaints.",
            "Could you repeat that? Your audio cut out for a moment.",
            "Sorry, I was on mute.",
            "I agree with the direction, I'm just worried about the effort.",
            "We should loop in finance before we commit to anything expensive.",
            "The last time we estimated this, it took twice as long as planned.",
            "Honestly I think the team can handle it if we cut scope a bit.",
            "There's also a dependency on the platform team that we shouldn't forget.",
            "I'll be out on Friday afternoon, just so you know.",
            "My gut feeling is that we're overthinking this one.",
            "Let's not go down that rabbit hole right now.",
            "I saw a competitor launch something like this last month.",
            "We need to be careful with the wording towards customers.",
            "Does anyone have the link to the planning document?",
            "I just dropped it in the chat.",
            "Thanks, got it.",
            "The metrics dashboard was down this morning, so these numbers are from yesterday.",
            "This affects the mobile app as well, not just the web.",
            "I would like to see a rough estimate before we go further.",
            "We discussed this with the customer success team last week.",
            "Let's keep an eye on it and revisit if it gets worse.",
            "I don't have a strong opinion on this one.",
            "From an engineering point of view it's not that hard.",
            "The documentation for this is pretty outdated, by the way.",
            "Coffee break soon? We've been going for a while.",
            "Let's finish this topic first and then take five minutes.",
        ],
    ),
    "de": dict(
        people=["Jonas", "Clara", "Mehmet", "Sofia"],
        intro=["Nächster Punkt: {t}.", "Okay, dann zu {t}.", "{n}, du wolltest noch über {t} sprechen?", "Schauen wir uns jetzt {t} an."],
        ask=["{o}, {a}, bis {d}?", "{o}, {a}? Am besten bis {d}."],
        accept=["Ja, mache ich bis {d}.", "Klar, das schaffe ich bis {d}.", "Okay, übernehme ich, {d} passt."],
        agree=["Einverstanden.", "Ja, passt für mich.", "Gut, so machen wir es.", "Okay, einverstanden."],
        back=["Mhm.", "Ähm.", "Hm.", "Aha.", "Mhm, ja."],
        filler=[
            "Ich habe mir die Zahlen nochmal angeschaut, die liegen ungefähr da, wo wir dachten.",
            "Das größte Risiko ist ehrlich gesagt immer noch der Zeitplan.",
            "Ich habe gestern mit dem Design-Team gesprochen, die hatten ein paar Bedenken.",
            "Kannst du kurz deinen Bildschirm teilen?",
            "Klar, Moment, ich suche das richtige Fenster.",
            "Ich würde es für die erste Version einfach halten.",
            "Ich schaue kurz in meine Notizen, das kam in der letzten Retro auch schon.",
            "Guter Punkt, aber die Kunden haben sich darüber eigentlich nicht beschwert.",
            "Wir haben vor zwei Jahren etwas Ähnliches versucht, das lief nicht gut.",
            "Ich bin nicht sicher, ob die Daten das schon hergeben.",
            "Aus Sicht des Supports ist das eine der häufigsten Beschwerden.",
            "Kannst du das wiederholen? Dein Ton war kurz weg.",
            "Sorry, ich war stummgeschaltet.",
            "Die Richtung finde ich gut, ich mache mir nur Sorgen um den Aufwand.",
            "Wir sollten die Finanzabteilung einbeziehen, bevor wir etwas Teures zusagen.",
            "Beim letzten Mal hat das doppelt so lange gedauert wie geplant.",
            "Ich glaube, das Team schafft das, wenn wir den Umfang etwas kürzen.",
            "Es gibt auch eine Abhängigkeit zum Plattform-Team, die wir nicht vergessen sollten.",
            "Ich bin Freitagnachmittag übrigens nicht da.",
            "Mein Bauchgefühl sagt, dass wir das zerdenken.",
            "Lass uns da jetzt nicht zu tief einsteigen.",
            "Ein Wettbewerber hat letzten Monat etwas Ähnliches gestartet.",
            "Bei der Formulierung gegenüber Kunden müssen wir vorsichtig sein.",
            "Hat jemand den Link zum Planungsdokument?",
            "Hab ihn gerade in den Chat gestellt.",
            "Danke, hab ihn.",
            "Das Kennzahlen-Dashboard war heute früh down, die Zahlen sind von gestern.",
            "Das betrifft übrigens auch die App, nicht nur das Web.",
            "Ich hätte gern eine grobe Schätzung, bevor wir weitermachen.",
            "Wir haben das letzte Woche mit dem Customer-Success-Team besprochen.",
            "Lass uns das im Auge behalten und nochmal drauf schauen, wenn es schlimmer wird.",
            "Ich habe dazu keine starke Meinung.",
            "Technisch ist das nicht besonders schwer.",
            "Die Dokumentation dazu ist übrigens ziemlich veraltet.",
            "Kaffeepause bald? Wir sind schon eine Weile dran.",
            "Lass uns das Thema noch fertig machen, dann fünf Minuten Pause.",
        ],
    ),
}

DUE_DE = {"Thursday": "Donnerstag", "Monday": "Montag", "Friday": "Freitag", "Tuesday": "Dienstag",
          "Wednesday": "Mittwoch", "tomorrow": "morgen", "today": "heute"}


def build(name, language, topics, minutes, seed):
    rnd = random.Random(seed)
    L = LANG[language]
    people = L["people"]
    lines, key = [], {"decisions": [], "actions": [], "traps": []}
    clock = 0.0

    def say(speaker, text):
        nonlocal clock
        lines.append({"t": int(clock), "speaker": speaker, "text": text})
        clock += 1.5 + len(text.split()) / 2.4

    # About 92 words of filler per minute; pauses between turns make up the rest of the time.
    words_per_topic = int(minutes * 92 / len(topics))
    for topic_index, topic in enumerate(topics):
        lead = people[topic_index % len(people)]
        say(lead, rnd.choice(L["intro"]).format(t=topic["topic"], n=rnd.choice([p for p in people if p != lead])))
        events = [("fact", f) for f in topic["facts"]]
        events.append(("trap", topic["trap"]))
        events.append(("decision", topic["decision"]))
        events += [("action", a) for a in topic["actions"]]
        rnd.shuffle(events)
        # The decision closes the discussion of its topic; actions follow it.
        events.sort(key=lambda e: {"fact": 0, "trap": 1, "decision": 2, "action": 3}[e[0]])
        budget = words_per_topic
        slots = sorted(rnd.sample(range(1, 40), len(events)))
        position = 0
        while budget > 0 or events:
            position += 1
            if events and (position >= slots[0] or budget <= 0):
                slots.pop(0)
                kind, item = events.pop(0)
                speaker = rnd.choice(people)
                if kind == "fact":
                    say(speaker, item)
                elif kind == "trap":
                    proposal, rejection, keywords = item
                    say(speaker, proposal)
                    say(rnd.choice([p for p in people if p != speaker]), rejection)
                    key["traps"].append({"keywords": keywords})
                elif kind == "decision":
                    sentence, keywords = item
                    say(lead, sentence)
                    say(rnd.choice([p for p in people if p != lead]), rnd.choice(L["agree"]))
                    key["decisions"].append({"keywords": keywords})
                else:
                    task, keywords, owner_index, dues = item
                    owner = people[owner_index]
                    due_word = dues[0] if language == "en" else DUE_DE.get(dues[0], dues[-1])
                    asker = rnd.choice([p for p in people if p != owner])
                    say(asker, rnd.choice(L["ask"]).format(o=owner, a=task, d=due_word))
                    say(owner, rnd.choice(L["accept"]).format(d=due_word))
                    key["actions"].append({"keywords": keywords, "owner": owner, "due": dues + ([DUE_DE[dues[0]]] if dues[0] in DUE_DE else [])})
                continue
            text = rnd.choice(L["back"]) if rnd.random() < 0.18 else rnd.choice(L["filler"])
            say(rnd.choice(people), text)
            budget -= len(text.split())
    return {"name": name, "language": language, "minutes": round(clock / 60), "people": people, "lines": lines, "key": key}


def transcript_text(meeting):
    """The line format the app sends to the summary pipeline."""
    return "\n".join(f"[{l['t'] // 60:02d}:{l['t'] % 60:02d}] [{l['speaker']}] {l['text']}" for l in meeting["lines"])


if __name__ == "__main__":
    meetings = [
        build("en-30min-product", "en", EN_TOPICS[:5], 30, 1),
        build("en-60min-planning", "en", EN_TOPICS[3:13], 60, 2),
        build("en-90min-review", "en", EN_TOPICS, 90, 3),
        build("de-45min-team", "de", DE_TOPICS, 45, 4),
    ]
    for meeting in meetings:
        meeting["transcript"] = transcript_text(meeting)
        (HERE / f"meeting-{meeting['name']}.json").write_text(json.dumps(meeting, ensure_ascii=False, indent=1) + "\n")
        k = meeting["key"]
        print(f"{meeting['name']}: {meeting['minutes']} min, {len(meeting['lines'])} lines, {len(meeting['transcript'])} chars, "
              f"{len(k['decisions'])} decisions, {len(k['actions'])} actions, {len(k['traps'])} traps")
