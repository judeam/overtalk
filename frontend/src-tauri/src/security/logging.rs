//! Production diagnostics contain locations and levels, never formatted payloads.
//! Detailed failures are returned to the active UI, not persisted as transcripts.
use std::io::Write;

fn safe_record(record: &log::Record<'_>) -> String {
    let target: String = record
        .target()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
        .take(120)
        .collect();
    format!(
        "{} {}:{}",
        record.level(),
        target,
        record.line().unwrap_or(0)
    )
}

pub fn initialize(default_level: &str) {
    let _ =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
            .format(|buffer, record| writeln!(buffer, "{}", safe_record(record)))
            .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_never_format_user_content_or_credentials() {
        let line = safe_record(
            &log::Record::builder()
                .level(log::Level::Error)
                .target("app_lib::speech")
                .line(Some(47))
                .args(format_args!(
                    "transcript: secret patient text; bearer=secret-key"
                ))
                .build(),
        );
        assert_eq!(line, "ERROR app_lib::speech:47");
        assert!(!line.contains("patient") && !line.contains("key"));
    }
}
