//! Integer USD microcredits, transactional grants and conservative reservations.
use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{path::Path, time::Duration};

mod keys;
pub use keys::{StaleKey, SummaryKey};

pub const STARTER_UNITS: i64 = 2_000_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Sign in again to use cloud credits")]
    Unauthorized,
    #[error("Cloud credits are exhausted or held by an unfinished request")]
    NoCredits,
    #[error("This request was already submitted; refresh its status before retrying")]
    Duplicate,
    #[error("Cloud access is paused pending a billing review")]
    Suspended,
    #[error("Invalid credit operation")]
    Invalid,
    #[error("Credit storage is unavailable")]
    Storage(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct Ledger(pub(crate) SqlitePool);

pub struct Grant<'a> {
    pub source: &'a str,
    pub account: &'a str,
    pub cycle: Option<&'a str>,
    pub payment: &'a str,
    pub units: i64,
    pub paid_cents: i64,
    pub refunded_cents: i64,
    pub disputed: bool,
    pub starts: i64,
    pub expires: Option<i64>,
}

#[derive(Serialize)]
pub struct Balance {
    pub available_micro_usd: i64,
    pub held_micro_usd: i64,
    pub suspended: bool,
    pub next_expiry: Option<i64>,
}

impl Ledger {
    pub async fn open(path: &Path) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        Self::connect(options).await
    }

    #[cfg(test)]
    pub async fn memory() -> Result<Self, Error> {
        Self::connect(
            SqliteConnectOptions::new()
                .in_memory(true)
                .foreign_keys(true),
        )
        .await
    }

    async fn connect(options: SqliteConnectOptions) -> Result<Self, Error> {
        // A single writer also serializes multi-statement reservations. No await
        // to a payment or AI provider happens inside a database transaction.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!("schema.sql"))
            .execute(&pool)
            .await?;
        Ok(Self(pool))
    }

    pub async fn create_account(&self, token: &str, now: i64) -> Result<String, Error> {
        let id = uuid::Uuid::new_v4().to_string();
        let salt = crate::secret().map_err(|_| Error::Invalid)?;
        sqlx::query("INSERT INTO accounts(id,token_hash,cache_salt,created_at) VALUES(?,?,?,?)")
            .bind(&id)
            .bind(crate::digest(token.as_bytes()))
            .bind(salt)
            .bind(now)
            .execute(&self.0)
            .await?;
        Ok(id)
    }

    pub async fn authenticate(&self, token: &str) -> Result<String, Error> {
        if token.len() != 43 {
            return Err(Error::Unauthorized);
        }
        sqlx::query_scalar("SELECT id FROM accounts WHERE token_hash=? AND revoked=0")
            .bind(crate::digest(token.as_bytes()))
            .fetch_optional(&self.0)
            .await?
            .ok_or(Error::Unauthorized)
    }

    pub async fn cache_salt(&self, account: &str) -> Result<String, Error> {
        Ok(
            sqlx::query_scalar("SELECT cache_salt FROM accounts WHERE id=?")
                .bind(account)
                .fetch_one(&self.0)
                .await?,
        )
    }

    pub async fn rotate(&self, account: &str, token: &str) -> Result<(), Error> {
        let result = sqlx::query("UPDATE accounts SET token_hash=? WHERE id=? AND revoked=0")
            .bind(crate::digest(token.as_bytes()))
            .bind(account)
            .execute(&self.0)
            .await?;
        if result.rows_affected() != 1 {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    pub async fn customer(&self, account: &str) -> Result<Option<String>, Error> {
        Ok(
            sqlx::query_scalar("SELECT customer FROM accounts WHERE id=?")
                .bind(account)
                .fetch_one(&self.0)
                .await?,
        )
    }

    pub async fn bind_customer(&self, account: &str, customer: &str) -> Result<(), Error> {
        if !customer.starts_with("cus_") {
            return Err(Error::Invalid);
        }
        let result = sqlx::query(
            "UPDATE accounts SET customer=? WHERE id=? AND (customer IS NULL OR customer=?)",
        )
        .bind(customer)
        .bind(account)
        .bind(customer)
        .execute(&self.0)
        .await?;
        if result.rows_affected() != 1 {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    pub async fn grant(&self, grant: Grant<'_>) -> Result<bool, Error> {
        if grant.units <= 0
            || grant.units > 100_000_000
            || grant.paid_cents <= 0
            || grant.paid_cents > 100_000_000
            || grant.refunded_cents < 0
            || grant.expires.is_some_and(|end| end <= grant.starts)
        {
            return Err(Error::Invalid);
        }
        let revoked = (grant.units * grant.refunded_cents.min(grant.paid_cents) + grant.paid_cents
            - 1)
            / grant.paid_cents;
        let mut tx = self.0.begin().await?;
        let inserted = sqlx::query("INSERT INTO grants(source,account,cycle,payment,units,paid_cents,starts,expires,revoked) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(grant.source).bind(grant.account).bind(grant.cycle).bind(grant.payment)
            .bind(grant.units).bind(grant.paid_cents).bind(grant.starts).bind(grant.expires)
            .bind(revoked).execute(&mut *tx).await?.rows_affected();
        if inserted == 1 && grant.disputed {
            sqlx::query(
                "INSERT INTO disputes VALUES(?,1) ON CONFLICT(payment) DO UPDATE SET blocked=1",
            )
            .bind(grant.payment)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(inserted == 1)
    }

    pub async fn balance(&self, account: &str, now: i64) -> Result<Balance, Error> {
        let suspended: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM grants g LEFT JOIN disputes d ON d.payment=g.payment WHERE g.account=? AND (g.spent+g.held>g.units-g.revoked OR d.blocked=1))")
            .bind(account).fetch_one(&self.0).await?;
        let row = sqlx::query("SELECT coalesce(sum(max(0,units-revoked-spent-held)),0) available, (SELECT coalesce(sum(held),0) FROM grants WHERE account=?) held, min(expires) expiry FROM grants WHERE account=? AND starts<=? AND (expires IS NULL OR expires>?)")
            .bind(account).bind(account).bind(now).bind(now).fetch_one(&self.0).await?;
        Ok(Balance {
            available_micro_usd: if suspended { 0 } else { row.get("available") },
            held_micro_usd: row.get("held"),
            suspended,
            next_expiry: row.get("expiry"),
        })
    }

    pub async fn reserve(
        &self,
        account: &str,
        client_id: &str,
        units: i64,
        now: i64,
    ) -> Result<String, Error> {
        if units <= 0 || client_id.is_empty() || client_id.len() > 100 {
            return Err(Error::Invalid);
        }
        let mut tx = self.0.begin().await?;
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM grants g LEFT JOIN disputes d ON d.payment=g.payment WHERE g.account=? AND (g.spent+g.held>g.units-g.revoked OR d.blocked=1))")
            .bind(account).fetch_one(&mut *tx).await?;
        if blocked {
            return Err(Error::Suspended);
        }
        let existing: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE account=? AND client_id=?)",
        )
        .bind(account)
        .bind(client_id)
        .fetch_one(&mut *tx)
        .await?;
        if existing {
            return Err(Error::Duplicate);
        }
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM requests WHERE account=? AND status='pending'",
        )
        .bind(account)
        .fetch_one(&mut *tx)
        .await?;
        if pending >= 2 {
            return Err(Error::NoCredits);
        }
        let rows = sqlx::query("SELECT source,units-revoked-spent-held available FROM grants WHERE account=? AND starts<=? AND (expires IS NULL OR expires>?) AND units>revoked+spent+held ORDER BY expires IS NULL,expires,source")
            .bind(account).bind(now).bind(now).fetch_all(&mut *tx).await?;
        let available: i64 = rows.iter().map(|r| r.get::<i64, _>("available")).sum();
        if available < units {
            return Err(Error::NoCredits);
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO requests(id,account,client_id,status,reserved,created_at) VALUES(?,?,?,'pending',?,?)")
            .bind(&id).bind(account).bind(client_id).bind(units).bind(now).execute(&mut *tx).await?;
        let mut left = units;
        for row in rows {
            if left == 0 {
                break;
            }
            let source: String = row.get("source");
            let take = left.min(row.get("available"));
            sqlx::query("UPDATE grants SET held=held+? WHERE source=?")
                .bind(take)
                .bind(&source)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO allocations VALUES(?,?,?)")
                .bind(&id)
                .bind(source)
                .bind(take)
                .execute(&mut *tx)
                .await?;
            left -= take;
        }
        tx.commit().await?;
        Ok(id)
    }

    /// Missing usage leaves a reservation pending. Retrying cannot spend it
    /// again; an operator must reconcile the provider usage before releasing it.
    pub async fn settle(&self, id: &str, charged: i64) -> Result<(), Error> {
        let mut tx = self.0.begin().await?;
        let request = sqlx::query("SELECT status,reserved,charged FROM requests WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if request.get::<String, _>("status") != "pending" {
            return if request.get::<Option<i64>, _>("charged") == Some(charged) {
                Ok(())
            } else {
                Err(Error::Invalid)
            };
        }
        if charged < 0 || charged > request.get("reserved") {
            return Err(Error::Invalid);
        }
        let rows = sqlx::query(
            "SELECT grant_source,units FROM allocations WHERE request=? ORDER BY rowid",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let mut left = charged;
        for row in rows {
            let held: i64 = row.get("units");
            let spent = left.min(held);
            sqlx::query("UPDATE grants SET held=held-?,spent=spent+? WHERE source=?")
                .bind(held)
                .bind(spent)
                .bind(row.get::<String, _>("grant_source"))
                .execute(&mut *tx)
                .await?;
            left -= spent;
        }
        sqlx::query("UPDATE requests SET status='settled',charged=? WHERE id=?")
            .bind(charged)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn refund(&self, payment: &str, refunded_cents: i64) -> Result<(), Error> {
        if refunded_cents < 0 {
            return Err(Error::Invalid);
        }
        sqlx::query("UPDATE grants SET revoked=max(revoked,(units * min(?,paid_cents) + paid_cents - 1) / paid_cents) WHERE payment=?")
            .bind(refunded_cents).bind(payment).execute(&self.0).await?;
        Ok(())
    }

    pub async fn dispute(&self, payment: &str, blocked: bool) -> Result<(), Error> {
        sqlx::query("INSERT INTO disputes VALUES(?,?) ON CONFLICT(payment) DO UPDATE SET blocked=excluded.blocked")
            .bind(payment).bind(blocked).execute(&self.0).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
