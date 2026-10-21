import unittest

from .evaluation_insights import compare_results,speed_comparison,markdown_report


class InsightsTests(unittest.TestCase):
    def test_empty_grading_report_does_not_claim_perfect_notes(self):
        text=markdown_report(dict(observed=0,expected=3,groups=[],worst_language_groups={}),{},dict(regressions=[]))
        self.assertIn('Not established',text)
        self.assertIn('0 / 0',text)
        self.assertNotIn('100%',text)

    def test_resume_cannot_be_presented_as_a_fresh_speedup(self):
        self.assertIsNone(speed_comparison(1000,1,baseline_cases=3,candidate_cases=3,resumed=True)['matched_speedup'])
        self.assertFalse(speed_comparison(1000,1,baseline_cases=3,candidate_cases=3,resumed=True)['tenfold_demonstrated'])

    def test_tenfold_requires_measured_time_not_a_projection(self):
        self.assertTrue(speed_comparison(1000,100,baseline_cases=3,candidate_cases=3)['tenfold_demonstrated'])
        self.assertFalse(speed_comparison(1000,101,baseline_cases=3,candidate_cases=3)['tenfold_demonstrated'])
        partial=speed_comparison(1000,50,baseline_cases=2,candidate_cases=3)
        self.assertIsNone(partial['matched_speedup'])
        self.assertEqual(partial['lower_bound_speedup'],20)
        failed=speed_comparison(1000,1,baseline_cases=3,candidate_cases=3,completed_cases=2)
        self.assertFalse(failed['tenfold_demonstrated'])
        self.assertIsNone(failed['matched_speedup'])
        ungraded=speed_comparison(1000,1,baseline_cases=3,candidate_cases=3,validation_complete=False)
        self.assertFalse(ungraded['tenfold_demonstrated'])
        self.assertIsNone(ungraded['matched_speedup'])

    def test_accuracy_regressions_remain_visible_and_changed_references_fail(self):
        row=dict(id='x',status='completed',languages=['en'],difficulty='hard',
                 transcript=dict(reference_words=100,word_errors=5))
        candidate=dict(row,transcript=dict(reference_words=100,word_errors=6))
        result=compare_results([row],[candidate])
        self.assertEqual(result['regressions'][0]['extra_errors'],1)
        candidate['transcript']['reference_words']=99
        with self.assertRaises(ValueError):compare_results([row],[candidate])
