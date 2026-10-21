# Privacy deployment and operating checklist

This document records work the application cannot perform on an organization's
behalf. It is an operating checklist, not a claim of legal compliance.

## Before enabling a deployment

Identify the legal operator/controller, contact for privacy requests, countries
of users and recorded participants, whether clinical/patient data will be used,
the recording purpose and applicable lawful bases. Provide participants a notice
before capture, including any voice identification, external AI processing,
recipients, retention and request/contact process. Check applicable recording
laws for all participants; an operator clicking Continue is not their consent.

Choose a retention period by purpose. Document legal holds and their review
date. Keep automatic deletion off until the configured period and backup policy
have been reviewed. Schedule real restore tests, including restoring an older
backup after an erasure or consent withdrawal. Manage external exported copies
and backups separately; Overtalk cannot erase someone else's copy.

For sensitive deployments, use an encrypted, updated device, private OS account,
automatic screen lock, least-privilege administration, encrypted backups and a
controlled recovery process. Protected mode requires the app to verify device
encryption for configured storage and temporary files; unknown means unverified.
It does not add an independent app login, staff roles or organizational audit
service. Do not use shared unattended accounts for sensitive meeting libraries.

Local-only processing is off by default. Turn on **Keep meeting content on this
device** (Settings → Privacy & security → Recording & data rules) unless specific
external services are approved. List the actual content-receiving hosts and document the appropriate
agreement and transfer basis. A local proxy to a remote AI is still an external
processor. A provider's marketing claim does not establish that the user's plan,
selected endpoint or signed agreement covers the intended data.

## Requests, withdrawal and incidents

Verify a requester's identity through an appropriate process before searching,
exporting or deleting. Search by known names and other relevant identifiers;
the app's name matches are candidates. Review the export for other people's
data and verify completeness. Record the decision and applicable deadline in
the organization's request register. A lawful exception or hold may require
retention; record the reason and communicate it appropriately.

Voice identification needs its own reviewed basis and record. Enroll only the
person whose authorization was checked. Withdrawal prevents future matching;
names previously written into a transcript are separate records to review.
Remembering a voice from a meeting asks how the person agreed and records that
explicit consent first; recognition after each transcription is off unless the
policy turns it on, because it also analyses the other speakers' voices.

**Your usage report** (Privacy settings) is the person's own record of what they
did: counts by week (recordings, transcripts, notes, sources played, exports, and
briefs and filings) and first-run timings. It holds no titles, names, text
or audio, is never sent, and can be saved as a JSON file the person may choose to
share. There is no telemetry.

For a suspected incident, preserve relevant evidence, restrict access, disable
affected external connections, rotate credentials, assess scope and affected
people, and involve the responsible privacy/security/legal contacts. Determine
notification duties and deadlines under the applicable law; there is no single
international deadline. Keep a written incident assessment and follow-up.

## Frameworks that require deployment-specific review

| Framework | Decisions/evidence still required from the operator |
| --- | --- |
| US HIPAA, where applicable | Covered entity/business associate role, risk analysis, administrative/physical/technical safeguards, workforce access and training, incident procedures and required BAAs. |
| EU/EEA GDPR | Roles, lawful basis and any special-category condition, transparency, rights process, retention, processor contracts, transfer mechanism, breach process and DPIA where required. |
| UK GDPR | The same deployment analysis under current UK law; voice identification may constitute special-category biometric processing. |
| US state privacy/biometric/recording laws | Applicability thresholds, notice/consent, retention and state-specific recording rules, including biometric requirements where applicable. |
| Canada, Australia, Brazil, Switzerland and other launch markets | Local applicability, sensitive-data rules, participant notice, rights, retention, incident and transfer requirements. Obtain local review before launch. |
| India | Review the phased commencement of the DPDP Act/Rules; do not assume every provision has the same effective date. |

Primary references: [HHS Security Rule](https://www.hhs.gov/hipaa/for-professionals/security/laws-regulations/index.html),
[HHS cloud/BAA guidance](https://www.hhs.gov/hipaa/for-professionals/special-topics/health-information-technology/cloud-computing/index.html),
[EDPB compliance guidance](https://www.edpb.europa.eu/sme/be-compliant/be-compliant_en),
[ICO biometric guidance](https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/lawful-basis/biometric-data-guidance-biometric-recognition/how-do-we-process-biometric-data-lawfully/),
[California Attorney General](https://www.oag.ca.gov/privacy/ccpa),
[Illinois BIPA](https://www.ilga.gov/legislation/ilcs/ilcs3.asp?ActID=3004),
[India's published Rules](https://www.meity.gov.in/static/uploads/2025/11/53450e6e5dc0bfa85ebd78686cadad39.pdf).
