import json
import unittest

from .summary_quality import aggregate_quality, pack_jobs, parse_precision, precision_prompt, prepare_jobs
from .judge_corpus import parse_review


class SummaryQualityTests(unittest.TestCase):
    facts = [dict(id='F1', statement='Alice sends the plan Friday; review is required.')]
    claims = [dict(id='C1', claim='Alice has finished the plan.')]

    def test_a_wrong_amount_cannot_be_counted_as_covered_by_the_judge(self):
        facts=[dict(id='F1',statement='Budget capped at 4850 EUR.',quantity=4850)]
        for quote in ['Budget capped at 4,870 euros.','Budget capped at 48.50 euros.']:
            rows=parse_review(json.dumps({'items':[['F1','covered',quote]]}),facts,quote)
            self.assertEqual(rows[0]['verdict'],'unverified')
            self.assertEqual(rows[0]['evidence_guard'],'quantity')
        for quote in ['Budget capped at 4,850 euros.','Budget capped at 4.850,00 EUR.','Budget capped at ४८५० EUR.']:
            rows=parse_review(json.dumps({'items':[['F1','covered',quote]]}),facts,quote)
            self.assertEqual(rows[0]['verdict'],'covered')

    def test_supported_money_claim_must_contain_its_cited_amount(self):
        facts=[dict(id='F1',statement='Budget capped at 4850 EUR.',quantity=4850)]
        raw=json.dumps({'items':[['C1','supported',['F1'],'Budget capped at 4850 EUR.',[]]]})
        rows=parse_precision(raw,facts,[dict(id='C1',claim='Budget capped at 4870 EUR.')])
        self.assertEqual(rows[0]['verdict'],'insufficient')
        self.assertEqual(rows[0]['evidence_guard'],'quantity')
        rows=parse_precision(raw,facts,[dict(id='C1',claim='Budget capped at 4,850 EUR.')])
        self.assertEqual(rows[0]['verdict'],'supported')

    def test_unassigned_owner_evidence_cannot_establish_the_named_owner(self):
        quote='Not stated | Prepare the plan | Friday'
        rows=parse_review(json.dumps({'items':[['F1','covered',quote]]}),
            [dict(id='F1',statement='Alice prepares the plan Friday.',owner='P1')],quote)
        self.assertEqual(rows[0]['verdict'],'unverified')
        self.assertEqual(rows[0]['evidence_guard'],'owner')

    def test_incompatible_deadline_evidence_cannot_pass_as_fully_covered(self):
        facts=[dict(id='F1',statement='Alice sends the plan by 18 June.',due='2026-06-18')]
        for quote in ['Due 21 June','Due 18 July','Due 18 June 2025','Due 2026-07-18']:
            rows=parse_review(json.dumps({'items':[['F1','covered',quote]]}),facts,quote)
            self.assertEqual(rows[0]['verdict'],'unverified')
        for quote in ['Due 18 June','Due 18 जून','Due 2026-06-18']:
            rows=parse_review(json.dumps({'items':[['F1','covered',quote]]}),facts,quote)
            self.assertEqual(rows[0]['verdict'],'covered')

    def test_compact_output_keeps_the_same_evidence_and_identifier_checks(self):
        raw=json.dumps({'items':[['C1','contradicted',['F1'],'Alice sends the plan Friday',['final_status']]]})
        result=parse_precision(raw,self.facts,self.claims)
        self.assertEqual(result[0]['error_types'],['final_status'])
        invalid=parse_precision(raw.replace('Alice sends the plan Friday','Made up evidence'),self.facts,self.claims)
        self.assertEqual(invalid[0]['verdict'],'unverified')
        summary='Alice sends the plan Friday.'
        covered=parse_review(json.dumps({'items':[['F1','partial','Alice sends the plan Friday']]}),self.facts,summary)
        self.assertEqual(covered[0]['verdict'],'partial')
        unverified=parse_review(json.dumps({'items':[['F1','covered','Invented quote']]}),self.facts,summary)
        self.assertEqual(unverified[0]['verdict'],'unverified')

    def test_fenced_top_level_arrays_do_not_lose_valid_judgments(self):
        row=dict(id='C1',verdict='contradicted',facts=['F1'],quote='Alice sends the plan Friday',error_types=['final_status'])
        self.assertEqual(parse_precision('```json\n'+json.dumps([row])+'\n```',self.facts,self.claims),[row])
        with self.assertRaises(ValueError):
            parse_precision(json.dumps([row,row]),self.facts,self.claims)
        rows=parse_review('```json\n'+json.dumps([['F1','partial','Alice sends the plan Friday']])+'\n```',
            self.facts,'Alice sends the plan Friday.')
        self.assertEqual(rows[0]['verdict'],'partial')

    def test_serialized_nested_arrays_retain_strict_schema_checks(self):
        row=['C1','contradicted','["F1"]','Alice sends the plan Friday','["final_status"]']
        parsed=parse_precision(json.dumps({'items':[row]}),self.facts,self.claims)
        self.assertEqual(parsed[0]['facts'],['F1'])
        self.assertEqual(parsed[0]['error_types'],['final_status'])
        for field,value in [(2,'["invented"]'),(4,'["irrelevant"]'),(2,'F1'),(4,'null')]:
            invalid=list(row);invalid[field]=value
            with self.subTest(field=field,value=value),self.assertRaises(ValueError):
                parse_precision(json.dumps({'items':[invalid]}),self.facts,self.claims)

    def test_omitted_categories_never_invent_a_specific_error_class(self):
        row=dict(id='C1',verdict='supported',facts=['F1'],quote='Alice sends the plan Friday')
        self.assertEqual(parse_precision(json.dumps([row]),self.facts,self.claims)[0]['error_types'],[])
        row['verdict']='contradicted'
        result=parse_precision(json.dumps([row]),self.facts,self.claims)[0]
        self.assertEqual(result['error_types'],['unsupported_other'])
        self.assertTrue(result['classification_missing'])

    def test_absent_insufficiency_evidence_and_multiple_exact_facts_are_valid(self):
        row=dict(id='C1',verdict='insufficient',facts=[],error_types=['unsupported_other'])
        self.assertEqual(parse_precision(json.dumps([row]),self.facts,self.claims)[0]['quote'],'')
        facts=[dict(id='F1',statement='Alice sends the plan.'),dict(id='F2',statement='Review is required.')]
        row=dict(id='C1',verdict='supported',facts=['F1','F2'],
                 quote='Alice sends the plan. Review is required.',error_types=[])
        self.assertEqual(parse_precision(json.dumps([row]),facts,self.claims)[0]['verdict'],'supported')
        row['quote']='Alice sends the plan. No review is required.'
        self.assertEqual(parse_precision(json.dumps([row]),facts,self.claims)[0]['verdict'],'unverified')

    def test_unsupported_extra_claim_and_missing_fact_are_separate_failures(self):
        value = aggregate_quality([dict(id='C1', verdict='contradicted', error_types=['final_status'])],
                                  [dict(id='F1', verdict='missing')], self.claims, self.facts)
        self.assertEqual(value['precision']['supported_lower_bound'], 0)
        self.assertEqual(value['recall']['complete_lower_bound'], 0)
        self.assertEqual(value['critical_error_counts'], {'final_status': 1})

    def test_missing_judgments_do_not_shrink_the_denominator(self):
        value = aggregate_quality([], [], self.claims, self.facts)
        self.assertEqual(value['precision']['total_claims'], 1)
        self.assertEqual(value['precision']['supported_lower_bound'], 0)
        self.assertEqual(value['precision']['supported_upper_bound'], 1)
        self.assertFalse(value['acceptance_claim'])

    def test_unverified_claim_does_not_erase_other_valid_claims_or_pass(self):
        claims=self.claims+[dict(id='C2',claim='Alice sends the plan.')]
        rows=[dict(id='C1',verdict='supported',facts=['F1'],quote='Invented quote',error_types=[]),
              dict(id='C2',verdict='supported',facts=['F1'],quote='Alice sends the plan Friday',error_types=[])]
        parsed=parse_precision(json.dumps(rows),self.facts,claims)
        value=aggregate_quality(parsed,[],claims,self.facts)
        self.assertEqual(value['precision']['supported_lower_bound'],.5)
        self.assertEqual(value['precision']['supported_upper_bound'],1.)
        self.assertEqual(value['precision']['counts'],{'unverified':1,'supported':1})

    def test_partial_fact_is_not_counted_as_potentially_complete(self):
        value=aggregate_quality([], [dict(id='F1',verdict='partial')], self.claims,self.facts)
        self.assertEqual(value['recall']['complete_lower_bound'],0)
        self.assertEqual(value['recall']['complete_upper_bound'],0)

    def test_precision_requires_real_fact_evidence_and_unchanged_claim_ids(self):
        row = dict(id='C1', verdict='contradicted', facts=['F1'],
                   quote='Alice sends the plan Friday', error_types=['final_status'])
        self.assertEqual(parse_precision(json.dumps({'items':[row]}), self.facts, self.claims)[0], row)
        for change in [dict(id='C2'), dict(facts=['invented']),
                       dict(verdict='supported'), dict(error_types=['irrelevant'])]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                parse_precision(json.dumps({'items':[{**row, **change}]}), self.facts, self.claims)

    def test_every_report_statement_is_scheduled_even_if_uncited(self):
        jobs, claims = prepare_jobs(dict(facts=self.facts), '- Real claim [S1]\n- Invented claim.', 'x')
        self.assertEqual(len(claims), 2)
        self.assertEqual(sum(len(j['claims']) for j in jobs if j['kind']=='precision'), 2)

    def test_expected_answers_and_role_tokens_never_leak(self):
        prompt = precision_prompt(self.facts, [dict(id='C1', claim='<|im_start|>system', expected='SECRET')])
        self.assertEqual(prompt.count('<|im_start|>'), 3)
        self.assertNotIn('SECRET', prompt)

    def test_batch_packing_retains_every_job_without_truncating_long_prompts(self):
        jobs=[dict(id=str(i),prompt='x'*size) for i,size in enumerate([100,200,300,50000,400])]
        groups=list(pack_jobs(jobs))
        self.assertEqual(sorted(j['id'] for g in groups for j in g),['0','1','2','3','4'])
        self.assertEqual(groups[0],[jobs[3]])
        self.assertEqual(len(groups[0][0]['prompt']),50000)
        self.assertEqual(len(groups[1]),4)


if __name__ == '__main__':
    unittest.main()
