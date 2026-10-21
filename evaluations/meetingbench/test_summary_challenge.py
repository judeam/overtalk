import hashlib
import tempfile
import unittest
from pathlib import Path

from .summary_challenge import cases, prepare, report_units, validate_review, wilson_lower, lock_candidate


class SummaryChallengeTests(unittest.TestCase):
    def test_partial_or_failed_preholdout_cannot_unlock_holdout(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory); (root/'notes').mkdir(); (root/'direct-review').mkdir()
            config=dict(cases=['dev-budget'],models=['glm-5.3-flash'],prompt_policy='grounded')
            (root/'configuration.json').write_text(json.dumps(config))
            with self.assertRaises(ValueError):lock_candidate(root,root/'lock.json')
            config['cases']=[c['id'] for c in cases() if c['split']!='holdout']
            (root/'configuration.json').write_text(json.dumps(config))
            case=next(c for c in cases() if c['id']=='dev-budget')
            text='The budget remains unresolved.'
            stem='dev-budget__glm-5.3-flash__app'
            (root/'notes'/f'{stem}.md').write_text(text)
            review=dict(report_sha256=hashlib.sha256(text.encode()).hexdigest(),
                facts=[dict(id=f['id'],status='missing',reason='Not recovered') for f in case['facts']],
                claims=[dict(id='C1',text=text,status='unsupported',reason='Budget was explicitly agreed',evidence=[3])],
                complete_claim_inventory=True,citation_errors=[])
            (root/'direct-review'/f'{stem}.json').write_text(json.dumps(review))
            with self.assertRaises(ValueError):lock_candidate(root,root/'lock.json')
            self.assertFalse((root/'lock.json').exists())

    def test_small_perfect_sample_does_not_establish_population_perfection(self):
        self.assertIsNone(wilson_lower(0,0))
        self.assertLess(wilson_lower(18,18),0.83)
        self.assertGreater(wilson_lower(18,18),0.80)

    def test_independent_splits_and_grounded_references(self):
        values=cases()
        self.assertEqual(len(values),18)
        self.assertEqual(len({c['transcript'] for c in values}),18)
        self.assertEqual(len({c['domain'] for c in values}),18)
        for split in ('development','validation','holdout'):
            rows=[c for c in values if c['split']==split]
            self.assertEqual(len(rows),6)
            for difficulty in ('normal','hard','very_hard'):
                self.assertEqual(sum(c['difficulty']==difficulty for c in rows),2)

    def test_confirmation_is_separate_with_valid_sources(self):
        values=cases(Path(__file__).with_name('summary_confirmation'))
        self.assertEqual(len(values),6)
        self.assertFalse({c['id'] for c in values}&{c['id'] for c in cases()})
        self.assertEqual({c['split'] for c in values},{'holdout'})
        for difficulty in ('normal','hard','very_hard'):
            self.assertEqual(sum(c['difficulty']==difficulty for c in values),2)

    def test_inputs_never_include_reference_answers(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'frozen'
            prepare(root)
            for path in (root/'inputs').glob('*.json'):
                self.assertEqual(set(json.loads(path.read_text())),{'transcript','language'})
            with self.assertRaises(FileExistsError): prepare(root)

    def review(self, report):
        case=dict(facts=[dict(id='F1')])
        review=dict(report_sha256=hashlib.sha256(report.encode()).hexdigest(),
            facts=[dict(id='F1',status='covered',reason='Explicit source')],claims=[dict(**u,status='supported',reason='Explicit source',evidence=[1]) for u in report_units(report)],
            complete_claim_inventory=True,citation_errors=[])
        return case,review

    def test_hallucination_fails_even_with_perfect_recall(self):
        report='# Notes\n- The launch is conditional. [S1]\n- Sam will notify clients tomorrow. [S2]'
        case,review=self.review(report)
        self.assertTrue(validate_review(case,report,review)['passed'])
        review['claims'][1]['status']='unsupported'
        self.assertFalse(validate_review(case,report,review)['passed'])

    def test_missing_ambiguous_and_wrong_citations_never_pass(self):
        report='The launch is conditional. [S1]'
        for change in ('partial','missing','contradicted'):
            case,review=self.review(report);review['facts'][0]['status']=change
            self.assertFalse(validate_review(case,report,review)['passed'])
        case,review=self.review(report);review['claims'][0]['status']='uncertain'
        self.assertFalse(validate_review(case,report,review)['passed'])
        case,review=self.review(report);review['citation_errors']=['S1 does not support claim']
        self.assertFalse(validate_review(case,report,review)['passed'])

    def test_stale_or_incomplete_review_rejected(self):
        report='The launch is conditional.'
        case,review=self.review(report)
        with self.assertRaises(ValueError):validate_review(case,report+' changed',review)
        for key in ('facts','claims'):
            case,review=self.review(report);review[key]=[]
            with self.assertRaises(ValueError):validate_review(case,report,review)
        case,review=self.review(report);case['id']='expected-case';review['case']='other-case'
        with self.assertRaises(ValueError):validate_review(case,report,review)

    def test_inventory_includes_factual_metadata_and_action_rows(self):
        units=report_units('# Title\nDate: Thursday\n## Actions\n| Task | Owner | Due |\n|---|---|---|\n| Ship | Sam | Friday |\n> Launch is conditional.')
        self.assertEqual(len(units),3)
        self.assertEqual(units[1]['text'],'| Ship | Sam | Friday |')


if __name__=='__main__': unittest.main()
