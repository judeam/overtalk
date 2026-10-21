# Everyday meeting workflow

The native app supports five connected flows:

- **Setup check:** Home → Run setup check validates the selected speech runtime
  and local model files. A 10-second recording captures microphone and call audio
  for playback; the user confirms both can be heard. Cloud notes configurations
  are labeled as configured, not as verified service calls. Existing providers
  are preserved; Qwen 3.5 4B is recommended for fast local notes.
- **Coming up:** Home reads the local calendar cache immediately, then refreshes
  linked Google and Microsoft accounts with at most two requests in flight.
  The engine repeats the sync every 15 minutes whatever view is open (not while
  recording) and announces `workspace-calendar-changed`. Errors preserve cached
  events. Invites show local start time and participants. Join & take notes opens
  the meeting link using the system handler and starts a named recording;
  duplicate clicks are ignored. Calendar participants are invitees, not verified
  voice identities. OAuth app registrations are still required for Google and
  Microsoft; see `CONNECTION-ACCOUNT-LINKING.md`.
- **Automatic completion:** the capture intent is durable before audio starts.
  The recording journal saves the meeting before scheduling notes; when no final
  transcript exists, it schedules transcription (as the notes' dependency, or on
  its own when automatic notes are off). That transcription uses the configured
  local engine (Parakeet or Whisper, or the speech worker for local speech) and
  waits, queued, while the speech model downloads. No old meetings are
  retroactively enqueued. Automatic jobs resume after a restart and expose
  errors/retry in Notes. Heavy jobs stay serialized and wait for recording.
  The default is enabled, with a visible per-user opt-out.
- **My notes:** pre-call writing on Home is copied into the new recording.
  During and after a call it saves separately from generated notes. Saves use
  revision checks; navigation retains pending drafts and app close waits for
  them. A conflict preserves the draft and asks for reconciliation. Personal
  writing can guide emphasis in generation but is not transcript evidence.
- **Meeting companion:** Notes → Ask opens a responsive side panel (full pane on
  narrow windows) with My notes, source-linked questions and editable follow-up
  drafts. Questions are restricted to the open meeting. The follow-up composes
  supported outcomes from the same validated evidence path; nothing is sent
  automatically. Drafts persist independently and can be copied for review.

- **Calendar link and project:** a recording keeps a copy of its calendar
  event in `meeting_events` (title, invitees, organizer, recurring series), so
  later syncs cannot erase it. Join links the chosen event; any other recording
  links a non-all-day event with invitees or a call link happening now, except
  sound checks. The recorder's project choice wins; "No project" is final;
  otherwise a choice made for the event applies. `project_meetings.origin`
  records whether a person or an automatic decision filed a meeting, with its
  reason, and a person's choice is never replaced. Suggestions
  (`project_suggestions`) are accepted as a person's choice. Calendar sync also
  stores organizer and series IDs; Google accounts are named from the
  calendar's own entry.
- **Recording notice:** Record a meeting and Join & take notes first show one
  sentence to paste into the call chat (Copy & record) or read aloud. Its
  wording follows the speech and notes providers configured when recording
  starts: on this computer by default ("…nothing is sent to a cloud service"),
  or naming the service that receives the audio or transcript. The notes
  provider counts only when the recording writes notes automatically; notes
  generated later with a cloud provider are not covered by the notice.
  Privatemode and Overtalk cloud notes go through Privatemode's local proxy, so
  the transcript is "encrypted before it is sent to Privatemode". Local-only
  mode is always local. Both sentences are
  editable in Privacy › Recording & data rules (`{services}` says what is sent
  where). Recording without notice stays possible and is saved as skipped.
  `meeting_notices`
  keeps how and when (copied, read aloud, confirmed, skipped), the sentence and
  the route; meeting details and every export show one line, such as
  "Participants were notified at 10:02 (notice copied to chat)". With "Confirm
  participant notice" off there is no step and the meeting records "notice step
  turned off". The record is the user's statement, not a compliance check.

**Automatic filing and briefs.** `src/extensions/project_memory/` files upcoming
calendar meetings in their project before they start (recurring series,
invitees, their organisation, title phrases, the calendar account) and calls
with no invite when their transcript is final, before notes; clear winners are
filed with their reason, close calls become suggestions, and a person's choice
always wins and teaches the matcher. Meetings recorded before filing was turned
on are filed once at start. About an hour before a calendar meeting it prepares
a brief: what was decided and what is still open (each line linked to its
moment), open actions, who is coming and when you last met them, project
context and ticket status; talking points come from the notes model and must
cite those lines. It runs through `src/addon.rs`, which the engine installs at
start; another build may install its own addon first. Results live in
`event_projects`, `project_suggestions`, `project_match_rules`,
`meeting_briefs`, `filing_log` and `brief_reminders`. Hooks never gate capture,
reading, export or recovery.

If someone edits the generated document while inference is running, completion,
failed generation and cancellation preserve their edits. An unapplied generated
alternative is kept in note version history. A cancelled or stale request cannot
replace a newer meeting's answer.

Storage is local SQLite. Deleting a meeting permanently also deletes its personal
notepad, follow-up, capture intent, event copy and notice record. Schema changes
are in the migrations `20261020000000_meeting_workflow.sql`,
`20261024000000_meeting_context.sql` and `20261025000000_recording_notices.sql`;
no applied migration is modified.

Verification uses service-level SQLite tests, the real native-service protocol,
production-controller fixtures, Slint LSP and small/large dark/light renders.
Fixtures are synthetic; they do not demonstrate live Google/Microsoft consent or
remote model availability. This workflow change does not run the paid 200-meeting
benchmark or change its budget.
