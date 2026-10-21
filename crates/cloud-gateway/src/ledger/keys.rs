//! The summaries entitlement and the Privatemode keys handed to subscribers' desktops.
//! Privatemode documents no API for creating keys, so the operator creates them
//! in its portal with monthly token limits and imports them here; retiring a key
//! here does not delete it there.
use super::{Error, Grant, Ledger, STARTER_UNITS};
use serde::Serialize;
use sqlx::Row;

#[derive(Serialize, Debug, PartialEq)]
pub struct SummaryKey {
    #[serde(skip)]
    pub id: String,
    pub key: String,
    pub expires: i64,
}

#[derive(Debug, PartialEq)]
pub struct StaleKey {
    pub id: String,
    pub label: String,
    pub account: String,
}

impl Ledger {
    /// A paid subscription period covers `now`: a subscription or invoiced cycle that has
    /// started and not ended, and is neither fully refunded nor disputed.
    pub async fn entitled(&self, account: &str, now: i64) -> Result<bool, Error> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM grants g LEFT JOIN disputes d ON d.payment=g.payment WHERE g.account=? AND g.cycle IS NOT NULL AND g.starts<=? AND g.expires>? AND g.revoked<g.units AND coalesce(d.blocked,0)=0)")
            .bind(account).bind(now).bind(now).fetch_one(&self.0).await?)
    }

    /// An invoiced period for one seat, outside Stripe (team plans).
    pub async fn entitle(
        &self,
        account: &str,
        reference: &str,
        paid_cents: i64,
        starts: i64,
        expires: i64,
    ) -> Result<bool, Error> {
        if reference.is_empty()
            || reference.len() > 80
            || !reference
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_/.".contains(&c))
        {
            return Err(Error::Invalid);
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=? AND revoked=0)")
                .bind(account)
                .fetch_one(&self.0)
                .await?;
        if !exists {
            return Err(Error::Unauthorized);
        }
        self.grant(Grant {
            source: &format!("invoice:{reference}:{account}"),
            account,
            cycle: Some(&format!("invoice:{reference}:{account}:{starts}:{expires}")),
            payment: &format!("invoice:{reference}"),
            units: STARTER_UNITS,
            paid_cents,
            refunded_cents: 0,
            disputed: false,
            starts,
            expires: Some(expires),
        })
        .await
    }

    pub async fn import_key(
        &self,
        label: &str,
        secret: &str,
        expires: i64,
        now: i64,
    ) -> Result<bool, Error> {
        if label.is_empty()
            || label.len() > 120
            || label.chars().any(char::is_control)
            || !(16..=512).contains(&secret.len())
            || secret.chars().any(|c| c.is_whitespace() || c.is_control())
            || expires <= now
        {
            return Err(Error::Invalid);
        }
        let inserted = sqlx::query(
            "INSERT INTO summary_keys(id,label,secret,expires) VALUES(?,?,?,?) ON CONFLICT DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(label)
        .bind(secret)
        .bind(expires)
        .execute(&self.0)
        .await?
        .rows_affected();
        Ok(inserted == 1)
    }

    /// The end of the paid period covering `now`, under the same rules as `entitled`.
    async fn paid_until(&self, account: &str, now: i64) -> Result<Option<i64>, Error> {
        Ok(sqlx::query_scalar("SELECT max(g.expires) FROM grants g LEFT JOIN disputes d ON d.payment=g.payment WHERE g.account=? AND g.cycle IS NOT NULL AND g.starts<=? AND g.expires>? AND g.revoked<g.units AND coalesce(d.blocked,0)=0")
            .bind(account).bind(now).bind(now).fetch_one(&self.0).await?)
    }

    /// The account's live key, assigning an unused one on first request. An
    /// expired assignment is retired first; Privatemode no longer accepts it.
    pub async fn summary_key(&self, account: &str, now: i64) -> Result<Option<SummaryKey>, Error> {
        let paid_until = self.paid_until(account, now).await?.unwrap_or(i64::MAX);
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "UPDATE summary_keys SET retired=? WHERE account=? AND retired IS NULL AND expires<=?",
        )
        .bind(now)
        .bind(account)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let mut row = sqlx::query(
            "SELECT id,secret,expires FROM summary_keys WHERE account=? AND retired IS NULL",
        )
        .bind(account)
        .fetch_optional(&mut *tx)
        .await?;
        if row.is_none() {
            // A key keeps working at Privatemode until it expires, whatever happens
            // to the subscription, so prefer the shortest-lived key that covers the
            // paid period; failing that, the longest-lived, which is renewed sooner.
            let free: Option<String> = sqlx::query_scalar("SELECT id FROM summary_keys WHERE account IS NULL AND retired IS NULL AND expires>? ORDER BY expires<?, CASE WHEN expires>=? THEN expires END, expires DESC LIMIT 1")
                .bind(now)
                .bind(paid_until)
                .bind(paid_until)
                .fetch_optional(&mut *tx)
                .await?;
            if let Some(id) = free {
                sqlx::query(
                    "UPDATE summary_keys SET account=?, assigned=? WHERE id=? AND account IS NULL",
                )
                .bind(account)
                .bind(now)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
                row = sqlx::query("SELECT id,secret,expires FROM summary_keys WHERE id=?")
                    .bind(&id)
                    .fetch_optional(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(row.map(|r| SummaryKey {
            id: r.get("id"),
            key: r.get("secret"),
            expires: r.get("expires"),
        }))
    }

    /// Keys still assigned to accounts without a paid period: delete these in the
    /// Privatemode portal, then retire them here.
    pub async fn stale_keys(&self, now: i64) -> Result<Vec<StaleKey>, Error> {
        let rows = sqlx::query("SELECT id,label,account FROM summary_keys WHERE account IS NOT NULL AND retired IS NULL ORDER BY assigned")
            .fetch_all(&self.0)
            .await?;
        let mut stale = Vec::new();
        for row in rows {
            let account: String = row.get("account");
            if !self.entitled(&account, now).await? {
                stale.push(StaleKey {
                    id: row.get("id"),
                    label: row.get("label"),
                    account,
                });
            }
        }
        Ok(stale)
    }

    pub async fn retire_key(&self, id: &str, now: i64) -> Result<(), Error> {
        let result =
            sqlx::query("UPDATE summary_keys SET retired=? WHERE id=? AND retired IS NULL")
                .bind(now)
                .bind(id)
                .execute(&self.0)
                .await?;
        if result.rows_affected() != 1 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    pub async fn free_keys(&self, now: i64) -> Result<i64, Error> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM summary_keys WHERE account IS NULL AND retired IS NULL AND expires>?",
        )
        .bind(now)
        .fetch_one(&self.0)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn account(db: &Ledger) -> String {
        db.create_account(&crate::secret().unwrap(), 0)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn only_a_current_paid_period_entitles_and_refunds_or_disputes_end_it() {
        let db = Ledger::memory().await.unwrap();
        let a = account(&db).await;
        assert!(!db.entitled(&a, 100).await.unwrap());
        // A one-time purchase has no cycle and does not entitle to an ongoing key.
        db.grant(Grant {
            source: "once",
            account: &a,
            cycle: None,
            payment: "pi_once",
            units: STARTER_UNITS,
            paid_cents: 1000,
            refunded_cents: 0,
            disputed: false,
            starts: 0,
            expires: None,
        })
        .await
        .unwrap();
        assert!(!db.entitled(&a, 100).await.unwrap());
        assert!(db.entitle(&a, "INV-2026-001", 1500, 50, 200).await.unwrap());
        assert!(!db.entitled(&a, 40).await.unwrap(), "not before it starts");
        assert!(db.entitled(&a, 100).await.unwrap());
        assert!(!db.entitled(&a, 200).await.unwrap(), "not once it ends");
        db.refund("invoice:INV-2026-001", 1500).await.unwrap();
        assert!(
            !db.entitled(&a, 100).await.unwrap(),
            "a full refund ends it"
        );
        let b = account(&db).await;
        db.grant(Grant {
            source: "inv:b",
            account: &b,
            cycle: Some("si:0:300"),
            payment: "pi_b",
            units: STARTER_UNITS,
            paid_cents: 1000,
            refunded_cents: 0,
            disputed: true,
            starts: 0,
            expires: Some(300),
        })
        .await
        .unwrap();
        assert!(!db.entitled(&b, 100).await.unwrap(), "a dispute ends it");
        assert!(db.entitle("missing", "INV-1", 1500, 0, 10).await.is_err());
        assert!(db
            .entitle(&a, "bad ref with spaces", 1500, 0, 10)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn each_account_keeps_one_live_key_and_expired_keys_are_replaced() {
        let db = Ledger::memory().await.unwrap();
        let (a, b) = (account(&db).await, account(&db).await);
        assert_eq!(db.summary_key(&a, 10).await.unwrap(), None, "empty pool");
        assert!(db
            .import_key("pm-1", "pm_key_aaaaaaaaaaaaaaaa", 100, 10)
            .await
            .unwrap());
        assert!(db
            .import_key("pm-2", "pm_key_bbbbbbbbbbbbbbbb", 1000, 10)
            .await
            .unwrap());
        assert!(
            !db.import_key("pm-2 again", "pm_key_bbbbbbbbbbbbbbbb", 1000, 10)
                .await
                .unwrap(),
            "a key is imported once"
        );
        assert!(db
            .import_key("expired", "pm_key_cccccccccccccccc", 5, 10)
            .await
            .is_err());
        assert!(db
            .import_key("spaces", "pm key with spaces here", 500, 10)
            .await
            .is_err());
        let first = db.summary_key(&a, 10).await.unwrap().unwrap();
        assert_eq!(
            first.key, "pm_key_bbbbbbbbbbbbbbbb",
            "the longest-lived key first"
        );
        assert_eq!(
            db.summary_key(&a, 20).await.unwrap().unwrap(),
            first,
            "the same key again"
        );
        let second = db.summary_key(&b, 20).await.unwrap().unwrap();
        assert_eq!(second.key, "pm_key_aaaaaaaaaaaaaaaa");
        assert_eq!(db.free_keys(20).await.unwrap(), 0);
        // b's key expires; with no free key left, b has none until the operator imports more.
        assert_eq!(db.summary_key(&b, 100).await.unwrap(), None);
        assert!(db
            .import_key("pm-3", "pm_key_dddddddddddddddd", 2000, 100)
            .await
            .unwrap());
        assert_eq!(
            db.summary_key(&b, 101).await.unwrap().unwrap().key,
            "pm_key_dddddddddddddddd"
        );
    }

    #[tokio::test]
    async fn a_subscriber_gets_the_shortest_key_that_covers_the_paid_period() {
        let db = Ledger::memory().await.unwrap();
        let (a, b) = (account(&db).await, account(&db).await);
        db.entitle(&a, "INV-1", 1500, 0, 500).await.unwrap();
        db.entitle(&b, "INV-2", 1500, 0, 5000).await.unwrap();
        for (label, secret, expires) in [
            ("short", "pm_key_aaaaaaaaaaaaaaaa", 400),
            ("month", "pm_key_bbbbbbbbbbbbbbbb", 600),
            ("year", "pm_key_cccccccccccccccc", 9000),
        ] {
            db.import_key(label, secret, expires, 10).await.unwrap();
        }
        assert_eq!(
            db.summary_key(&a, 10).await.unwrap().unwrap().expires,
            600,
            "not the year-long key, which would outlive the period"
        );
        assert_eq!(db.summary_key(&b, 10).await.unwrap().unwrap().expires, 9000);
        let c = account(&db).await;
        db.entitle(&c, "INV-3", 1500, 0, 5000).await.unwrap();
        assert_eq!(
            db.summary_key(&c, 10).await.unwrap().unwrap().expires,
            400,
            "without a covering key, the longest left, renewed when it expires"
        );
    }

    #[tokio::test]
    async fn keys_of_accounts_without_a_paid_period_are_listed_until_retired() {
        let db = Ledger::memory().await.unwrap();
        let (paid, lapsed) = (account(&db).await, account(&db).await);
        db.entitle(&paid, "INV-1", 1500, 0, 1000).await.unwrap();
        db.entitle(&lapsed, "INV-2", 1500, 0, 50).await.unwrap();
        db.import_key("k1", "pm_key_aaaaaaaaaaaaaaaa", 5000, 0)
            .await
            .unwrap();
        db.import_key("k2", "pm_key_bbbbbbbbbbbbbbbb", 4000, 0)
            .await
            .unwrap();
        db.summary_key(&paid, 10).await.unwrap().unwrap();
        let lapsed_key = db.summary_key(&lapsed, 10).await.unwrap().unwrap();
        assert_eq!(db.stale_keys(10).await.unwrap(), vec![]);
        let stale = db.stale_keys(100).await.unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(
            (stale[0].id.as_str(), stale[0].account.as_str()),
            (lapsed_key.id.as_str(), lapsed.as_str())
        );
        db.retire_key(&stale[0].id, 100).await.unwrap();
        assert!(
            db.retire_key(&stale[0].id, 100).await.is_err(),
            "retired once"
        );
        assert_eq!(db.stale_keys(100).await.unwrap(), vec![]);
        assert_eq!(
            db.summary_key(&lapsed, 100).await.unwrap(),
            None,
            "no free key left for a new assignment"
        );
    }
}
