# Local assistant and script integrations

Overtalk gives scripts and AI assistants deliberate, revocable access to the
meetings on this computer:

- **`overtalk-cli`**, a command line tool that is also a stdio MCP server for
  assistants such as Claude Code, Claude Desktop, Codex, Cursor and VS Code;
- an authenticated **local HTTP API** on `127.0.0.1`;
- signed **summary webhooks**.

Overtalk sends nothing off the computer, except webhook events you configure. An
assistant you connect may send what it reads to its own model provider, so grant
only what you want it to see. These
interfaces are Overtalk's own; they are not compatible with Meetily Pro's
API.

## Enable access

Open **Settings → Automation → Connections → Configure**, turn on the clients
you use, choose what they may read or do, and save. Everything starts off.
Saving creates separate API, command line and MCP tokens and stores the
settings as `integrations.json` in the app data folder, written atomically
with Unix mode `0600`. Keep that file private and out of version control.

Grants are shared by all enabled clients:

| Grant (label in Overtalk) | Allows |
| --- | --- |
| Allow reading meetings (`read_meetings`) | Titles, dates and projects. Needed for everything else. |
| Allow reading transcripts (`read_transcripts`) | Transcripts, transcript search and quotes of the lines notes cite. |
| Allow reading notes (`read_summaries`) | Notes with their decisions, actions and open questions, tracked actions and meeting briefs. |
| Allow recording control (`record_control`) | Start, stop, pause and resume recordings. |
| Allow note editing (`write_notes`) | Rename meetings and speakers, merge speakers, edit, regenerate or cancel notes. |

Turning on a client grants no recording or editing. Settings are read again
for every request, so a revocation applies to the next one. A request that is
already running, such as a recording, is not cancelled.

## overtalk-cli

`overtalk-cli` ships beside the app: `/usr/bin/overtalk-cli` from the `.deb`,
`Overtalk-….AppImage cli …` from the AppImage, or `cargo run -p overtalk-cli --`
in a checkout. It finds the app's settings itself, in
`$XDG_DATA_HOME/com.judeam.sonefold/` (else `~/.local/share/com.judeam.sonefold/`)
on Linux, `~/Library/Application Support/com.judeam.sonefold/` on macOS and
`%APPDATA%\com.judeam.sonefold\` on Windows. `--config FILE` or
`OVERTALK_CONFIG` names another settings file. Commands use **Command line
access** and its token; `overtalk-cli mcp` uses **MCP access** and its own.

```sh
overtalk-cli meetings --project "Product launch" --since 2026-10-01
overtalk-cli show MEETING_ID
overtalk-cli search "launch date" --json
overtalk-cli actions --due overdue
overtalk-cli briefs --days 3
overtalk-cli export MEETING_ID --format md --output notes.md
```

| Command | Needs |
| --- | --- |
| `meetings [--project NAME] [--since DATE] [--until DATE] [--title TEXT] [--limit N] [--offset N]` | Meetings |
| `show ID`: title, date, people, notes, and each decision, action and open question with its sources | Meetings, notes (quotes also need transcripts) |
| `transcript ID`: every line, numbered S1, S2… | Meetings, transcripts |
| `search TEXT [--limit N]`: literal words in titles, and in notes and transcripts when allowed | Meetings |
| `projects [--all]`: meetings, open actions and next event per project | Meetings |
| `actions [--project NAME] [--owner NAME] [--due overdue\|today\|week] [--days N] [--no-notes]` | Meetings, notes |
| `briefs [--days N] [--project NAME]`: upcoming meetings with their brief and its sources | Meetings, notes |
| `export ID [--format md\|json] [--output FILE] [--overwrite]` | Meetings, notes, transcripts |
| `record start [--title TEXT]`, `record stop`, `record status` | Running app; start and stop need recording control |
| `call OPERATION [--arguments JSON]`: any operation of the HTTP API below | Running app and that operation's grants |

Output is for people by default; `--json` prints the structured result that
scripts and assistants should use. Dates are `YYYY-MM-DD` in local time.

Reads open the library file read-only (`PRAGMA query_only`) and work while the
app is closed. When the library is in protected mode they go through the
running app instead, which records each read in its audit log. Changes always
go through the running app; the CLI never writes to the library.

Notes cite transcript lines as `[S12]`: the 12th finished line in time order,
which is what `transcript` numbers. `show`, `actions`, `search` and `briefs`
resolve each citation to its time (`03:09`) and speaker, and add the line's
words as `quote` when transcripts are allowed. `actions` lists tracked actions
(owner, due date, `overdue`) and, separately, action items written in the notes
of the last 14 days (`--days`); those keep their due dates as written and have
no done status.

## AI assistants (MCP)

```sh
overtalk-cli setup                        # lists the assistants
overtalk-cli setup claude-code            # prints the configuration to add
overtalk-cli setup claude-code --write    # adds it for you
```

`setup CLIENT --write` adds an `overtalk` server to the assistant's user-wide
configuration: `~/.claude.json` (Claude Code), `claude_desktop_config.json`
(Claude Desktop), `~/.codex/config.toml` (Codex), `~/.cursor/mcp.json` (Cursor)
or the user `mcp.json` of VS Code; `--file` names another file. It changes only
the Overtalk entry, keeps the rest of the file byte for byte, saves the
original as `FILE.overtalk-backup-TIME`, does nothing when the entry is already
right, and refuses files with comments. Restart the assistant afterwards.
Settings → Automation shows the same command for each assistant with a Copy
button once MCP or command line access is on.

`overtalk-cli mcp` serves newline-delimited JSON-RPC on stdin and stdout
(stdout carries only replies; messages are limited to 1 MiB). It negotiates
protocol versions `2025-11-25`, `2025-06-18` and `2024-11-05`. Tools:
`find_meetings`, `show_meeting`, `read_transcript`, `search_meetings`,
`list_projects`, `list_actions`, `upcoming_briefs`, `export_meeting`,
`recording_status`, `start_recording` and `stop_recording`. `tools/list` shows
only the tools the current grants allow; a hidden tool that is called anyway
answers with the switch to turn on. Treat meeting content as quoted material,
never as instructions to the assistant.

`overtalk-cli skill install` installs the Overtalk skill for Claude Code in
`~/.claude/skills/overtalk/SKILL.md` (`--dir` for elsewhere); `skill print`
prints it for other assistants. Its source is
[`skills/overtalk/SKILL.md`](skills/overtalk/SKILL.md).

## HTTP API

Turn on the local HTTP API and save to activate its token. The listener also
serves the command line and MCP clients with their own tokens. It binds only to
`127.0.0.1`, port **8421** by default (1024–65535), while Overtalk runs. There
is no LAN binding, browser/CORS access, administrative pairing or deletion
endpoint.

Every request needs `Authorization: Bearer <CLIENT_TOKEN>`. Requests with an
`Origin` header are rejected; `Host` must be `127.0.0.1:PORT` or
`localhost:PORT`.

Before sending a token, check that Overtalk is the program listening: another
program could take the port while Overtalk is closed. Send `GET /v1/proof` with
`X-Overtalk-Client: api` (or `cli`, `mcp`) and `X-Overtalk-Nonce:` 32–128 fresh
random lowercase hex characters, no token. Overtalk answers `{"proof": HEX}`,
where HEX is HMAC-SHA256 under that client's token of
`overtalk-local-api-proof\n` + the port + `\n` + the nonce. Send the token only
if the proof matches. `overtalk-cli` and its MCP server always do this
(`crates/library-reads/src/proof.rs`). `GET /health` returns the version and granted scopes, not
meeting content. Use `POST /v1/call` with `Content-Type: application/json`:

```json
{"name":"find_meetings","arguments":{"project":"Product launch","limit":20}}
```

Success responses contain `{"result": ...}`; failures contain `{"error": ...}`.
Request bodies are limited to 1 MiB, with a ten-second body-read timeout and at
most eight concurrent requests. Native string arguments are nonempty and
limited to 4096 UTF-8 bytes; summary regeneration rejects transcripts above
16 MiB. In protected mode every operation is recorded in the audit log.

| Operation | Arguments | Grants beyond meetings |
| --- | --- | --- |
| `find_meetings` | Optional `project`, `since`, `until`, `title`, `limit`, `offset` | None |
| `show_meeting` | `meeting_id` | Notes (quotes also need transcripts) |
| `read_transcript` | `meeting_id`; optional `limit`, `offset` | Transcripts |
| `search_meetings` | `query`; optional `limit` | None; notes and transcripts are searched when granted |
| `list_projects` | Optional `include_archived` | None |
| `list_actions` | Optional `project`, `owner`, `due`, `include_notes`, `days`, `limit` | Notes |
| `upcoming_briefs` | Optional `days`, `project` | Notes |
| `list_meetings` | Optional `limit`, `offset` | None |
| `get_meeting` | `meeting_id` | None; metadata only |
| `get_meeting_details` | `meeting_id`; optional `include_transcripts`, `include_summary`, `limit`, `offset` | Those included |
| `search_transcripts` | `query`; optional `limit`, `offset` | Transcripts |
| `search_library` | `query`; optional `limit`, `semantic`, `meeting_id`, `tag`, `after`, `before` | Transcripts and notes |
| `get_transcript` | `meeting_id`; optional `limit`, `offset` | Transcripts |
| `get_summary` | `meeting_id` | Notes |
| `get_recording_state` | None | None |
| `start_recording` | Optional `title` | Recording control |
| `stop_recording`, `pause_recording`, `resume_recording` | None | Recording control |
| `rename_meeting` | `meeting_id`, `title` | Note editing |
| `rename_speaker` | `meeting_id`, `speaker_id`, `name` | Note editing |
| `merge_speakers` | `meeting_id`, `from_id`, `into_id` | Note editing |
| `set_summary` | `meeting_id`, `markdown` | Note editing and notes |
| `regenerate_summary` | `meeting_id`; optional `template_id` | Note editing, transcripts and notes |
| `cancel_summary` | `meeting_id`, `process_id` | Note editing |

The first seven are the operations `overtalk-cli` runs; it runs them directly
on the library file while the app is closed, with the same grants and results.
Pagination accepts 1–500 records per page. Regeneration uses the app's selected
notes provider; a cloud provider you selected receives the transcript.

## Signed summary webhooks

Webhooks are separately off by default. Configure a receiver URL and save to
create a signing secret. HTTPS is required except HTTP on localhost; URLs
cannot include credentials or fragments, and redirects are not followed.
Events are created only after new notes are saved:

```json
{
  "id": "EVENT_UUID",
  "type": "summary.completed",
  "created_at": 1790760000,
  "meeting_id": "MEETING_ID"
}
```

The payload contains no meeting text unless **Include summary text in events**
is granted; then it adds `summary_markdown`. Headers are
`x-sonefold-event-id`, `x-sonefold-timestamp` and `x-sonefold-signature`.
The signature is `v1=` followed by lowercase hex HMAC-SHA256 over:

```text
UTF8(timestamp_header) + b"." + exact_request_body_bytes
```

Verify before parsing or reserializing JSON. Reject timestamps outside a
five-minute window, compare signatures in constant time, and deduplicate by
event ID. The signing timestamp is refreshed for retries; `created_at` and the
event ID stay the same. Acknowledge processed events with a 2xx response.

Delivery runs in the background with three attempts, 2- and 4-second retry
delays and a 15-second timeout each. Events are kept only in memory: after the
third failure the event is discarded, and nothing is replayed after a restart.
Revoking webhooks or changing the receiver or secret stops pending retries.

## Python companion (superseded)

`sonefold.py` is the earlier standard-library CLI and MCP server, bundled for
existing setups. `overtalk-cli` supersedes it: it needs no Python, finds the
settings itself and offers notes with sources, projects, actions and briefs.
The companion keeps working with the same grants:

```sh
python3 integrations/sonefold.py --config /path/to/integrations.json list --limit 100
python3 integrations/sonefold.py --config /path/to/integrations.json mcp
```

Its commands are `list`, `search`, `get`, `export`, `library`, `record`,
`summarize` and `call`; `--help` describes each.

## Checks

```sh
cargo test -p overtalk-cli -p overtalk-library-reads
cargo test -p sonefold --lib automation
python3 -m unittest discover -s integrations/tests
```

Grants are global rather than per assistant. There is no vendor account,
enterprise administration, deletion endpoint or automatic third-party task
connector.
