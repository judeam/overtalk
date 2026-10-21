//! Server-only billing and inference. This crate is never bundled in the desktop.
pub mod config;
pub mod ledger;
pub mod server;
pub mod stripe;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn secret() -> anyhow::Result<String> {
    use base64::Engine;
    use ring::rand::SecureRandom;
    let mut bytes = [0_u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Random source unavailable"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub fn digest(value: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value))
}

/// Midnight UTC of a YYYY-MM-DD date, as Unix seconds.
pub fn date(value: &str) -> anyhow::Result<i64> {
    let parts: Vec<i64> = value
        .split('-')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| anyhow::anyhow!("Use a YYYY-MM-DD date"))?;
    let [y, m, d] = parts[..] else {
        anyhow::bail!("Use a YYYY-MM-DD date");
    };
    anyhow::ensure!(
        (2000..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d),
        "Use a YYYY-MM-DD date"
    );
    // Days from the civil calendar (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok((era * 146_097 + doe - 719_468) * 86_400)
}

#[cfg(test)]
mod date_tests {
    #[test]
    fn dates_are_midnight_utc() {
        assert!(
            super::date("1970-01-01").is_err(),
            "outside the accepted range"
        );
        assert_eq!(super::date("2026-10-20").unwrap(), 1_792_454_400);
        assert_eq!(
            super::date("2028-02-29").unwrap() + 86_400,
            super::date("2028-03-01").unwrap()
        );
        assert!(super::date("2026-13-01").is_err() && super::date("20-10-2026").is_err());
    }
}
