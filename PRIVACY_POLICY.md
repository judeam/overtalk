# Overtalk privacy

Overtalk stores its meeting library, recordings, settings and transcript recovery data on the device. The library is a plain SQLite database that Overtalk does not encrypt; use the operating system's disk encryption. Encrypted backups are a separate, opt-in feature. Overtalk has no usage telemetry and no automatic updater, and it does not contact Meetily's original vendor.

Local speech models and the built-in notes models process content on the device. A new installation contacts nothing until the person downloads a model, connects an account or chooses a cloud provider. Turning on **Keep meeting content on this device** (Settings → Privacy & security → Recording & data rules) blocks sending meeting content to any host outside the device.

## What leaves your computer

| Destination | When | Default |
| --- | --- | --- |
| Hugging Face (speech and notes models); GitHub (the voice-memory model) | Only when the person starts a download in Setup or Settings. Downloads are pinned and checksummed. | Nothing is downloaded |
| A cloud notes provider: OpenAI, Claude, Groq, OpenRouter, Privatemode, a custom OpenAI-compatible endpoint, or a remote Ollama | When the person selects and configures it. It receives the text each request needs (notes, questions, translations, briefs). Its own data policies apply. | Off; local models |
| ghcr.io (Privatemode's official proxy, Linux only) | The first Privatemode request when no proxy is already running on the default local address. The download is pinned by hash. | Off |
| Connected accounts: Google, Microsoft, Slack, GitHub, Linear, Atlassian (Jira, Confluence), Notion | Only after the person connects the account. Connected Google and Microsoft calendars then sync in the background every 15 minutes, except while recording. Changes to other tools are sent only when the person publishes a reviewed draft. | Nothing connected |
| Linear or Jira, for meeting briefs | When a brief lists tickets created from earlier meetings, it asks the connected tool for their current status. Only the ticket ID is sent. | Only with a connected account |
| The webhook URL the person sets | After notes are completed, when webhooks are on. Events contain identifiers; adding summary text is a separate setting. | Off |
| Overtalk's cloud service | Only in builds configured with one (`OVERTALK_CLOUD_URL`), when the person signs in to a paid plan. It handles account, billing and key requests and never receives meeting text. | Not present in source builds |

**Meeting briefs.** About 70 minutes before a calendar meeting that belongs to a project, Overtalk prepares a brief from the local library. Change this under **Settings → Automation → Briefs before meetings** (project meetings, all meetings with invitees, or off). If a cloud notes provider is selected, Overtalk uses it in the background to phrase the brief's talking points, which sends short excerpts of earlier notes.

**Credentials.** Provider API keys and connected-account OAuth tokens are kept in the OS keychain, never in the SQLite database. The local API, command line and MCP tokens and the webhook signing secret are stored in `integrations.json` in the app data folder, with Unix mode `0600`.

**Local access and webhooks.** CLI/MCP access, the loopback API and webhooks are off by default. Enabling a connection grants only its configured scopes. A webhook delivery is tried up to three times in memory; if every attempt fails, it is discarded. It is not written to disk. Revoke credentials or disable a connection to stop future access.

Exports and copies go to destinations the user chooses. Back up recordings and the database when needed, and keep recordings, transcripts and configuration files out of Git. Deleting a meeting does not erase exported copies or backups the user created elsewhere. The application makes no claim of regulatory certification or guaranteed anonymization.
