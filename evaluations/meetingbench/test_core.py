import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
from datetime import datetime, timedelta, timezone
import urllib.error
import wave

from .gemini import AuthenticationRejected, BudgetExceeded, Gemini, Ledger, wav_bytes
from .plan import LANGUAGES, PROFILES, build_plan
from .metrics import transcript_score
from .batch_asr import bounded_inference
from .make_preview import overlap_stats, schedule


class DesignTests(unittest.TestCase):
    def test_exact_design_and_reproducibility(self):
        first = build_plan(18)
        self.assertEqual(first, build_plan(18))
        self.assertNotEqual(first["meetings"], build_plan(19)["meetings"])
        for difficulty, profile in PROFILES.items():
            meetings = [m for m in first["meetings"] if m["difficulty"] == difficulty]
            self.assertEqual(len(meetings), profile["count"])
            covered = {m["languages"][0] for m in meetings if len(m["languages"]) == 1 and m["dialect"] is None}
            self.assertEqual(covered, set(LANGUAGES))
        self.assertEqual(sum(len(m["languages"]) == 2 for m in first["meetings"]), 32)
        self.assertEqual(first["evaluation_models"]["asr"], ["qwen3-asr-1.7b"])
        self.assertEqual(first["evaluation_models"]["summary"], ["qwen3.5:4b"])

    def test_reference_actions_have_actual_source_turns(self):
        pilot = json.loads((Path(__file__).parent / "pilot_script.json").read_text())
        turns = {t["id"]: t for t in pilot["turns"]}
        people = {p["id"] for p in pilot["people"]}
        self.assertFalse(pilot["count_toward_200"])
        for fact in pilot["facts"]:
            self.assertTrue(fact["sources"])
            self.assertTrue(set(fact["sources"]) <= turns.keys())
            if fact["kind"] == "action":
                self.assertIn(fact["owner"], people)
                self.assertTrue(fact["due"].startswith("2026-06-"))


class BudgetTests(unittest.TestCase):
    def test_authentication_rejection_releases_only_rejected_usage(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger=Ledger(Path(folder)/'cost.sqlite',.31)
            first=ledger.reserve('speech','unauthenticated',100)
            with self.assertRaises(ValueError):
                ledger.reject_auth(first,{'code':503,'status':'UNAVAILABLE'})
            self.assertGreater(ledger.report()['accounted_usd'],0)
            ledger.reject_auth(first,{'code':401,'status':'UNAUTHENTICATED'})
            self.assertEqual(ledger.report()['accounted_usd'],0)
            self.assertEqual(ledger.pending(),0)
            second=ledger.reserve('speech','accepted',100)
            ledger.settle(second,20,100,{})
            ledger.reject_auth(second,{'code':401,'status':'UNAUTHENTICATED'})
            self.assertAlmostEqual(ledger.report()['accounted_usd'],.00182)

    def test_unknown_calls_keep_reservations_but_do_not_count_as_running(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger=Ledger(Path(folder)/'cost.sqlite',.31)
            call=ledger.reserve('speech','uncertain',100)
            self.assertEqual(ledger.pending(),1)
            amount=ledger.report()['accounted_usd']
            ledger.unknown(call)
            self.assertEqual(ledger.pending(),0)
            self.assertEqual(ledger.report()['accounted_usd'],amount)
            with self.assertRaises(BudgetExceeded):ledger.reserve('speech','next',100)

    def test_uncertain_requests_keep_their_full_reservation(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger = Ledger(Path(folder) / "cost.sqlite", .31)
            ledger.reserve("speech", "unknown", 100)
            with self.assertRaises(BudgetExceeded):
                Ledger(ledger.path, .31).reserve("speech", "retry", 100)

    def test_concurrent_workers_cannot_both_use_the_last_budget(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger = Ledger(Path(folder) / "cost.sqlite", .31)
            outcomes = []
            def reserve():
                try:
                    ledger.reserve("speech", "concurrent", 100)
                    outcomes.append("reserved")
                except BudgetExceeded:
                    outcomes.append("stopped")
            workers = [threading.Thread(target=reserve) for _ in range(6)]
            for worker in workers:
                worker.start()
            for worker in workers:
                worker.join()
            self.assertEqual(outcomes.count("reserved"), 1)
            self.assertEqual(outcomes.count("stopped"), 5)

    def test_completed_usage_releases_only_unused_reservation(self):
        with tempfile.TemporaryDirectory() as folder:
            ledger = Ledger(Path(folder) / "cost.sqlite", .31)
            first = ledger.reserve("speech", "first", 100)
            ledger.settle(first, 20, 100, {})
            self.assertAlmostEqual(ledger.report()["accounted_usd"], .00182)
            ledger.reserve("speech", "second", 100)


class AuthenticationTests(unittest.TestCase):
    def client(self,folder):
        return Gemini({'GOOGLE_CLOUD_PROJECT':'synthetic-test'},Path(folder),.7)

    def test_token_expiry_is_respected_and_refresh_is_shared(self):
        with tempfile.TemporaryDirectory() as folder:
            client=self.client(folder)
            expiry=(datetime.now(timezone.utc)+timedelta(minutes=12)).isoformat()
            result=json.dumps({'credential':{'access_token':'test-token','token_expiry':expiry}})
            with patch('evaluations.meetingbench.gemini.subprocess.check_output',return_value=result) as run:
                self.assertEqual(client.access_token(),'test-token')
                self.assertEqual(client.access_token(),'test-token')
            self.assertEqual(run.call_count,1)
            self.assertIn('--force-auth-refresh',run.call_args.args[0])
            import time
            self.assertLess(client.token_until-time.monotonic(),8*60)

    def test_authentication_retry_is_bounded_and_does_not_charge_rejections(self):
        with tempfile.TemporaryDirectory() as folder:
            client=self.client(folder)
            body=json.dumps({'error':{'code':401,'status':'UNAUTHENTICATED'}}).encode()
            def rejected(*args,**kwargs):
                raise urllib.error.HTTPError('https://aiplatform.googleapis.com',401,'Unauthorized',{},io.BytesIO(body))
            with patch.object(client,'access_token',return_value='rejected'), patch.object(client.opener,'open',side_effect=rejected) as call:
                with self.assertRaises(AuthenticationRejected):client.request('speech','test','unused',{})
            self.assertEqual(call.call_count,2)
            self.assertEqual(client.ledger.report()['accounted_usd'],0)

    def test_server_failures_are_not_retried_or_released(self):
        with tempfile.TemporaryDirectory() as folder:
            client=self.client(folder)
            error=urllib.error.HTTPError('https://aiplatform.googleapis.com',503,'Unavailable',{},io.BytesIO(b'{}'))
            with patch.object(client,'access_token',return_value='test'), patch.object(client.opener,'open',side_effect=error) as call:
                with self.assertRaises(RuntimeError):client.request('speech','test','unused',{})
            self.assertEqual(call.call_count,1)
            self.assertGreater(client.ledger.report()['accounted_usd'],0)


class AudioTests(unittest.TestCase):
    def test_voice_design_pcm_gets_one_valid_header(self):
        audio = wav_bytes(b"\0\0" * 24000, "audio/l16; rate=24000; channels=1")
        self.assertEqual(wav_bytes(audio, "audio/wav"), audio)
        with wave.open(io.BytesIO(audio)) as reader:
            self.assertEqual(reader.getnframes(), 24000)

    def test_wrong_rate_is_not_silently_relabelled(self):
        output = io.BytesIO()
        with wave.open(output, "wb") as writer:
            writer.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            writer.writeframes(b"\0\0" * 16000)
        with self.assertRaises(ValueError):
            wav_bytes(output.getvalue(), "audio/wav")


class TextMetricTests(unittest.TestCase):
    def test_summary_judge_must_quote_the_actual_summary(self):
        from .judge_corpus import parse_review, aggregate
        facts=[{'id':'D1','statement':'The ceiling is 320 euros.'}]
        good=parse_review('{"items":[{"id":"D1","verdict":"covered","quote":"320 euros"}]}',facts,'The approved ceiling is 320 euros.')
        self.assertEqual(aggregate(good)['diagnostic_band'],'excellent')
        fabricated=parse_review('{"items":[{"id":"D1","verdict":"covered","quote":"320 euros"}]}',facts,'The ceiling is unspecified.')
        self.assertEqual(fabricated[0]['verdict'],'unverified')
        self.assertEqual(aggregate(fabricated)['diagnostic_band'],'needs_review')
        with self.assertRaises(ValueError):
            parse_review('{"items":[]}',facts,'The ceiling is unspecified.')

    def test_valid_loanword_script_variants_keep_strict_errors_visible(self):
        score = transcript_score("Friday customers confirmed plan", "फ्राइडे कस्टमर्स कंफर्म्ड प्लान", ["hi", "en"])
        self.assertEqual(score["strict_wer"], 1)
        self.assertEqual(score["orthographic_wer"], 0)

    def test_changed_numbers_and_missing_negation_stay_errors(self):
        score = transcript_score("not approved for ८००", "approved for 1800", ["hi", "en"])
        self.assertGreater(score["orthographic_wer"], 0)
        self.assertEqual(transcript_score("८००", "800", ["hi"])["strict_wer"], 0)

    def test_failed_jobs_are_not_excluded_from_error_metrics(self):
        score = transcript_score("one two three", None, ["en"])
        self.assertEqual(score["strict_wer"], 1)
        self.assertEqual(score["diagnostic_band"], "failed")

    def test_unsegmented_language_uses_character_errors(self):
        score = transcript_score("不能发布", "不能", ["zh"])
        self.assertEqual(score["primary_metric"], "cer")
        self.assertEqual(score["cer"], .5)


class BatchRecoveryTests(unittest.TestCase):
    def test_one_bad_window_does_not_erase_other_meetings(self):
        def predict(items):
            if "broken" in items:
                raise RuntimeError("bad audio window")
            return [item.upper() for item in items]
        results = bounded_inference(predict, ["first", "broken", "third"], 3)
        self.assertEqual(results[0], ("FIRST", None))
        self.assertIsNone(results[1][0])
        self.assertEqual(results[2], ("THIRD", None))

    def test_memory_failure_reduces_batch_without_reordering(self):
        cleared = []
        def predict(items):
            if len(items) > 2:
                raise RuntimeError("CUDA out of memory")
            return items
        results = bounded_inference(predict, list(range(7)), 7, lambda: cleared.append(True))
        self.assertEqual(results, [(i, None) for i in range(7)])
        self.assertTrue(cleared)


class PreviewTests(unittest.TestCase):
    def test_all_corpus_scripts_have_real_participants_and_cited_final_facts(self):
        from .author_corpus import build_script, validate_script
        from .plan import build_plan, LANGUAGES
        from .language_packs import PACKS
        from .topics import TOPICS
        self.assertEqual(set(PACKS), set(LANGUAGES))
        self.assertEqual(set(TOPICS), set(LANGUAGES))
        digests=set()
        for spec in build_plan()['meetings']:
            script=build_script(spec)
            validate_script(script)
            self.assertNotIn(script['script_sha256'], digests)
            digests.add(script['script_sha256'])
            rows={t['id']:t for t in script['turns']}
            for fact in script['facts']:
                if fact['kind']=='action':
                    self.assertEqual(rows[fact['sources'][0]]['speaker'],fact['owner'])
            _,spans=schedule(script['turns'],[bytes(96000)]*len(script['turns']),.8,script['max_concurrent_speakers'])
            active=[]
            for turn,(start,end) in zip(script['turns'],spans):
                active=[entry for entry in active if entry[1]>start]
                for other,other_end in active:
                    self.assertEqual(turn.get('emotion'),'angry')
                    self.assertEqual(other.get('emotion'),'angry')
                    self.assertTrue(turn.get('group'))
                    self.assertEqual(turn['group'],other['group'])
                active.append((turn,end))

    def test_reference_names_do_not_mislabel_merged_voices(self):
        from .install_preview import reference_display_names
        reference = {"people": [{"id": "a", "name": "Alice"}, {"id": "b", "name": "Bob"}],
                     "turns": [{"speaker": "a", "start_seconds": 0, "end_seconds": 10},
                               {"speaker": "b", "start_seconds": 10, "end_seconds": 20}]}
        self.assertEqual(reference_display_names(reference, [("merged", 0, 20)]), [])
        labels = reference_display_names(reference, [("voice-a", 0, 10), ("voice-b", 10, 20)])
        self.assertEqual([label["name"] for label in labels], ["Alice", "Bob"])
        self.assertTrue(all("not automatic" in label["source"] for label in labels))

    def test_authored_preview_sources_and_simultaneous_speakers_are_valid(self):
        script = json.loads((Path(__file__).parent / "very_hard_example.json").read_text())
        people = {person["id"] for person in script["people"]}
        turns = script["turns"]
        self.assertEqual(len(people), 6)
        self.assertTrue(all(turn["speaker"] in people for turn in turns))
        turn_ids = {turn["id"] for turn in turns}
        self.assertEqual(len(turn_ids), len(turns))
        self.assertTrue(all(set(fact["sources"]) <= turn_ids for fact in script["facts"]))
        _starts, intervals = schedule(turns, [bytes(48000)] * len(turns), .5)
        self.assertLessEqual(overlap_stats(intervals)["peak_speakers"], 4)
        self.assertGreater(overlap_stats(intervals)["overlap_fraction_of_speech_time"], 0)
        self.assertLess(overlap_stats(intervals)["overlap_fraction_of_speech_time"], .1)
        for i, (left, right) in enumerate(intervals):
            for j in range(i):
                if max(left, intervals[j][0]) < min(right, intervals[j][1]):
                    self.assertEqual(turns[i].get('group'), turns[j].get('group'))
                    self.assertEqual(turns[i]['emotion'], 'angry')
                    self.assertEqual(turns[j]['emotion'], 'angry')
        ends = {}
        for turn, (start, end) in zip(turns, intervals):
            self.assertGreaterEqual(start, ends.get(turn["speaker"], 0))
            ends[turn["speaker"]] = end

    def test_calm_conversation_never_overlaps_even_at_high_intensity(self):
        turns=[dict(speaker=str(i%3),emotion='calm',group='discussion') for i in range(12)]
        _,intervals=schedule(turns,[bytes(240000)]*len(turns),1)
        self.assertEqual(overlap_stats(intervals)['overlap_seconds'],0)

    def test_angry_interruptions_begin_late_and_end_before_the_next_topic(self):
        turns=[dict(speaker='A',emotion='angry',group='argument'),
               dict(speaker='B',emotion='angry',group='argument'),
               dict(speaker='C',emotion='calm')]
        starts,intervals=schedule(turns,[bytes(240000)]*3,.8)
        overlap=intervals[0][1]-intervals[1][0]
        self.assertGreater(overlap,0)
        self.assertLessEqual(overlap,1.15)
        self.assertGreater(starts[1]-starts[0],8)
        self.assertGreaterEqual(starts[2],max(end for _,end in intervals[:2]))


if __name__ == "__main__":
    unittest.main()
