//! Hosted Checkout, signed durable webhooks and authoritative fulfillment.
use crate::{
    config::{Config, STRIPE_VERSION},
    ledger::{Grant, Ledger, STARTER_UNITS},
};
use anyhow::{ensure, Context, Result};
use ring::hmac;
use serde_json::Value;
use sqlx::Row;
use std::{sync::Arc, time::Duration};

pub fn verify_signature(body: &[u8], header: &str, secret: &str, now: i64) -> bool {
    if header.len() > 2048 {
        return false;
    }
    let times: Vec<_> = header
        .split(',')
        .filter_map(|s| s.strip_prefix("t="))
        .collect();
    if times.len() != 1 {
        return false;
    }
    let Ok(timestamp) = times[0].parse::<i64>() else {
        return false;
    };
    if now.abs_diff(timestamp) > 300 {
        return false;
    }
    let mut signed = format!("{timestamp}.").into_bytes();
    signed.extend_from_slice(body);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    header
        .split(',')
        .filter_map(|s| s.strip_prefix("v1="))
        .any(|s| {
            if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
                return false;
            }
            let signature: Result<Vec<_>, _> = (0..64)
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
                .collect();
            signature.is_ok_and(|bytes| hmac::verify(&key, &signed, &bytes).is_ok())
        })
}

pub fn id(value: &Value) -> Result<&str> {
    let id = value
        .as_str()
        .or_else(|| value["id"].as_str())
        .context("Missing Stripe object ID")?;
    ensure!(
        !id.is_empty()
            && id.len() < 128
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "Invalid Stripe object ID"
    );
    Ok(id)
}

/// The refund promise shown above Checkout's pay button for private summaries: the same sentence
/// as the website and the app.
pub const REFUND: &str =
    "If private summaries aren't right for you, ask within 30 days of paying for a full refund.";

fn submit_message(plan: &str) -> Option<&'static str> {
    plan.starts_with("summaries_").then_some(REFUND)
}

pub struct Stripe {
    pub config: Arc<Config>,
    client: reqwest::Client,
    api_base: url::Url,
}

impl Stripe {
    pub async fn validate_prices(&self) -> Result<()> {
        for (price, interval) in [
            (&self.config.lifetime_price, None),
            (&self.config.monthly_price, Some("month")),
            (&self.config.annual_price, Some("year")),
            (&self.config.topup_price, None),
        ] {
            if price.is_empty() {
                continue; // Optional: the annual plan, and on the end-to-end route one-time plans and top-ups.
            }
            let price = self.get(&format!("prices/{price}"), &[]).await?;
            ensure!(
                price["active"] == true
                    && price["currency"] == self.config.currency.as_str()
                    && price["livemode"].as_bool() == Some(self.config.live),
                "Stripe price is inactive or has the wrong currency/environment"
            );
            ensure!(
                price["unit_amount"]
                    .as_i64()
                    .is_some_and(|amount| amount > 0),
                "Use fixed, positive Stripe prices"
            );
            if let Some(interval) = interval {
                ensure!(
                    price["type"] == "recurring"
                        && price["recurring"]["interval"] == interval
                        && price["recurring"]["interval_count"] == 1,
                    "Subscriptions require a Stripe price billed every {interval}"
                );
            } else {
                ensure!(
                    price["type"] == "one_time",
                    "One-time purchases must use one-time Stripe prices"
                );
            }
        }
        Ok(())
    }

    pub fn new(config: Arc<Config>) -> Result<Self> {
        Ok(Self {
            config,
            api_base: url::Url::parse("https://api.stripe.com/v1/")?,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()?,
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let mut response = request
            .bearer_auth(&self.config.stripe_key)
            .header("Stripe-Version", STRIPE_VERSION)
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "Billing service rejected the request"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "Billing response exceeded its limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.send(self.client.get(self.api_base.join(path)?).query(query))
            .await
    }

    async fn post(&self, path: &str, form: &[(String, String)], idem: &str) -> Result<Value> {
        self.send(
            self.client
                .post(self.api_base.join(path)?)
                .header("Idempotency-Key", idem)
                .form(form),
        )
        .await
    }

    pub async fn checkout(
        &self,
        db: &Ledger,
        account: &str,
        plan: &str,
        idem: &str,
    ) -> Result<String> {
        ensure!(
            idem.len() <= 80
                && !idem.is_empty()
                && idem.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid checkout request ID"
        );
        let price = self.config.price(plan).context("Unknown plan")?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM checkouts WHERE account=? AND idem=?")
                .bind(account)
                .bind(idem)
                .fetch_optional(&db.0)
                .await?;
        if let Some(existing) = existing {
            let session = self
                .get(&format!("checkout/sessions/{existing}"), &[])
                .await?;
            return Ok(session["url"]
                .as_str()
                .context("Checkout is no longer open")?
                .to_owned());
        }
        let subscription = matches!(plan, "summaries_monthly" | "summaries_annual");
        let mut fields = vec![
            (
                "mode".into(),
                if subscription {
                    "subscription"
                } else {
                    "payment"
                }
                .into(),
            ),
            ("line_items[0][price]".into(), price.into()),
            ("line_items[0][quantity]".into(), "1".into()),
            ("client_reference_id".into(), account.into()),
            (
                "success_url".into(),
                self.config
                    .public_url
                    .join("checkout/complete")?
                    .to_string(),
            ),
            (
                "cancel_url".into(),
                self.config
                    .public_url
                    .join("checkout/cancelled")?
                    .to_string(),
            ),
            ("payment_method_types[0]".into(), "card".into()),
        ];
        if let Some(message) = submit_message(plan) {
            fields.push(("custom_text[submit][message]".into(), message.into()));
        }
        if subscription {
            fields.push((
                "subscription_data[metadata][overtalk_account]".into(),
                account.into(),
            ));
            fields.push((
                "subscription_data[billing_mode][type]".into(),
                "flexible".into(),
            ));
        }
        if let Some(customer) = db.customer(account).await? {
            if subscription {
                let subs = self
                    .get(
                        "subscriptions",
                        &[("customer", &customer), ("limit", "100")],
                    )
                    .await?;
                ensure!(
                    !subs["has_more"].as_bool().unwrap_or(true),
                    "Contact billing support to change your plan"
                );
                let duplicate = subs["data"]
                    .as_array()
                    .context("Invalid subscription list")?
                    .iter()
                    .any(|s| {
                        matches!(
                            s["status"].as_str(),
                            Some("active" | "trialing" | "past_due" | "incomplete")
                        )
                    });
                ensure!(
                    !duplicate,
                    "A subscription already exists; manage it in billing"
                );
            }
            fields.push(("customer".into(), customer));
        } else if !subscription {
            fields.push(("customer_creation".into(), "always".into()));
        }
        let session = self
            .post(
                "checkout/sessions",
                &fields,
                &format!("checkout:{account}:{idem}"),
            )
            .await?;
        let session_id = id(&session)?;
        sqlx::query("INSERT INTO checkouts VALUES(?,?,?,?) ON CONFLICT(account,idem) DO NOTHING")
            .bind(session_id)
            .bind(account)
            .bind(plan)
            .bind(idem)
            .execute(&db.0)
            .await?;
        Ok(session["url"]
            .as_str()
            .context("No Checkout URL returned")?
            .to_owned())
    }

    pub async fn portal(&self, db: &Ledger, account: &str) -> Result<String> {
        let customer = db
            .customer(account)
            .await?
            .context("Complete a purchase first")?;
        let response = self
            .post(
                "billing_portal/sessions",
                &[
                    ("customer".into(), customer),
                    (
                        "return_url".into(),
                        self.config
                            .public_url
                            .join("checkout/complete")?
                            .to_string(),
                    ),
                ],
                &uuid::Uuid::new_v4().to_string(),
            )
            .await?;
        Ok(response["url"]
            .as_str()
            .context("No billing URL returned")?
            .to_owned())
    }

    async fn payment_state(&self, payment: &str) -> Result<(i64, bool)> {
        let intent = self
            .get(
                &format!("payment_intents/{payment}"),
                &[("expand[]", "latest_charge")],
            )
            .await?;
        let charge = &intent["latest_charge"];
        let amount = charge["amount_refunded"]
            .as_i64()
            .context("Charge refund information unavailable")?;
        Ok((
            amount,
            charge["disputed"]
                .as_bool()
                .context("Charge dispute information unavailable")?,
        ))
    }

    async fn fulfill_checkout(&self, db: &Ledger, session_id: &str) -> Result<()> {
        let session = self
            .get(
                &format!("checkout/sessions/{session_id}"),
                &[("expand[]", "line_items")],
            )
            .await?;
        let stored = sqlx::query("SELECT account,plan FROM checkouts WHERE id=?")
            .bind(session_id)
            .fetch_optional(&db.0)
            .await?
            .context("Checkout not registered yet")?;
        let account: String = stored.get("account");
        let plan: String = stored.get("plan");
        ensure!(
            session["livemode"].as_bool() == Some(self.config.live),
            "Wrong billing environment"
        );
        ensure!(
            session["client_reference_id"].as_str() == Some(&account),
            "Checkout account mismatch"
        );
        if session["payment_status"] != "paid" {
            return Ok(());
        }
        db.bind_customer(&account, id(&session["customer"])?)
            .await?;
        if matches!(plan.as_str(), "summaries_monthly" | "summaries_annual") {
            return Ok(());
        } // invoice.paid owns recurring credits.
        let lines = session["line_items"]["data"]
            .as_array()
            .context("Checkout line items missing")?;
        ensure!(
            lines.len() == 1 && !session["line_items"]["has_more"].as_bool().unwrap_or(true),
            "Unexpected checkout line items"
        );
        ensure!(
            id(&lines[0]["price"])? == self.config.price(&plan).context("Unknown product")?,
            "Wrong product"
        );
        ensure!(
            session["currency"] == self.config.currency.as_str() && lines[0]["quantity"] == 1,
            "Unsupported purchase currency or quantity"
        );
        let paid = session["amount_total"]
            .as_i64()
            .filter(|a| *a > 0)
            .context("Purchase was not paid")?;
        let payment = id(&session["payment_intent"])?;
        let (refunded_cents, disputed) = self.payment_state(payment).await?;
        db.grant(Grant {
            source: session_id,
            account: &account,
            cycle: None,
            payment,
            units: STARTER_UNITS,
            paid_cents: paid,
            refunded_cents,
            disputed,
            starts: crate::now(),
            expires: None,
        })
        .await?;
        db.refund(payment, refunded_cents).await?;
        Ok(())
    }

    async fn fulfill_invoice(&self, db: &Ledger, invoice_id: &str) -> Result<()> {
        let invoice = self
            .get(
                &format!("invoices/{invoice_id}"),
                &[("expand[]", "payments")],
            )
            .await?;
        ensure!(
            invoice["livemode"].as_bool() == Some(self.config.live),
            "Wrong billing environment"
        );
        if invoice["status"] != "paid"
            || invoice["currency"] != self.config.currency.as_str()
            || !matches!(
                invoice["billing_reason"].as_str(),
                Some("subscription_create" | "subscription_cycle")
            )
        {
            return Ok(());
        }
        let paid = invoice["amount_paid"]
            .as_i64()
            .filter(|a| *a > 0)
            .context("Invoice has no paid amount")?;
        let sub_id = id(&invoice["parent"]["subscription_details"]["subscription"])?;
        let subscription = self.get(&format!("subscriptions/{sub_id}"), &[]).await?;
        let Some(account) = subscription["metadata"]["overtalk_account"].as_str() else {
            return Ok(());
        };
        ensure!(
            id(&subscription["customer"])? == id(&invoice["customer"])?,
            "Customer mismatch"
        );
        db.bind_customer(account, id(&invoice["customer"])?).await?;
        let payments = invoice["payments"]["data"]
            .as_array()
            .context("Invoice payment information missing")?;
        ensure!(
            payments.len() == 1 && !invoice["payments"]["has_more"].as_bool().unwrap_or(true),
            "Manual review needed for split invoice payment"
        );
        let payment = id(&payments[0]["payment"]["payment_intent"])?;
        let (refunded_cents, disputed) = self.payment_state(payment).await?;
        let lines = self
            .get(&format!("invoices/{invoice_id}/lines"), &[("limit", "100")])
            .await?;
        ensure!(
            !lines["has_more"].as_bool().unwrap_or(true),
            "Invoice has too many lines"
        );
        for line in lines["data"].as_array().context("Invoice lines missing")? {
            let price = id(&line["pricing"]["price_details"]["price"])?;
            let (days, units) = if price == self.config.monthly_price {
                (27..=32, STARTER_UNITS)
            } else if !self.config.annual_price.is_empty() && price == self.config.annual_price {
                (360..=370, STARTER_UNITS * 12)
            } else {
                continue;
            };
            if line["parent"]["subscription_item_details"]["proration"] != false {
                continue;
            }
            ensure!(line["quantity"] == 1, "Unsupported subscription quantity");
            let start = line["period"]["start"]
                .as_i64()
                .context("Missing period start")?;
            let end = line["period"]["end"]
                .as_i64()
                .context("Missing period end")?;
            ensure!(
                days.contains(&((end - start) / 86400)),
                "Unsupported billing period"
            );
            let item = id(&line["parent"]["subscription_item_details"]["subscription_item"])?;
            let source = format!("{invoice_id}:{}", id(line)?);
            let cycle = format!("{item}:{start}:{end}");
            db.grant(Grant {
                source: &source,
                account,
                cycle: Some(&cycle),
                payment,
                units,
                paid_cents: paid,
                refunded_cents,
                disputed,
                starts: start,
                expires: Some(end),
            })
            .await?;
        }
        db.refund(payment, refunded_cents).await?;
        Ok(())
    }

    pub async fn process_event(&self, db: &Ledger, event_id: &str) -> Result<()> {
        let event = self.get(&format!("events/{event_id}"), &[]).await?;
        ensure!(
            event["livemode"].as_bool() == Some(self.config.live)
                && event["api_version"] == STRIPE_VERSION,
            "Webhook version/environment mismatch"
        );
        let object = &event["data"]["object"];
        match event["type"].as_str().unwrap_or_default() {
            "checkout.session.completed" | "checkout.session.async_payment_succeeded" => {
                self.fulfill_checkout(db, id(object)?).await?
            }
            "invoice.paid" => self.fulfill_invoice(db, id(object)?).await?,
            "charge.refunded" => {
                let charge = self.get(&format!("charges/{}", id(object)?), &[]).await?;
                db.refund(
                    id(&charge["payment_intent"])?,
                    charge["amount_refunded"]
                        .as_i64()
                        .context("Missing refunded amount")?,
                )
                .await?;
            }
            "charge.dispute.created" | "charge.dispute.updated" | "charge.dispute.closed" => {
                let dispute = self.get(&format!("disputes/{}", id(object)?), &[]).await?;
                let charge = self
                    .get(&format!("charges/{}", id(&dispute["charge"])?), &[])
                    .await?;
                db.dispute(id(&charge["payment_intent"])?, dispute["status"] != "won")
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summaries_checkouts_state_the_refund_promise() {
        for plan in ["summaries_monthly", "summaries_annual", "summaries_once"] {
            assert_eq!(submit_message(plan), Some(REFUND));
        }
        assert_eq!(submit_message("credits"), None);
        assert!(REFUND.len() < 1200, "Stripe's limit for the submit message");
    }
    #[test]
    fn signatures_require_original_bytes_and_a_recent_single_timestamp() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"secret");
        let mac = hmac::sign(&key, b"1000.{\"id\":\"evt_test\"}");
        let hex: String = mac.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        let header = format!("t=1000,v1={hex}");
        assert!(verify_signature(
            br#"{"id":"evt_test"}"#,
            &header,
            "secret",
            1000
        ));
        assert!(!verify_signature(
            br#"{"id": "evt_test"}"#,
            &header,
            "secret",
            1000
        ));
        assert!(!verify_signature(
            br#"{"id":"evt_test"}"#,
            &header,
            "secret",
            1301
        ));
        assert!(!verify_signature(
            br#"{"id":"evt_test"}"#,
            &format!("t=1000,{header}"),
            "secret",
            1000
        ));
        assert!(!verify_signature(
            b"{}",
            &format!("t=1000,v1={}", "é".repeat(32)),
            "secret",
            1000
        ));
    }
}

#[cfg(test)]
mod fulfillment_tests;
