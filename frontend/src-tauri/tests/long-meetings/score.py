#!/usr/bin/env python3
"""Scores notes written by eval_long_meetings against each meeting's answer key.

    python3 score.py <output dir>

Per model and mode: decisions found, traps reported as decisions, actions found, correct owners
and due dates among the found actions, action lines that match nothing planted, and time.
"""
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent


def has(line, keyword):
    return any(alt.lower() in line for alt in keyword.split("|"))


def sections(markdown, word):
    """Lines under any heading that contains `word`."""
    lines, inside = [], False
    for raw in markdown.splitlines():
        if raw.lstrip().startswith("#"):
            inside = word in raw.lower()
            continue
        line = raw.strip().lower()
        if inside and line and not re.fullmatch(r"\|?[\s:\-|]+\|?", line):
            lines.append(line)
    return lines


def score(markdown, key, people):
    decisions = sections(markdown, "decision")
    actions = [l for l in sections(markdown, "action") if not ("owner" in l and "task" in l and l.startswith("|"))]
    found = lambda lines, keywords: next((l for l in lines if all(has(l, k) for k in keywords)), None)
    result = dict(decisions=0, traps=0, actions=0, owners=0, dues=0, wrong_owner=0, unmatched=0,
                  n_decisions=len(key["decisions"]), n_actions=len(key["actions"]), n_traps=len(key["traps"]))
    result["decisions"] = sum(found(decisions, d["keywords"]) is not None for d in key["decisions"])
    # A trap mentioned as rejected ("keeping both offices was rejected") is not a false decision.
    rejected = re.compile(r"reject|declin|\bnot\b|\bno\b|won't|will not|out of scope|parked|deferred|abgelehnt|nicht")
    result["traps"] = sum(
        (line := found(decisions, t["keywords"])) is not None and not rejected.search(line) for t in key["traps"]
    )
    matched = set()
    for action in key["actions"]:
        line = found(actions, action["keywords"])
        if line is None:
            continue
        matched.add(line)
        result["actions"] += 1
        if action["owner"].lower() in line:
            result["owners"] += 1
        elif any(p.lower() in line for p in people if p != action["owner"]):
            result["wrong_owner"] += 1
        if any(d.lower() in line for d in action["due"]):
            result["dues"] += 1
    result["unmatched"] = sum(1 for l in actions if l not in matched and len(l) > 12 and "none noted" not in l)
    return result


def main():
    out = Path(sys.argv[1])
    rows = defaultdict(lambda: defaultdict(float))
    for md in sorted(out.glob("*.md")):
        meeting, model, mode = md.stem.split("__")
        data = json.loads((HERE / f"meeting-{meeting}.json").read_text())
        timing = json.loads(md.with_suffix(".json").read_text())
        text = md.read_text()
        row = rows[(model, mode)]
        row["runs"] += 1
        row["seconds"] += timing["seconds"]
        if text.startswith("ERROR"):
            row["errors"] += 1
            continue
        for k, v in score(text, data["key"], data["people"]).items():
            row[k] += v
    print(f"{'model':<12} {'mode':<9} {'decisions':>10} {'traps':>7} {'actions':>9} {'owners':>8} {'dues':>6} {'wrong':>6} {'extra':>6} {'errors':>6} {'time':>7}")
    for (model, mode), r in sorted(rows.items()):
        print(f"{model:<12} {mode:<9} {int(r['decisions']):>4}/{int(r['n_decisions']):<5} {int(r['traps']):>3}/{int(r['n_traps']):<3} "
              f"{int(r['actions']):>4}/{int(r['n_actions']):<4} {int(r['owners']):>7} {int(r['dues']):>6} {int(r['wrong_owner']):>6} "
              f"{int(r['unmatched']):>6} {int(r['errors']):>6} {r['seconds']:>6.0f}s")


if __name__ == "__main__":
    main()
