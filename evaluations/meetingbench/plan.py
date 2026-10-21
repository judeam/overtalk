"""Build a deterministic 200-meeting design before spending on synthesis.

Language weights are declared workload assumptions, NOT population estimates.
Coverage floors ensure that less common languages are not lost to weighting.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import random

LANGUAGES = {
    "zh": ("Chinese", 10), "en": ("English", 24), "yue": ("Cantonese", 2),
    "ar": ("Arabic", 5), "de": ("German", 6), "fr": ("French", 6),
    "es": ("Spanish", 8), "pt": ("Portuguese", 5), "id": ("Indonesian", 3),
    "it": ("Italian", 3), "ko": ("Korean", 3), "ru": ("Russian", 5),
    "th": ("Thai", 2), "vi": ("Vietnamese", 2), "ja": ("Japanese", 5),
    "tr": ("Turkish", 3), "hi": ("Hindi", 8), "ms": ("Malay", 2),
    "nl": ("Dutch", 2), "sv": ("Swedish", 1), "da": ("Danish", 1),
    "fi": ("Finnish", 1), "pl": ("Polish", 2), "cs": ("Czech", 1),
    "fil": ("Filipino", 2), "fa": ("Persian", 2), "el": ("Greek", 1),
    "hu": ("Hungarian", 1), "mk": ("Macedonian", 1), "ro": ("Romanian", 1),
}
DIALECTS = [
    "Anhui", "Dongbei", "Fujian", "Gansu", "Guizhou", "Hebei", "Henan",
    "Hubei", "Hunan", "Jiangxi", "Ningxia", "Shandong", "Shaanxi", "Shanxi",
    "Sichuan", "Tianjin", "Yunnan", "Zhejiang", "Cantonese (Hong Kong accent)",
    "Cantonese (Guangdong accent)", "Wu language", "Minnan language",
]
SCENARIOS = [
    "product roadmap and disputed release date", "incident response and root-cause review",
    "construction progress and contractor coordination", "school staff and timetable planning",
    "customer support escalation", "sales negotiation and procurement",
    "nonprofit event and volunteer planning", "design critique and accessibility review",
    "research planning and contradictory experiment results", "logistics and shipment delays",
    "restaurant staffing and inventory", "community association and maintenance budget",
    "engineering handover with technical terms", "hiring panel and candidate feedback",
    "finance planning with corrected amounts", "marketing campaign and revised responsibilities",
    "remote onboarding and connection interruptions", "manufacturing quality-control review",
    "sports club event planning", "cross-border project coordination",
]
# Equal counts in four declared bands, stratified logarithmically within each.
# This covers 2–90 minutes without making half of the paid corpus hour-long.
DURATION_BANDS = [(2, 4), (4, 9), (9, 20), (20, 90)]
PARTICIPANTS = [2, 3, 4, 6, 8]
PROFILES = {
    "normal": {"count": 100, "mixed": 16, "dialects": 12, "interruption_intensity": (0, 0), "peak": 1, "snr": (25, 38), "loss": (0, .005), "spoken_ratio": .80},
    "hard": {"count": 60, "mixed": 10, "dialects": 7, "interruption_intensity": (.3, .6), "peak": 2, "snr": (12, 24), "loss": (.01, .04), "spoken_ratio": .90},
    "really_hard": {"count": 40, "mixed": 6, "dialects": 3, "interruption_intensity": (.6, .9), "peak": 3, "snr": (4, 12), "loss": (.04, .09), "spoken_ratio": 1.0},
}


def build_plan(seed: int = 20261015) -> dict:
    rng = random.Random(seed)
    meetings, dialect_index = [], 0
    languages = list(LANGUAGES)
    for difficulty, profile in PROFILES.items():
        count = profile["count"]
        # The first 30 are one monolingual meeting per supported language.
        language_specs = [{"languages": [code], "dialect": None} for code in languages]
        for _ in range(profile["dialects"]):
            dialect = DIALECTS[dialect_index]
            dialect_index += 1
            language_specs.append({"languages": ["yue" if "Cantonese" in dialect else "zh"], "dialect": dialect})
        for i in range(profile["mixed"]):
            other = "hi" if i < 2 else ["de", "es", "zh", "ar", "pt", "fr", "ja", "hi"][i % 8]
            language_specs.append({"languages": ["en", other], "dialect": None})
        while len(language_specs) < count:
            code = rng.choices(languages, [LANGUAGES[c][1] for c in languages])[0]
            language_specs.append({"languages": [code], "dialect": None})
        rng.shuffle(language_specs)
        duration_specs = []
        for band, (low, high) in enumerate(DURATION_BANDS):
            n = count // len(DURATION_BANDS)
            for i in range(n):
                fraction = i / (n - 1)
                minutes = math.exp(math.log(low) + fraction * math.log(high / low))
                duration_specs.append((band, round(minutes * 60)))
        paired = [(band, duration, PARTICIPANTS[i % len(PARTICIPANTS)])
                  for i, (band, duration) in enumerate(duration_specs)]
        rng.shuffle(paired)
        scenarios = SCENARIOS * (count // len(SCENARIOS))
        rng.shuffle(scenarios)
        for index, (spec, (band, duration, participants), scenario) in enumerate(zip(language_specs, paired, scenarios)):
            meeting_id = f"{difficulty}-{index + 1:03d}"
            meetings.append({
                "id": meeting_id, "difficulty": difficulty, **spec,
                "duration_seconds": duration, "duration_band": band,
                "participants": participants,
                "scenario": scenario,
                "seed": rng.getrandbits(32),
                "overlap_policy": "heated_arguments_only",
                "interruption_intensity": round(rng.uniform(*profile["interruption_intensity"]), 3),
                "max_concurrent_speakers": min(participants, profile["peak"]),
                "target_snr_db": round(rng.uniform(*profile["snr"]), 1),
                "packet_loss_fraction": round(rng.uniform(*profile["loss"]), 4),
                "target_spoken_seconds": round(duration * profile["spoken_ratio"]),
                "conditions": (["natural accents", "hesitations", "self-corrections", "explicit decisions", "rejected proposals"]
                    + ([] if difficulty == "normal" else ["interruptions", "frustrated disagreement", "remote-call degradation"])
                    + ([] if difficulty != "really_hard" else ["heated argument", "quiet side conversation", "rapid speaker changes", "near-homophone names"])),
            })
    validate_plan(meetings)
    spoken = sum(m["target_spoken_seconds"] for m in meetings)
    return {"schema_version": 1, "seed": seed, "budget_usd": 100,
        "tts_model": "gemini-3.8-flash-tts", "meetings": meetings,
        "evaluation_models": {
            "asr": ["qwen3-asr-1.7b"],
            "summary": ["qwen3.5:4b"],
            "summary_inputs": ["reference_transcript", "asr_transcript"],
            "scope_note": "User narrowed the current run to large Qwen3-ASR and Qwen3.5 4B; other model comparisons are deferred.",
        },
        "allocation_policy": "30-language coverage floor in every difficulty; declared workload weights for remaining monolingual slots; equal duration-band and participant-count marginals. No claimed population probabilities.",
        "estimated_cost": {"basis": "Conservative $18/M audio tokens before promotional credits, 25 tokens/second; excludes tax. Actual API usage is ledgered; no spend above authorization.",
            "planned_spoken_hours": round(spoken / 3600, 2), "audio_usd_before_credits": round(spoken * 25 * 18 / 1e6, 2),
            "voice_design_and_retry_reserve_usd": 12},
        "sources": ["https://github.com/QwenLM/Qwen3-ASR", "https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/text-to-speech/voice-design", "https://cloud.google.com/gemini-enterprise-agent-platform/generative-ai/pricing"],
    }


def validate_plan(meetings: list[dict]) -> None:
    assert len(meetings) == 200 and len({m["id"] for m in meetings}) == 200
    for difficulty, profile in PROFILES.items():
        rows = [m for m in meetings if m["difficulty"] == difficulty]
        assert len(rows) == profile["count"]
        assert set(LANGUAGES) <= {m["languages"][0] for m in rows if len(m["languages"]) == 1 and not m["dialect"]}
        assert len(set(Counter(m["duration_band"] for m in rows).values())) == 1
        assert len(set(Counter(m["participants"] for m in rows).values())) == 1
        assert len(Counter((m["duration_band"], m["participants"]) for m in rows)) == 20
        assert len(set(Counter((m["duration_band"], m["participants"]) for m in rows).values())) == 1
        assert set(m["scenario"] for m in rows) == set(SCENARIOS)
        assert min(m["duration_seconds"] for m in rows) == 120
        assert max(m["duration_seconds"] for m in rows) == 5400
    assert {m["dialect"] for m in meetings if m["dialect"]} == set(DIALECTS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path(__file__).parent / ".runtime/plan.json")
    parser.add_argument("--seed", type=int, default=20261015)
    args = parser.parse_args()
    plan = build_plan(args.seed)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(plan, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"meetings": len(plan["meetings"]), "difficulty": dict(Counter(m["difficulty"] for m in plan["meetings"])), "cost": plan["estimated_cost"], "plan_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest()}, indent=2))


if __name__ == "__main__":
    main()
