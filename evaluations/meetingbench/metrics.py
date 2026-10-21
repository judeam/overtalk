"""Transparent text metrics; accepted spelling groups are frozen before test use.

Strict errors remain visible. Never learn substitutions from test predictions.
"""
from __future__ import annotations

import unicodedata

ORTHOGRAPHY = {
    "hi": [
        ("हफ़्ते", "हफ्ते"), ("बताऊँ", "बताऊं"), ("पाँच", "पांच"),
        ("friday", "फ्राइडे"), ("customers", "कस्टमर्स"),
        ("confirmed", "कंफर्म्ड", "कन्फर्म्ड"), ("plan", "प्लान"),
        ("approval", "अप्रूवल", "अप्रोवल"), ("announcement", "अनाउंसमेंट"),
        ("security", "सिक्योरिटी"), ("review", "रिव्यू"),
        ("release", "रिलीज", "रिलीज़"), ("date", "डेट"),
    ],
}
CHARACTER_PRIMARY = {"zh", "yue", "ja", "th"}


def normalize(text: str) -> str:
    text = unicodedata.normalize("NFKC", text).casefold()
    return " ".join("".join(
        " " if unicodedata.category(c).startswith("P")
        else str(unicodedata.decimal(c)) if unicodedata.category(c) == "Nd"
        else c for c in text).split())


def distance(reference, hypothesis) -> int:
    # Bit-parallel Levenshtein keeps long-meeting scoring bounded in memory.
    if len(reference) > len(hypothesis):
        reference, hypothesis = hypothesis, reference
    length = len(reference)
    if not length:
        return len(hypothesis)
    matches = {}
    for i, token in enumerate(reference):
        matches[token] = matches.get(token, 0) | (1 << i)
    mask, high = (1 << length) - 1, 1 << (length - 1)
    positive, negative, errors = mask, 0, length
    for token in hypothesis:
        equal = matches.get(token, 0)
        vertical = equal | negative
        horizontal = (((equal & positive) + positive) ^ positive) | equal
        ph = negative | ~(horizontal | positive)
        nh = positive & horizontal
        errors += bool(ph & high) - bool(nh & high)
        ph = (ph << 1) | 1
        nh <<= 1
        positive = (nh | ~(vertical | ph)) & mask
        negative = ph & vertical
    return errors


def accepted_tokens(text: str, languages: list[str], extra_groups=()) -> list[str]:
    aliases = {}
    groups = [g for language in languages for g in ORTHOGRAPHY.get(language, [])]
    groups.extend(extra_groups)
    for index, group in enumerate(groups):
        for word in group:
            tokens = normalize(word).split()
            if len(tokens) != 1:
                raise ValueError("Orthographic groups must contain single tokens")
            key = tokens[0]
            canonical = f"__orthography_{index}__"
            if key in aliases and aliases[key] != canonical:
                raise ValueError("An orthographic variant belongs to two groups")
            aliases[key] = canonical
    return [aliases.get(token, token) for token in normalize(text).split()]


def transcript_score(reference: str, hypothesis: str | None, languages: list[str], extra_groups=()) -> dict:
    failed = hypothesis is None
    ref, hyp = normalize(reference), normalize(hypothesis or "")
    rw, hw = ref.split(), hyp.split()
    rc, hc = ref.replace(" ", ""), hyp.replace(" ", "")
    tolerant = distance(accepted_tokens(ref, languages, extra_groups), accepted_tokens(hyp, languages, extra_groups))
    strict_words, chars = distance(rw, hw), distance(rc, hc)
    primary = "cer" if any(language in CHARACTER_PRIMARY for language in languages) else "orthographic_wer"
    result = {"failed": failed, "reference_words": len(rw), "word_errors": strict_words,
              "strict_wer": strict_words / len(rw) if rw else None,
              "orthographic_word_errors": tolerant,
              "orthographic_wer": tolerant / len(rw) if rw else None,
              "reference_characters": len(rc), "character_errors": chars,
              "cer": chars / len(rc) if rc else None,
              "silence_hallucination_words": len(hw) if not rw else 0,
              "primary_metric": primary,
              "normalization": "nfkc-casefold-decimal-punctuation-space-v1",
              "orthography_policy": "explicit-groups-v1; frozen using excluded pilot; strict scores retained"}
    rate = result[primary]
    result["diagnostic_band"] = "failed" if failed else "silence" if rate is None else "excellent" if rate < .05 else "good" if rate < .15 else "usable_with_corrections" if rate < .30 else "poor"
    result["band_caveat"] = "Heuristic diagnostic thresholds, not human-validated acceptance criteria"
    return result
