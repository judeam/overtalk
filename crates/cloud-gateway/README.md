# Overtalk cloud credits

The server for Overtalk's optional private AI summaries: accounts, Stripe billing
and access to Privatemode. This executable and its Stripe / Privatemode secrets
must **never** be bundled in the desktop installer. Nothing here is needed to use
Overtalk with local models or your own provider keys.

## Routes

`OVERTALK_CLOUD_ROUTE` selects how private AI summaries reach Privatemode.

- **`end-to-end` (default).** This server never sees meeting text. During a paid
  subscription period it hands each account its own Privatemode API key
  (`POST /v1/summaries/key`); the desktop runs Privatemode's official proxy on
  loopback with that key (`frontend/src-tauri/src/summary/privatemode_runtime.rs`,
  which downloads the proxy from Privatemode's registry and checks pinned
  hashes). `POST /v1/chat/completions` answers 410 on this route. Each key's
  monthly token limits at Privatemode enforce the allowance.
- **`gateway` (legacy).** This server forwards requests through its own proxy
  with one shared key and meters credits. It can read the text.

### Keys for the end-to-end route

The operator creates keys in the Privatemode portal: one per subscriber, with a
short expiry and monthly token limits that cover the summary allowance
(`SUMMARY_HOURS` in `src/config.rs`). A key keeps working at Privatemode until it
expires, so the gateway hands each subscriber the shortest free key that covers
their paid period, and the desktop asks for a new key when its key lapses.

```sh
# keys.tsv, mode 600: LABEL<TAB>KEY<TAB>EXPIRES (YYYY-MM-DD), one per line
overtalk-cloud-gateway keys import keys.tsv
overtalk-cloud-gateway keys stale          # assigned keys without a paid period
overtalk-cloud-gateway keys retire ID      # after deleting the key in the portal
# One seat of an invoiced team plan (outside Stripe):
overtalk-cloud-gateway entitle ACCOUNT_ID INVOICE_REF PAID_CENTS 2027-10-31
```

Keep a few free keys imported; a subscriber without a free key gets a 503 and
keeps local processing. Run `keys stale` daily and after billing changes, and
delete those keys in the portal: retiring here does not revoke them there.

The desktop's `cloud_account` command stores a separate, random Overtalk account
credential in the OS keychain and opens hosted Stripe Checkout and Billing Portal
pages. Local processing and personal provider connections remain available.

## Plans

Plans are `summaries_once`, `summaries_monthly`, `summaries_annual` (optional) and
invoiced team seats (`entitle`); their credit grants are set in `src/ledger.rs`.
On the end-to-end route only a paid monthly or annual period, or an invoiced seat,
receives a key. Initial subscription Checkout grants nothing; `invoice.paid` owns
fulfilment, so it cannot happen twice. Trials, unpaid invoices and proration
invoices grant nothing. Metered and quantity-based Stripe plans are unsupported.
Changing a device clock or restarting the desktop cannot create credit.

## Before enabling it

Leave `OVERTALK_CLOUD_ENABLED=false` until:

1. You have the provider's written permission to resell access, recorded in
   `OVERTALK_PRIVATEMODE_APPROVAL_REFERENCE`. This is an operator assertion, not
   automatic legal verification.
2. A Stripe account and its prices are configured (`OVERTALK_STRIPE_CURRENCY`,
   `OVERTALK_STRIPE_MONTHLY_PRICE`, optionally `OVERTALK_STRIPE_ANNUAL_PRICE`; the
   legacy gateway route also needs USD prices for its credits). The server
   validates prices, currency, billing mode and test/live environment before
   accepting traffic. Tax collection, customer terms and refunds for your
   markets are your responsibility.
3. A single gateway instance runs with private persistent SQLite storage behind a
   managed HTTPS reverse proxy, with monitoring of the billing inbox and backups
   of its financial state.
4. End-to-end route: keys are imported and `keys stale` is scheduled. Gateway
   route: the official Privatemode proxy runs privately on loopback, pinned to a
   reviewed image digest, with request dumps disabled.
5. The desktop's `OVERTALK_CLOUD_URL` points at the deployed HTTPS origin. No
   default production endpoint or secret is included in source or installers.

Server-side secrets solve credential distribution; they do not override
[Privatemode's terms](https://www.privatemode.ai/terms-of-service).

## Run and verify

```sh
cargo test --locked -p overtalk-cloud-gateway
cargo clippy --locked -p overtalk-cloud-gateway --all-targets -- -D warnings
cargo build --locked --release -p overtalk-cloud-gateway
```

Copy `gateway.env.example` outside the repository and configure it. Secrets are
read from files, which must be regular, owner-readable-only files; never pass
credentials as shell arguments. The gateway binds to loopback, defaults to
disabled, refuses non-HTTPS live billing origins and does not follow redirects or
inherit a system HTTP proxy. Stop it gracefully with SIGINT.

Use Stripe test mode first. Register `/stripe/webhook` with API version
`2026-09-30.endive` and these events:

- `checkout.session.completed`, `checkout.session.async_payment_succeeded`
- `invoice.paid`
- `charge.refunded`
- `charge.dispute.created`, `charge.dispute.updated`, `charge.dispute.closed`

The signature is checked against the original body with a five-minute tolerance.
Event IDs enter a durable inbox before acknowledgment, so duplicates are harmless;
the worker retrieves authoritative objects from Stripe and retries with bounded
backoff. Rows marked `review` need operator attention: reset them to `pending`
only after fixing the cause, and alert on them.

## Credit accounting (legacy gateway route)

Grants, reservations and settlements use integer USD microcredits. A request
reserves a conservative allowance before dispatch; two in-flight requests per
account and a global limit bound spending. Insufficient credit returns 402, and
a repeated request ID returns 409 without another upstream request. The rate card
is set in `src/config.rs` and reported by `GET /v1/account`; check it against the
provider's pricing before launch and whenever that changes.

Missing usage, timeouts or disconnects leave a bounded hold. After checking the
provider's actual usage, settle it with:

```sh
overtalk-cloud-gateway reconcile REQUEST_ID VERIFIED_MICRO_USD
```

Settlement is idempotent and cannot exceed the reservation. Refunds reverse the
matching grant proportionally; a refund beyond unused credit or an open dispute
pauses further cloud use. There is no automatic card top-up.

## Privacy

The end-to-end route never receives meeting text: the desktop's own proxy
encrypts it. On the legacy gateway route the server receives plaintext over TLS
before the official proxy encrypts it to Privatemode, so that route is not
end-to-end encrypted, and the desktop says so. Meeting content is never stored in
the financial database or logs. The ledger stores account hashes, random cache
salts, Stripe references, balances and request IDs.

## Accounts and recovery

Accounts use opaque credentials (256 random bits), stored hashed on the gateway
and in the desktop's OS keychain; never put them in URLs, Stripe metadata or
logs. Recovery is support-assisted: verify ownership against the Stripe purchase
record, run `recover ACCOUNT_ID` on the gateway host to issue a replacement and
revoke the old one, and deliver it privately; the customer enters it under
**Restore existing access**.

Run one instance only: independent SQLite replicas would allow overspending. The
gateway rate-limits account creation and authenticated calls and does not trust
forwarded IP headers; add connection limits at the HTTPS proxy too.

## API

| Method/path | Authorization | Result |
| --- | --- | --- |
| `GET /health` | None | Availability gate only |
| `POST /v1/accounts` | Rate-limited | New empty account; credential returned once |
| `GET /v1/account` | Overtalk account bearer | Balance, holds, policies and rate card |
| `POST /v1/checkouts` | Overtalk account bearer | Allowlisted plan's hosted Checkout URL |
| `POST /v1/billing` | Overtalk account bearer | Hosted Billing Portal URL |
| `POST /v1/summaries/key` | Overtalk account bearer | The account's Privatemode key during a paid period |
| `GET /v1/models` | Overtalk account bearer | Allowed cloud model |
| `POST /v1/chat/completions` | Overtalk bearer + `Idempotency-Key` | Bounded, metered request (legacy route) |
| `POST /stripe/webhook` | Stripe raw-body signature | Durable event acknowledgment |

There is no endpoint that returns the shared provider credential, chooses
arbitrary upstream hosts or models, resets balances or fulfils payment from a
browser success URL.
