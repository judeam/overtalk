# Sharing a meeting

"Share a copy" writes a file the person chooses. Nothing is uploaded and there is no
server; the person decides who receives the file. The engine
(`frontend/src-tauri/src/extensions/share.rs`) builds, redacts and checks every copy;
the desktop interface only collects choices. Sending notes to a connected tool is a
separate feature.

Commands: `workspace_share_estimate` (size before saving),
`workspace_share_meeting` (write a copy to the chosen path) and
`workspace_open_shared` (open an .overtalk file as a new meeting). The local usage
report counts `meeting_shared` and `meeting_imported`; it never records titles,
names, text or audio.

## A web page

One self-contained `.html` file that opens offline in any browser, on a phone too.

- Title, date and length, and how participants were told (the meeting's recording
  notice line) when there is a record.
- The notes as written: decisions, action items with owner and due date, open
  questions. Each source reference (`[S12]`) becomes a button that plays that moment.
- The transcript with speakers and timestamps. A line with audio shows its time as a
  play button; pressing a source also highlights its transcript line.
- A footer: "Shared from Overtalk · date", and that the page is a copy.

Audio:

| Option | Contents |
| --- | --- |
| Source clips (default) | Two seconds before to two seconds after each cited line (at most 26 s of one line), merged where clips meet. |
| Full recording | The whole recording. |
| No audio | Sources link to their transcript line instead. |

Audio is mono Opus speech at 24 kbit/s in WebM, encoded by the engine's verified
FFmpeg inside the worker sandbox (it may read only the recording and write only the
clip), and embedded as `data:` URIs. A 65-second meeting gives a 186 KB page with
three clips, or 287 KB with the full recording. Pages over 200 MB are refused in
favour of clips or an .overtalk file.

The page makes no requests. Its Content Security Policy is `default-src 'none';
media-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri
'none'; form-action 'none'`. Styles and a small player script are inline. Every
title, name, note and transcript character is HTML-escaped. Raw HTML in notes stays
visible text, and links and images keep only their label, never their address.
Light and dark follow `prefers-color-scheme`; buttons are real `<button>` elements
with labels.

## An .overtalk file

A zip with three entries at its root:

- `manifest.json`: `format` (`"overtalk-meeting-v1"`), `app_version`, `created_at`,
  whether redaction changed the copy, and each other entry's name, size and SHA-256.
- `meeting.json`: title, `created_at`, duration, the notice line and its record
  (method, route, wording, required, time), optional project name, speakers (id and
  name), transcript segments (text, start, end, speaker id, capture source) in
  Overtalk's order, and the notes: the saved summary result JSON as stored.
- `audio.<ext>`: the original recording, a smaller Opus copy (32 kbit/s mono WebM),
  or nothing. Silencing time ranges always produces the smaller copy.

Segment order is the order that numbers the notes' sources, so `[S4]` is the fourth
segment both before and after a round trip.

## Redaction

Both formats use the export rules (`extensions/privacy.rs`, `Redactions`), the same
ones the redacted Word, PDF and Markdown exports use:

- `terms`: literal names or phrases, case-insensitive, replaced with `[redacted]` in
  the title, notes, speaker names, transcript, project name and recording notice.
- `contacts`: email addresses and phone numbers, matched before terms so a term
  inside an address does not leave the rest of it.
- `omit_notes`: no notes in the copy.
- `spans`: time ranges. Overlapping transcript lines become `[redacted passage]`,
  the audio is silenced there (with a 0.1 s guard), the title becomes "Redacted
  meeting", and the notes are left out because they cannot be aligned to time.

Without rules, an .overtalk file keeps the notes exactly as stored. With any rule it
carries only the redacted notes as Markdown, since the stored notes hold fields the
Markdown never shows, and speaker ids become opaque (`speaker-1`, …) in both the
speakers and the lines. A redacted copy's suggested file name is "Shared meeting",
not the meeting title.

## Where a copy is written

`workspace_share_meeting` writes only to a path the person picked in the engine's
own save dialog (`workspace_pick_file` with `share-page` or `share-file`), once,
within an hour of picking it. An interface cannot name a path itself. When the
engine adds the `.html` or `.overtalk` extension, it refuses rather than replace an
existing file the dialog never asked about.

## Opening an .overtalk file safely

An .overtalk file comes from someone else and is treated as untrusted:

- Only a file the person picked in the engine's open dialog (`workspace_pick_file`
  with `share-open`) is read, once, within the hour; an interface cannot name one.
- The meeting's notice record is kept as the sender's report, not as a notice given
  on this computer: it is stored with `source = 'shared-file'` and its line reads
  "Reported in the shared file: …" in meeting details and exports.

- At most 2 GB, opened as a regular file (not a link or device), with at most three
  entries. Only `manifest.json`, `meeting.json` and `audio.` plus webm, ogg, m4a,
  mp4, mp3, wav or flac are allowed, as flat names. Paths, `..`, absolute names,
  directories, symbolic links, special files, encrypted entries and compression other
  than stored or deflate are refused before any content is read.
- Size limits hold however large an entry claims to be: 64 KB for the manifest,
  32 MB for the meeting, and the declared size for audio. Every entry must match the
  manifest's size and SHA-256, and the manifest must list exactly the entries present.
- Only format `overtalk-meeting-v1` opens. A later `overtalk-meeting-*` version asks
  for an Overtalk update; anything else is "not an Overtalk meeting file".
- JSON is parsed strictly (unknown fields are refused) and every field is checked:
  text lengths and NUL characters, RFC 3339 dates, times within 24 hours with
  end ≥ start, lines in source order, speaker ids that exist, a known notice method
  and route, notes that Overtalk can render and that cite only existing lines.
- The audio is extracted under its allow-listed name into a new meeting folder and
  decoded by FFmpeg in the worker sandbox; a file that does not decode is refused and
  the folder removed. Nothing in the file is executed or rendered as markup.
- The meeting is created in one transaction with new identifiers, so opening the same
  file twice gives two independent meetings. It is filed into a project only when one
  of the same name already exists; no project is created.
