# Security policy

## Supported versions

Overtalk has not published a release yet. Security fixes go to the latest commit
on `main`; once releases exist, only the newest release line is supported.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:
[Report a vulnerability](https://github.com/judeam/overtalk/security/advisories/new)
(Security tab → Advisories). Do not open a public issue or pull request for a
suspected vulnerability.

Include the commit or version, your platform, the steps to reproduce and the
impact you expect. Use synthetic data: never attach real recordings, transcripts,
logs with meeting content, credentials or tokens.

Overtalk is maintained by one person. Reports are handled as time allows, and you
will get a reply in the advisory thread. Please allow a fix to ship before you
publish details.

## Scope

In scope:

- the local API, `overtalk-cli` and the MCP server (tokens, the `/v1/proof`
  handshake, Host and Origin checks), described in [integrations](integrations/README.md);
- opening `.overtalk` files and the shared web page ([sharing](docs/SHARING.md));
- the worker sandbox (`crates/worker-sandbox/`) around speech, model and FFmpeg work;
- storage of provider keys and OAuth tokens, webhook signing, and connected-account flows;
- the build and packaging scripts and GitHub workflows in this repository.

Out of scope: problems that need an attacker who already controls your user
account, weaknesses in third-party services or models Overtalk connects to, and
the accuracy of transcripts or generated notes.

The [privacy policy](PRIVACY_POLICY.md) lists what leaves your computer, and the
[attack-surface review](docs/SECURITY-ATTACK-SURFACE.md)
records the threat model at the time it was written.
