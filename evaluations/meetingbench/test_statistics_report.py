import unittest

from .statistics_report import build_report, paired_difference, ratio_summary, wilson_interval


class StatisticsTests(unittest.TestCase):
    def rows(self):
        return [dict(id=str(i), numerator=i, denominator=100, scenario=str(i // 2)) for i in range(10)]

    def test_bootstrap_is_repeatable_and_resamples_whole_meetings(self):
        a = ratio_summary(self.rows(), samples=200)
        self.assertEqual(a, ratio_summary(self.rows(), samples=200))
        self.assertEqual(a['rate'], .045)
        self.assertEqual(a['cases'], 10)
        self.assertLess(a['bootstrap_95'][0], .045)
        self.assertGreater(a['bootstrap_95'][1], .045)
        self.assertEqual(ratio_summary(self.rows(), samples=200, cluster='scenario')['clusters'], 5)

    def test_small_samples_do_not_get_precise_looking_bootstrap_intervals(self):
        self.assertIsNone(ratio_summary(self.rows()[:2])['bootstrap_95'])
        interval = wilson_interval(2, 2)
        self.assertLess(interval[0], .5)
        self.assertIsNone(wilson_interval(0, 0))

    def test_missing_metrics_and_missing_meetings_remain_visible(self):
        meta = [dict(id=str(i), languages=['en'], difficulty='hard') for i in range(3)]
        observed = [dict(**meta[0], metrics={'wer': dict(numerator=0, denominator=100)}),
                    dict(**meta[1], metrics={})]
        report = build_report(observed, meta, samples=200)
        overall = next(g for g in report['groups'] if g['dimension'] == 'all')
        self.assertEqual(overall['missing_or_ungraded'], ['1', '2'])
        self.assertEqual(report['missing'], ['2'])
        self.assertFalse(report['acceptance_claim'])

    def test_paired_comparison_requires_unchanged_reference_and_reports_missing_cases(self):
        old = self.rows()
        new = [dict(row, numerator=0) for row in old[:-1]]
        result = paired_difference(old, new, samples=200)
        self.assertLess(result['delta'], 0)
        self.assertEqual(result['only_before'], ['9'])
        new[0]['denominator'] = 99
        with self.assertRaises(ValueError):
            paired_difference(old, new)

    def test_invalid_counts_are_rejected_but_errors_above_one_hundred_percent_are_retained(self):
        for value in [-1, float('nan'), float('inf')]:
            with self.assertRaises(ValueError):
                ratio_summary([dict(id='a', numerator=value, denominator=2)])
        self.assertEqual(ratio_summary([dict(id='a', numerator=3, denominator=2)])['rate'], 1.5)

    def test_word_and_character_rates_and_mixed_languages_stay_separate(self):
        rows = [dict(id='a', languages=['en'], difficulty='normal', metrics={'wer': dict(numerator=2, denominator=10)}),
                dict(id='b', languages=['en', 'zh'], difficulty='normal', metrics={'cer': dict(numerator=1, denominator=100)})]
        report = build_report(rows, samples=200)
        self.assertEqual(set(report['worst_language_groups']), {'wer', 'cer'})
        self.assertEqual(report['worst_language_groups']['cer'][0]['value'], 'en+zh')


if __name__ == '__main__':
    unittest.main()
