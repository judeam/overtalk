# Small-model summary regression conversations

These are newly authored synthetic text controls, not recordings or corpus score
claims. They intentionally differ from the repeated agenda template in the 200
meeting evaluation. The native notes probe receives only transcript and language;
this checklist is for direct review and must never enter the model prompt.

- `corrected-task`: Leah owns the migration plan, due **Monday**. Thursday was
  replaced. Friday is the customer demo, not the plan deadline. Owen reviews and
  is not the plan owner. Migration is conditional on a security review; staff
  training remains an open question.
- `conditional-scope`: 46 invoices have errors, but **no invoice count** has been
  approved as scope. Reduce project scope; review before proceeding. The cap is
  **800 EUR**, not 2400. Marta owns the corrected plan, due **22 de junio**;
  Diego helps. Client delivery has no confirmed date; external help is unresolved.
- `no-committed-deadline`: Noah owns the accessibility report and has **no agreed
  deadline**. 12 November is the workshop and was rejected as the report deadline.
  Sending invitations is completed work. Publication requires participant consent;
  sharing an anonymized version is unresolved.

Run using `python -m evaluations.meetingbench.probe_topic_notes` with this folder
as `--inputs`, the three case names as `--cases`, a fresh runtime output directory,
and the real native test runner/helper.
Review all output statements and omissions, not just the checklist matches.
