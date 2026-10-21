import unittest

from .claim_packets import build_packets


class ClaimPacketTests(unittest.TestCase):
    def test_structural_discussion_heading_is_not_a_factual_claim(self):
        packet=build_packets('## Discussion Highlights\n\n- The deadline is Friday. [S1]',
                             '[S1] The deadline is Friday.')
        self.assertEqual(packet['total_statements'],1)
        self.assertEqual(packet['claims'][0]['claim'],'The deadline is Friday. [S1]')

    def test_a_claim_in_a_heading_is_not_silently_excluded(self):
        packet=build_packets('# Meeting notes\n\n## Alice approved the launch [S1]\n\n## Decisions\n',
                             '[S1] Alice said the launch needs review.')
        self.assertEqual(packet['total_statements'],1)
        self.assertIn('Alice approved',packet['claims'][0]['claim'])

    def test_extra_uncited_assertions_stay_in_the_denominator(self):
        packet = build_packets('# Notes\n\n- Approved after review. [S1]\n- Everything is finished.',
                               '[S1] Review is still required.')
        self.assertEqual(packet['total_statements'], 2)
        self.assertEqual(len(packet['jobs']), 1)
        self.assertEqual(packet['ungraded'][0]['reason'], 'missing_citation')

    def test_action_table_retains_owner_task_and_deadline(self):
        text = '| Owner | Task | Due | Source | Time |\n| --- | --- | --- | --- | --- |\n| Alice | Send the plan | Friday | [S2] | 00:02 |'
        packet = build_packets(text, '[S2] [Alice] I will send it Friday.')
        claim = packet['claims'][0]['claim']
        self.assertIn('Owner: Alice; Task: Send the plan; Due: Friday', claim)
        self.assertEqual(packet['jobs'][0]['sources'][0]['id'], 'S2')
        self.assertNotIn('00:02', claim)

    def test_missing_or_invented_citations_never_become_a_pass(self):
        packet = build_packets('A valid detail. [S1]\n\nAn invented detail. [S99]', '[S1] Actual words.')
        self.assertEqual(packet['total_statements'], 2)
        self.assertEqual(packet['ungraded'][0]['reason'], 'unknown_citation')

    def test_long_passages_are_ungraded_without_silent_truncation(self):
        packet = build_packets('Claim. [S1]', '[S1] ' + '文' * 6000)
        self.assertEqual(packet['total_statements'], 1)
        self.assertEqual(packet['jobs'], [])
        self.assertEqual(packet['ungraded'][0]['reason'], 'passage_exceeds_verifier_budget')

    def test_duplicate_source_ids_and_broken_tables_are_rejected(self):
        with self.assertRaises(ValueError):
            build_packets('Claim [S1]', '[S1] A\n[S1] B')
        with self.assertRaises(ValueError):
            build_packets('| Owner | Task |\n| --- | --- |\n| Alice |', '[S1] A')


if __name__ == '__main__':
    unittest.main()
