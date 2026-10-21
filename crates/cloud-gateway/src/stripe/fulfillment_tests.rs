use super::*;
use serde_json::json;
use std::{collections::HashMap, convert::Infallible, sync::Mutex};

struct Fixture {
    stripe: Stripe,
    db: Ledger,
    account: String,
    responses: Arc<Mutex<HashMap<String, Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let responses = Arc::new(Mutex::new(HashMap::<String, Value>::new()));
        let table = responses.clone();
        let make = hyper::service::make_service_fn(move |_| {
            let table = table.clone();
            async move {
                Ok::<_, Infallible>(hyper::service::service_fn(
                    move |req: hyper::Request<hyper::Body>| {
                        assert_eq!(req.headers()["stripe-version"], STRIPE_VERSION);
                        assert_eq!(
                            req.headers()["authorization"],
                            "Bearer test-private-stripe-key"
                        );
                        let reply = table.lock().unwrap().get(req.uri().path()).cloned();
                        async move {
                            Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(if reply.is_some() { 200 } else { 404 })
                                    .body(hyper::Body::from(
                                        reply
                                            .unwrap_or(json!({"error":"missing fixture"}))
                                            .to_string(),
                                    ))
                                    .unwrap(),
                            )
                        }
                    },
                ))
            }
        });
        let task = tokio::spawn(async move {
            hyper::Server::from_tcp(listener)
                .unwrap()
                .serve(make)
                .await
                .unwrap();
        });
        let mut stripe = Stripe::new(Arc::new(crate::config::test_config())).unwrap();
        stripe.api_base = format!("http://{address}/v1/").parse().unwrap();
        let db = Ledger::memory().await.unwrap();
        let account = db
            .create_account(&crate::secret().unwrap(), 10)
            .await
            .unwrap();
        Self {
            stripe,
            db,
            account,
            responses,
            task,
        }
    }
    fn set(&self, path: &str, value: Value) {
        self.responses
            .lock()
            .unwrap()
            .insert(format!("/v1/{path}"), value);
    }
    fn event(&self, event: &str, kind: &str, object: &str) {
        self.set(&format!("events/{event}"),json!({"id":event,"api_version":STRIPE_VERSION,"livemode":false,"type":kind,"data":{"object":{"id":object}}}));
    }
    async fn once(&self, paid: bool) {
        sqlx::query("INSERT INTO checkouts VALUES('cs_once',?,'summaries_once','request1')")
            .bind(&self.account)
            .execute(&self.db.0)
            .await
            .unwrap();
        self.event("evt_once", "checkout.session.completed", "cs_once");
        self.set(
            "checkout/sessions/cs_once",
            json!({
                "id": "cs_once",
                "livemode": false,
                "payment_status": if paid { "paid" } else { "unpaid" },
                "client_reference_id": self.account,
                "customer": "cus_customer",
                "currency": "usd",
                "amount_total": 1000,
                "payment_intent": "pi_once",
                "line_items": {
                    "has_more": false,
                    "data": [{"quantity": 1, "price": {"id": "price_once"}}]
                }
            }),
        );
        self.set(
            "payment_intents/pi_once",
            json!({
                "id": "pi_once",
                "latest_charge": {
                    "id": "ch_once",
                    "amount_refunded": 0,
                    "disputed": false
                }
            }),
        );
    }
    fn monthly(&self, invoice: &str, start: i64) {
        self.event(&format!("evt_{invoice}"), "invoice.paid", invoice);
        self.set(
            &format!("invoices/{invoice}"),
            json!({
                "id": invoice,
                "livemode": false,
                "status": "paid",
                "currency": "usd",
                "billing_reason": "subscription_cycle",
                "amount_paid": 1000,
                "customer": "cus_customer",
                "parent": {
                    "subscription_details": {"subscription": "sub_monthly"}
                },
                "payments": {
                    "has_more": false,
                    "data": [{
                        "payment": {
                            "type": "payment_intent",
                            "payment_intent": "pi_monthly"
                        }
                    }]
                }
            }),
        );
        self.set(
            "subscriptions/sub_monthly",
            json!({
                "id": "sub_monthly",
                "customer": "cus_customer",
                "status": "active",
                "metadata": {"overtalk_account": self.account}
            }),
        );
        self.set(
            &format!("invoices/{invoice}/lines"),
            json!({
                "has_more": false,
                "data": [{
                    "id": format!("il_{invoice}"),
                    "quantity": 1,
                    "pricing": {"price_details": {"price": "price_month"}},
                    "parent": {
                        "subscription_item_details": {
                            "subscription": "sub_monthly",
                            "subscription_item": "si_monthly",
                            "proration": false
                        }
                    },
                    "period": {"start": start, "end": start + 30 * 86400}
                }]
            }),
        );
        self.set(
            "payment_intents/pi_monthly",
            json!({
                "id": "pi_monthly",
                "latest_charge": {
                    "id": "ch_monthly",
                    "amount_refunded": 0,
                    "disputed": false
                }
            }),
        );
    }
}

#[tokio::test]
async fn checkout_is_fulfilled_once_only_after_verified_payment() {
    let f = Fixture::new().await;
    f.once(false).await;
    f.stripe.process_event(&f.db, "evt_once").await.unwrap();
    assert_eq!(
        f.db.balance(&f.account, crate::now())
            .await
            .unwrap()
            .available_micro_usd,
        0
    );
    f.responses
        .lock()
        .unwrap()
        .get_mut("/v1/checkout/sessions/cs_once")
        .unwrap()["payment_status"] = "paid".into();
    f.stripe.process_event(&f.db, "evt_once").await.unwrap();
    f.stripe.process_event(&f.db, "evt_once").await.unwrap();
    assert_eq!(
        f.db.balance(&f.account, crate::now())
            .await
            .unwrap()
            .available_micro_usd,
        STARTER_UNITS
    );
}

#[tokio::test]
async fn historical_invoice_periods_expire_and_different_duplicate_events_do_not_refill() {
    let f = Fixture::new().await;
    f.monthly("in_first", 10);
    f.monthly("in_duplicate", 10);
    f.stripe.process_event(&f.db, "evt_in_first").await.unwrap();
    f.stripe
        .process_event(&f.db, "evt_in_duplicate")
        .await
        .unwrap();
    assert_eq!(
        f.db.balance(&f.account, 20)
            .await
            .unwrap()
            .available_micro_usd,
        STARTER_UNITS
    );
    assert_eq!(
        f.db.balance(&f.account, 30 * 86400 + 10)
            .await
            .unwrap()
            .available_micro_usd,
        0
    );
    f.monthly("in_next", 30 * 86400 + 10);
    f.stripe.process_event(&f.db, "evt_in_next").await.unwrap();
    assert_eq!(
        f.db.balance(&f.account, 30 * 86400 + 11)
            .await
            .unwrap()
            .available_micro_usd,
        STARTER_UNITS
    );
}

#[tokio::test]
async fn refunds_before_fulfillment_and_customer_mismatches_cannot_grant_free_credit() {
    let f = Fixture::new().await;
    f.once(true).await;
    f.set("payment_intents/pi_once",json!({"id":"pi_once","latest_charge":{"id":"ch_once","amount_refunded":1000,"disputed":false}}));
    f.stripe.process_event(&f.db, "evt_once").await.unwrap();
    assert_eq!(
        f.db.balance(&f.account, crate::now())
            .await
            .unwrap()
            .available_micro_usd,
        0
    );
    f.responses
        .lock()
        .unwrap()
        .get_mut("/v1/checkout/sessions/cs_once")
        .unwrap()["client_reference_id"] = "different-account".into();
    assert!(f.stripe.process_event(&f.db, "evt_once").await.is_err());
}

#[tokio::test]
async fn proration_and_unpaid_invoices_never_receive_a_monthly_grant() {
    let f = Fixture::new().await;
    f.monthly("in_proration", 10);
    f.responses
        .lock()
        .unwrap()
        .get_mut("/v1/invoices/in_proration")
        .unwrap()["billing_reason"] = "subscription_update".into();
    f.stripe
        .process_event(&f.db, "evt_in_proration")
        .await
        .unwrap();
    f.monthly("in_unpaid", 10);
    f.responses
        .lock()
        .unwrap()
        .get_mut("/v1/invoices/in_unpaid")
        .unwrap()["status"] = "open".into();
    f.stripe
        .process_event(&f.db, "evt_in_unpaid")
        .await
        .unwrap();
    assert_eq!(
        f.db.balance(&f.account, 20)
            .await
            .unwrap()
            .available_micro_usd,
        0
    );
}
