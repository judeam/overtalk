use super::*;

async fn fixture() -> (Ledger, String) {
    let db = Ledger::memory().await.unwrap();
    let account = db
        .create_account(&crate::secret().unwrap(), 10)
        .await
        .unwrap();
    (db, account)
}

async fn grant(
    db: &Ledger,
    account: &str,
    source: &str,
    cycle: Option<&str>,
    expires: Option<i64>,
) -> bool {
    db.grant(Grant {
        source,
        account,
        cycle,
        payment: "pi_1",
        units: STARTER_UNITS,
        paid_cents: 1000,
        refunded_cents: 0,
        disputed: false,
        starts: 10,
        expires,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn one_time_purchase_does_not_refill_and_paid_periods_do_not_roll_over() {
    let (db, account) = fixture().await;
    assert!(grant(&db, &account, "one-off", None, None).await);
    let request = db.reserve(&account, "a", STARTER_UNITS, 20).await.unwrap();
    db.settle(&request, STARTER_UNITS).await.unwrap();
    assert!(!grant(&db, &account, "one-off", None, None).await);
    assert_eq!(
        db.balance(&account, 100_000)
            .await
            .unwrap()
            .available_micro_usd,
        0
    );
    assert!(
        grant(
            &db,
            &account,
            "invoice-first",
            Some("sub_item:10:30"),
            Some(30)
        )
        .await
    );
    assert!(
        !grant(
            &db,
            &account,
            "duplicate-invoice",
            Some("sub_item:10:30"),
            Some(30)
        )
        .await
    );
    assert_eq!(
        db.balance(&account, 30).await.unwrap().available_micro_usd,
        0
    );
    assert!(db
        .grant(Grant {
            source: "renewal",
            account: &account,
            cycle: Some("sub_item:30:60"),
            payment: "pi_2",
            units: STARTER_UNITS,
            paid_cents: 1000,
            refunded_cents: 0,
            disputed: false,
            starts: 30,
            expires: Some(60)
        })
        .await
        .unwrap());
    assert_eq!(
        db.balance(&account, 31).await.unwrap().available_micro_usd,
        STARTER_UNITS
    );
}

#[tokio::test]
async fn concurrent_reservations_cannot_overspend_or_reuse_an_idempotency_key() {
    let (db, account) = fixture().await;
    grant(&db, &account, "purchase", None, None).await;
    let (a, b) = tokio::join!(
        db.reserve(&account, "a", 1_500_000, 20),
        db.reserve(&account, "b", 1_500_000, 20)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let (request, client) = if let Ok(id) = a {
        (id, "a")
    } else {
        (b.unwrap(), "b")
    };
    assert!(matches!(
        db.reserve(&account, client, 1, 20).await,
        Err(Error::Duplicate)
    ));
    db.settle(&request, 500_000).await.unwrap();
    db.settle(&request, 500_000).await.unwrap();
    assert!(db.settle(&request, 0).await.is_err());
    assert_eq!(
        db.balance(&account, 20).await.unwrap().available_micro_usd,
        1_500_000
    );
}

#[tokio::test]
async fn unknown_usage_stays_reserved_and_cannot_be_settled_above_the_reservation() {
    let (db, account) = fixture().await;
    grant(&db, &account, "purchase", None, None).await;
    let request = db.reserve(&account, "a", STARTER_UNITS, 20).await.unwrap();
    assert_eq!(
        db.balance(&account, 20).await.unwrap().held_micro_usd,
        STARTER_UNITS
    );
    assert!(db.settle(&request, STARTER_UNITS + 1).await.is_err());
    assert!(matches!(
        db.reserve(&account, "b", 1, 20).await,
        Err(Error::NoCredits)
    ));
    db.settle(&request, 0).await.unwrap();
    assert_eq!(
        db.balance(&account, 20).await.unwrap().available_micro_usd,
        STARTER_UNITS
    );
}

#[tokio::test]
async fn refunds_are_cumulative_and_disputes_block_without_destroying_other_grants() {
    let (db, account) = fixture().await;
    grant(&db, &account, "purchase", None, None).await;
    db.refund("pi_1", 250).await.unwrap();
    db.refund("pi_1", 250).await.unwrap();
    db.refund("pi_1", 100).await.unwrap(); // delayed event cannot restore credits
    assert_eq!(
        db.balance(&account, 20).await.unwrap().available_micro_usd,
        1_500_000
    );
    db.dispute("pi_1", true).await.unwrap();
    assert!(matches!(
        db.reserve(&account, "x", 1, 20).await,
        Err(Error::Suspended)
    ));
    db.dispute("pi_1", false).await.unwrap();
    let request = db.reserve(&account, "a", 1_500_000, 20).await.unwrap();
    db.settle(&request, 1_500_000).await.unwrap();
    db.refund("pi_1", 1000).await.unwrap();
    assert!(db.balance(&account, 20).await.unwrap().suspended);
}

#[tokio::test]
async fn credentials_are_hashed_and_cannot_read_another_account() {
    let (db, account) = fixture().await;
    let token = crate::secret().unwrap();
    let other = db.create_account(&token, 10).await.unwrap();
    assert_eq!(db.authenticate(&token).await.unwrap(), other);
    assert!(db.authenticate(&crate::secret().unwrap()).await.is_err());
    grant(&db, &account, "purchase", None, None).await;
    assert_eq!(db.balance(&other, 20).await.unwrap().available_micro_usd, 0);
    let saved: String = sqlx::query_scalar("SELECT token_hash FROM accounts WHERE id=?")
        .bind(other)
        .fetch_one(&db.0)
        .await
        .unwrap();
    assert_ne!(saved, token);
    let new_token = crate::secret().unwrap();
    let account = db.authenticate(&token).await.unwrap();
    db.rotate(&account, &new_token).await.unwrap();
    assert!(db.authenticate(&token).await.is_err());
    assert_eq!(db.authenticate(&new_token).await.unwrap(), account);
}
