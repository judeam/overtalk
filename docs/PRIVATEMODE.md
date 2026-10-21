# Privatemode in Overtalk

In **Settings → Notes & AI**, choose the provider **Privatemode · encrypted
cloud AI**. The native and web interfaces use the same service. **Connect & list
models** queries a proxy that is already running, without sending a meeting.
GLM-5.3, GLM-5.3-Flash and GPT-OSS-120B are supported; aliases are hidden when
their concrete model is available.

The model choice is explicit. Selecting Privatemode sends subsequent AI requests
through its encrypted cloud service; local transcription and stored recordings
stay on the device. Overtalk does not upload audio to this provider.

## Start the official proxy

Overtalk connects to Privatemode's official attesting/encrypting proxy on
loopback. How the proxy starts depends on the platform:

- **Linux.** Leave **Local proxy address** empty (the default is
  `http://127.0.0.1:8080/v1`) and save the API key in Overtalk. If nothing is
  listening on that address when the first AI request is made, the engine
  downloads Privatemode's official proxy v1.59.0 from `ghcr.io`, checks it
  against pinned hashes, and runs it on a private loopback port without Docker.
  The proxy stops when the engine exits. A proxy you started yourself on the
  default address is used instead.
- **macOS and Windows.** Start Privatemode's proxy yourself, for example with
  Privatemode's proxy app or the Docker launcher below, and enter its address in
  Overtalk.

The Docker launcher `scripts/run-privatemode-proxy.py` requires Docker
(including its Linux VM on macOS and Windows). It pins official release v1.59.0
by the registry manifest digest; it does not install an always-running service
or change billing settings. Save the key in an owner-only file without including
it in a command:

```sh
bash -c 'umask 077; set -o noclobber; read -r -s -p "Privatemode API key: " key; printf "\n"; printf "%s" "$key" > /tmp/overtalk-privatemode-api-key; unset key'
python3 scripts/run-privatemode-proxy.py \
  --key-file /tmp/overtalk-privatemode-api-key --port 8788 --detach
```

In Overtalk, set **Local proxy address** to `http://127.0.0.1:8788/v1`.
Leave the app key empty when the proxy already has it. Alternatively enter it
in Overtalk; that key is saved in the OS keychain, never the SQLite settings or
provider-change events. An empty field preserves an existing key.
The launcher substitutes its key only for Overtalk's `placeholder` credential;
an explicit key entered in the app is forwarded unchanged.

The local HTTP connection is confined to the device. The proxy encrypts the
remote connection after attestation. Overtalk rejects remote/proxyless URLs,
redirects, URL credentials, and inherited system HTTP proxies for this provider.
The Docker launcher drops container capabilities and uses a read-only root
filesystem. Both the launcher and the engine's own proxy reject unknown or
revoked NVIDIA certificates.
`/readyz` by itself is not proof of attestation. A successful authenticated
request and the proxy's successful secret-update log establish a working path.

Stop a proxy started by the Docker launcher when it is no longer needed:

```sh
docker stop overtalk-privatemode-8788
```

Keep its key file somewhere private and persistent if using it regularly;
`/tmp` is intended only for the temporary comparison. Never commit it.

## Reproduce the accuracy comparison

The comparison uses frozen **synthetic** inputs from the existing benchmark.
Both recognized speech and clean-reference controls are sent. Expected facts
are read only when the saved reports are graded, never during generation.
The original Qwen3.5-4B outputs are preserved; a separate current-code local
control isolates improvements from the earlier validator fixes.

Use the release native unit-test executable as `--native-runner`. Build with
the platform dependencies in [NATIVE-UI.md](NATIVE-UI.md). The ignored
`eval_long_meetings` test uses the real application pipeline and template.

```sh
python3 -m evaluations.meetingbench.privatemode_compare \
  --inputs evaluations/meetingbench/.runtime/experiments/fast-full200-v2/summary-inputs \
  --baseline evaluations/meetingbench/.runtime/experiments/fast-full200-v2/notes \
  --native-runner /path/to/native-unit-test-executable \
  --output /path/to/new-private-output-directory \
  --key-file /path/to/private-key-file \
  --endpoint http://127.0.0.1:8788/v1 \
  --models glm-5.3 glm-5.3-flash gpt-oss-120b \
  --cases normal-004-reference normal-004-asr \
  --confirmed-free-plan
```

`--confirmed-free-plan` means the user verified **Free** in Privatemode's billing
portal with pay-as-you-go disabled. A key alone or positive balance on a paid
plan cannot guarantee no charges. This runner has no paid fallback. The
default per-run ceiling is 500,000 quota tokens, reserving UTF-8 prompt bytes,
framing, capped output, and GPT-OSS's 2× quota multiplier before each request.
It stops on quota/authentication/provider errors. `--prepare-only` freezes inputs
without a key read, network connection, or inference request.

Saved artifacts include input/baseline/native-binary hashes, notes, generation
timing, actual token usage, and failure status. Output/reasoning is capped at
8,192 tokens for the comparison (16,384 in the app); reasoning effort is `low`.
The small-model and large-model app paths differ, so results compare usable
systems, not parameter count in isolation. No automatic model judges run.

Model capabilities, dates, pricing and licensing come from an internal research
note dated 16 October 2026, which is not published. Check Privatemode's current
documentation before relying on them.
