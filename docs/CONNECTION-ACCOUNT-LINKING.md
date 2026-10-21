# Account linking in Overtalk

The native Connections picker uses account sign-in for Linear, Atlassian
(Jira and Confluence together), Google Workspace, Microsoft 365, Slack, Notion
and GitHub. It includes provider search, official logos and deployment status. Personal API-token forms
remain under **Advanced setup**. Signing in never publishes meeting content.
Outgoing changes still require an explicit reviewed draft and publish action.

## Implementation

- Linear: official `https://mcp.linear.app/mcp`, dynamic public-client registration,
  authorization code with PKCE S256, read/write scopes.
- Atlassian: official `https://mcp.atlassian.com/v2/mcp?tools=all`, dynamic public-client
  registration using the authorization server advertised for the v2 resource.
  Requests identity, offline access and Jira/Confluence read/write scopes; no
  delete, admin, other-product or Rovo search scopes.
- Google: system-browser Desktop OAuth with a random-port loopback callback,
  PKCE S256, Docs, Drive metadata and Calendar read permissions. Google needs a
  registered Overtalk desktop client; it does not offer the same dynamic
  registration flow. No shared developer client secret is embedded in the app.

The Rust service owns the handshake and token refresh. The UI remains responsive
while waiting and can cancel or time out without retaining incomplete credentials.
The callback binds only 127.0.0.1, checks state, rejects duplicate query parameters,
checks a returned issuer, and limits request size and duration. Authentication and
resource endpoints are fixed/validated against official provider hosts. Provider
redirects are disabled. Access and refresh tokens stay in the OS keychain.

MCP credentials are used only for their MCP resource, never sent to the provider's
REST API. The bounded Streamable HTTP client supports JSON and SSE replies,
negotiated protocol/session headers and paginated tool discovery. A ten-minute
session/catalogue cache avoids rediscovery on every action. Overtalk calls a small
explicit set of tools; remote instructions do not choose actions. Responses have
size limits and errors do not expose tokens. Refresh rotation is serialized per
account. Sign in again preserves the connection ID and existing drafts, and only
replaces the stored credential after successful consent. Disconnect removes local credentials and attempts provider revocation;
when revocation cannot be confirmed, the UI says to remove the grant at the provider.

Atlassian resource identifiers preserve the site and product:
`jira:SITE_ID:ISSUE_OR_PROJECT_KEY` or `confluence:SITE_ID:PAGE_OR_SPACE_ID`.
Browse fills the destination prefix. New items use a project/space; existing items
use an issue/page. The existing draft revision check remains in place. MCP revision
checks are best-effort preflight checks, **not** an atomic compare-and-set guarantee.
A failed or interrupted write remains uncertain and is not automatically retried.

## Verified delivery

Installed and reopened native version `20261016T192544Z`. All six executable
hashes match the manifest; SQLite integrity, foreign keys, the account-provider
migration, existing meetings and the 200 benchmark-project members are verified.
419 service tests, 22 native unit tests, four native interaction/layout tests and
seven document tests pass; six existing manual service tests remain ignored.
All 18 Slint files pass the language-server check. Real-service disposable-library
workflows pass. Dark/light UI checks cover 760×540, 720p and 4K at 150% scale.
The production-controller fixture exercises provider filtering and Send through
the final reviewed draft without publishing.

This is local implementation/fixture verification, not live provider consent or
successful real remote writes. Those still require an account authorized by its
owner and the publisher registrations below.

## Google deployment gate

As of 17 October 2026, no Overtalk Google OAuth client exists. The Google option therefore explains the missing registration and disables
the normal sign-in action. It must not present a fake working connection.

The publisher must create a Google Cloud **Desktop app** OAuth client, enable the
Google Docs, Drive and Calendar APIs, configure the consent screen and complete
any Google verification required for public distribution and the requested scopes.
For a locally controlled build, set the public client ID using
`OVERTALK_GOOGLE_CLIENT_ID` at build time or in the app launch environment. The
Advanced Google form accepts a personal Desktop client ID and optional client
secret for self-managed installations. Client IDs are not user API keys.

## Verification and remaining gates

Run the native suite and Slint LSP check, then the service connection tests:

```sh
cargo test --locked --offline -p overtalk-native
python3 scripts/check-slint-lsp.py
cargo test --locked --offline -p sonefold --lib extensions::connection
```

Service tests include real local HTTP sockets; restrictive sandboxes need local
socket permission. They test callback forgery/denial/duplicates/issuer, endpoint
allowlists, SSE/JSON/session responses, Unicode network boundaries, argument
filtering and revision change detection. They do not authenticate real accounts.
Live account consent, provider-specific resource permissions and a reviewed write
must be checked with an authorized test account; do not claim these passed based
on fixtures. User account grants are never taken from this coding session's MCP
connectors. No benchmark generation or paid model calls are part of this change.

## Sources checked on 17 October 2026

- [Linear MCP](https://linear.app/docs/mcp)
- [Atlassian MCP setup](https://support.atlassian.com/atlassian-ai-gateway/docs/get-started-with-the-atlassian-remote-mcp-server/)
- [Atlassian supported tools](https://support.atlassian.com/atlassian-rovo-mcp-server/docs/supported-tools/)
- [Google native-app OAuth](https://developers.google.com/identity/protocols/oauth2/native-app)
- [MCP authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization)

## New connections — 17 October

- **Notion:** official `https://mcp.notion.com/mcp`, dynamic public-client registration
  and PKCE. Search and fetch pages, create private pages or children of a selected
  page, and prepare replacement content. Queued operations are polled without
  retrying the write. Notion warning messages must be reviewed alongside results.
- **Microsoft 365:** public desktop authorization code + PKCE through the common
  tenant. Read Outlook calendar events, participants and meeting links; browse
  OneDrive folders; read Word, Markdown and text documents. New notes create a
  Word document. Existing files receive appended content, preserving their
  original parts; updates use an `If-Match` ETag. SharePoint/OneDrive links work
  only when granted access permits them. This does not crawl every SharePoint site.
- **Slack:** public-client PKCE with user scopes, no bot required. Browse joined
  channels, read messages and threads, publish reviewed messages and update the
  connected user's messages. Unfurls and mention parsing are disabled. Private
  channel access remains subject to membership and workspace policy.
- **GitHub:** GitHub App device flow. Shows a one-time code and opens the official
  GitHub verification page. No embedded client secret. Browse accessible repositories,
  read issues and pull-request descriptions, and create/update issues. Repository
  access follows the user's GitHub App grant; it never requests a broad `repo` scope.

The notes toolbar now offers **Send**. It opens connected accounts, destination
selection and editable content, followed by the existing explicit publish review.
Nothing is sent by choosing an account or destination. Read-only permissions and
workspace policy errors remain possible and are displayed by the service.

All APIs have fixed authorities; redirect downloads from Microsoft omit the bearer
header, use known Microsoft hosts and have size limits. Word ZIPs are read in
memory with archive limits and XML external entities disabled. Automatic retries
never repeat a mutation. Remote revisions on GitHub, Slack and MCP are preflight
checks, not transactional locks; Microsoft and Google enforce revision conditions.

The additive `20261019000000_connection_accounts.sql` migration widens the old
four-provider check constraint. It copies both connection and outgoing-draft
rows before replacing their tables, with foreign keys enabled throughout. A
regression verifies draft contents/revisions/status, all new provider values,
project SET NULL behavior and disconnect cascading. A disposable copy of an
existing library with the 200-member benchmark project passes the upgrade and
integrity check.

## Publisher registrations required

**Google, Microsoft, Slack and GitHub registrations do not exist yet** (as of
17 October 2026). These four options correctly show “App setup needed” in a stock build.
This is a publishing gate, not something each ordinary user should configure.
Linear, Atlassian and Notion use dynamic registration. No client IDs from another
application or account are reused.

Public client IDs may be supplied at build time or in the launch environment:

| Provider | Public configuration | Registration |
| --- | --- | --- |
| Google | `OVERTALK_GOOGLE_CLIENT_ID` | Desktop app; see Google deployment gate above |
| Microsoft | `OVERTALK_MICROSOFT_CLIENT_ID` | Microsoft Entra application, public desktop client; support organizational and personal accounts if both are required |
| Slack | `OVERTALK_SLACK_CLIENT_ID` | Slack app with PKCE enabled and user scopes |
| GitHub | `OVERTALK_GITHUB_CLIENT_ID` | GitHub App with device flow enabled |

For self-managed installs, **Advanced setup** accepts a public client ID, without
asking users for a Microsoft/Slack/GitHub client secret.

### Microsoft registration

1. Create an app in [Microsoft Entra](https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade).
2. Add a **Mobile and desktop applications** platform with loopback redirect
   `http://localhost/callback`. The runtime chooses an available port; Microsoft
   ignores the port for localhost redirect matching. Enable public-client flows.
3. Add delegated permissions `User.Read`, `Calendars.Read`, `Files.ReadWrite`.
   Authorization also requests `openid profile offline_access`. Tenant policy
   may require an administrator to approve consent.
4. Use the Application (client) ID, not a secret, in the public configuration.

### Slack registration

1. Create an app at [Slack app settings](https://api.slack.com/apps).
2. Under OAuth & Permissions, enable **PKCE**. Slack treats this as a public-client
   setting; enabling it is a one-way setting, according to its documentation.
3. Register `http://localhost:43871/callback` and add **user** scopes:
   `channels:read`, `groups:read`, `channels:history`, `groups:history`, `chat:write`.
4. Use its client ID. For distribution outside the development workspace, complete
   Slack's distribution/review requirements. Enable token rotation as appropriate;
   expired grants prompt reconnection. No bot scopes are requested.

### GitHub registration

1. Register a [GitHub App](https://github.com/settings/apps/new).
2. Enable **Device flow**. Webhooks and a private signing key are not used by this
   user-authorized desktop flow.
3. Grant repository **Metadata: read**, **Issues: read/write** and **Pull requests:
   read**. Keep the repository selection controlled by the user.
4. Use the app's **Client ID**, not its App ID, private key or client secret.
5. For distribution, allow installation for the intended accounts/organizations.

## Additional primary references

- [Microsoft desktop authorization](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)
- [Microsoft file uploads](https://learn.microsoft.com/en-us/graph/api/driveitem-put-content)
- [Slack public-client PKCE](https://docs.slack.dev/authentication/using-pkce/)
- [GitHub device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)
- [Notion MCP client authorization](https://developers.notion.com/guides/mcp/build-mcp-client)
- [Notion tools](https://developers.notion.com/guides/mcp/mcp-supported-tools)

Asana/ClickUp/Trello remain possible follow-ons; HubSpot/Salesforce are more useful
if sales/customer-success becomes a primary audience. They are not implemented
by this change. The competitor findings and prioritized recommendations came from
an internal workflow audit that is not published.
