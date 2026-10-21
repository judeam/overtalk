import json
from pathlib import Path
import random
import tempfile
import unittest
import zipfile

from evaluations.meetingbench.metrics import distance
from .reference import ami_orthography, meeting_reference, overlap_fraction, parse_xml
from .score import align, alignment, breakdown, normalise, pooled, reference_texts, score_condition

HEADER = '<?xml version="1.0" encoding="ISO-8859-1" standalone="yes"?>\n'
NS = 'xmlns:nite="http://nite.sourceforge.net/"'


def words_xml(meeting, speaker, rows):
    body = "".join(rows)
    return f'{HEADER}<nite:root nite:id="{meeting}.{speaker}.words" {NS}>{body}</nite:root>'


def word(meeting, speaker, index, start, text, **extra):
    attributes = "".join(f' {key}="{value}"' for key, value in extra.items())
    return (f'<w nite:id="{meeting}.{speaker}.words{index}" starttime="{start}" '
            f'endtime="{start + 0.2}"{attributes}>{text}</w>')


def segments_xml(meeting, speaker, spans):
    rows = "".join(
        f'<segment nite:id="{meeting}.sync.{n}" transcriber_start="{start}" transcriber_end="{end}">'
        f'<nite:child href="{meeting}.{speaker}.words.xml#id({meeting}.{speaker}.words{first})'
        f'..id({meeting}.{speaker}.words{last})"/></segment>'
        for n, (start, end, first, last) in enumerate(spans))
    return f'{HEADER}<nite:root nite:id="{meeting}.{speaker}.segs" {NS}>{rows}</nite:root>'


class ReferenceTests(unittest.TestCase):
    def build(self, members):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "annotations.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name, text in members.items():
                archive.writestr(name, text.encode("iso-8859-1"))
        archive = zipfile.ZipFile(path)
        self.addCleanup(archive.close)
        return archive

    def test_utterances_follow_start_time_and_skip_punctuation_and_noises(self):
        m = "EN2002a"
        a = [word(m, "A", 0, 5.0, "The"), word(m, "A", 1, 5.2, "T_V_s"),
             f'<w nite:id="{m}.A.words2" starttime="5.4" endtime="5.4" punc="true">.</w>',
             f'<vocalsound nite:id="{m}.A.words3" starttime="5.4" endtime="6" type="laugh"/>',
             word(m, "A", 4, 9.0, "compo", trunc="true"), word(m, "A", 5, 9.3, "Okay")]
        b = [word(m, "B", 0, 1.0, "Hello"), word(m, "B", 1, 5.1, "Mm-hmm"),
             word(m, "B", 2, 30.0, "stray")]
        archive = self.build({
            f"words/{m}.A.words.xml": words_xml(m, "A", a),
            f"segments/{m}.A.segments.xml": segments_xml(m, "A", [(4.9, 6.0, 0, 3), (8.9, 9.6, 4, 5)]),
            f"words/{m}.B.words.xml": words_xml(m, "B", b),
            f"segments/{m}.B.segments.xml": segments_xml(m, "B", [(0.9, 1.3, 0, 0), (5.05, 5.3, 1, 1)]),
        })
        reference = meeting_reference(archive, m)
        self.assertEqual([row["words"] for row in reference["utterances"]],
                         [["Hello"], ["The", "TVs"], ["Mm-hmm"], ["compo", "Okay"], ["stray"]])
        self.assertEqual(reference["speakers"], ["A", "B"])
        self.assertEqual(reference["reference_words"], 7)
        self.assertEqual(reference["truncated_words"], 1)
        self.assertEqual(reference["words_outside_segments"], 1)
        self.assertEqual(reference_texts(reference, keep_fragments=False)[3], "Okay")
        # B's "Mm-hmm" (5.1-5.3 s) is fully inside A's words "The TVs" (5.0-5.4 s).
        self.assertEqual(reference["utterances"][2]["overlap_fraction"], 1.0)
        self.assertEqual(reference["utterances"][0]["overlap_fraction"], 0.0)

    def test_overlap_fraction_merges_overlapping_words(self):
        self.assertAlmostEqual(overlap_fraction(0.0, 10.0, [(1, 3), (2, 4), (9, 12)]), 0.4)
        self.assertEqual(overlap_fraction(0.0, 10.0, []), 0.0)
        self.assertEqual(overlap_fraction(5.0, 5.0, [(4, 6)]), 0.0)

    def test_rejects_entity_declarations(self):
        with self.assertRaises(ValueError):
            parse_xml(b'<?xml version="1.0"?><!DOCTYPE x [<!ENTITY a "aaaa">]><x>&a;</x>')

    def test_acronym_letters_are_joined_like_a_transcript(self):
        self.assertEqual(ami_orthography("L_C_D_'s"), "LCD's")
        self.assertEqual(ami_orthography("anti-R_S_I_"), "anti-RSI")
        self.assertEqual(ami_orthography("well"), "well")


class AlignmentTests(unittest.TestCase):
    def test_counts_each_error_kind(self):
        self.assertEqual(align("a b c".split(), "a b c".split())["errors"], 0)
        result = align("the cat sat on the mat".split(), "the bat sat the mat today".split())
        self.assertEqual((result["substitutions"], result["deletions"], result["insertions"]), (1, 1, 1))
        self.assertEqual(result["wer"], 3 / 6)
        self.assertEqual(align([], "a b".split())["insertions"], 2)
        self.assertEqual(align("a b".split(), [])["deletions"], 2)
        self.assertIsNone(align([], [])["wer"])
        self.assertEqual(alignment("the cat sat on".split(), "the bat sat".split())[1], ["C", "S", "C", "D"])

    def test_breakdown_attributes_errors_to_the_kind_of_utterance(self):
        reference = {"utterances": [
            {"start": 0.0, "end": 2.0, "overlap_fraction": 0.0},
            {"start": 2.0, "end": 4.0, "overlap_fraction": 0.5},
            {"start": 20.0, "end": 22.0, "overlap_fraction": 0.0}]}
        segments = [{"start": 0.0, "end": 5.0, "text": "unused"}]
        result = breakdown(reference, segments, ["C", "S", "D", "D", "D"], [0, 0, 1, 1, 2])
        self.assertEqual(result["clear"], {"reference_words": 2, "substitutions": 1, "deletions": 0})
        self.assertEqual(result["overlapped"], {"reference_words": 2, "substitutions": 0, "deletions": 2})
        self.assertEqual(result["no_saved_row"], {"reference_words": 1, "substitutions": 0, "deletions": 1})

    def test_matches_an_independent_edit_distance(self):
        generator = random.Random(7)
        for _ in range(200):
            ref = [generator.choice("abcde") for _ in range(generator.randrange(0, 30))]
            hyp = [generator.choice("abcdef") for _ in range(generator.randrange(0, 30))]
            result = align(ref, hyp)
            self.assertEqual(result["errors"], distance(ref, hyp))
            # Every reference word is matched, substituted or deleted; hypothesis words likewise.
            self.assertEqual(len(hyp), len(ref) - result["deletions"] + result["insertions"])


class NormalisationTests(unittest.TestCase):
    def test_fillers_numbers_and_punctuation_are_normalised_on_both_sides(self):
        self.assertEqual(normalise(["Mm-hmm. Um, it's twenty five euros, right-click."]),
                         normalise(["It is €25, right click"]))
        self.assertEqual(normalise(["The colour"]), ["the", "color"])


class PoolingTests(unittest.TestCase):
    def test_pooled_rate_weights_meetings_by_reference_words(self):
        kinds = {"clear": {"reference_words": 500, "substitutions": 7, "deletions": 5}}
        rows = [dict(reference_words=100, hypothesis_words=90, substitutions=5, deletions=10, insertions=0,
                     errors=15, audio_seconds=60.0, wall_seconds=6.0, wer=0.15, by_utterance_kind=kinds),
                dict(reference_words=900, hypothesis_words=900, substitutions=9, deletions=0, insertions=0,
                     errors=9, audio_seconds=540.0, wall_seconds=54.0, wer=0.01, by_utterance_kind=kinds)]
        total = pooled(rows)
        self.assertEqual(total["by_utterance_kind"]["clear"]["deletions"], 10)
        self.assertAlmostEqual(total["by_utterance_kind"]["clear"]["deletion_rate"], 0.01)
        self.assertAlmostEqual(total["by_utterance_kind"]["clear"]["share_of_reference_words"], 1.0)
        self.assertAlmostEqual(total["wer"], 24 / 1000)
        self.assertAlmostEqual(total["mean_meeting_wer"], 0.08)
        self.assertAlmostEqual(total["real_time_factor"], 0.1)

    def test_missing_and_failed_meetings_mark_the_condition_incomplete(self):
        reference = {"meeting": "M1", "truncated_words": 0,
                     "utterances": [{"start": 0.0, "end": 1.0, "overlap_fraction": 0.0,
                                     "words": ["hello", "there"], "truncated": [False, False]}]}
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / "M1.json").write_text(json.dumps({
                "status": "completed", "audio_seconds": 10.0, "wall_seconds": 1.0,
                "segments": [{"start": 0.0, "end": 1.0, "text": "Hello there."}]}))
            (folder / "M2.json").write_text(json.dumps({"status": "failed", "error": "decoder"}))
            result = score_condition({"M1": reference, "M2": reference, "M3": reference}, folder,
                                     ["M1", "M2", "M3"], jobs=1)
        self.assertFalse(result["complete"])
        self.assertEqual(result["missing"], ["M3"])
        self.assertEqual(result["failed"], [{"meeting": "M2", "error": "decoder"}])
        self.assertEqual(result["pooled"]["wer"], 0.0)


if __name__ == "__main__":
    unittest.main()
