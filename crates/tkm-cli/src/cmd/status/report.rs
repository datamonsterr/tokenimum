//! Shared read-only reporting helpers.
use crate::{ctx::Ctx, daemon, error::CliError};
use std::time::{SystemTime, UNIX_EPOCH};
use tkm_core::model::{Host, Timestamp};
use tkm_store::Store;

#[must_use]
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
pub(crate) fn store_error(ctx: &Ctx<'_>, source: tkm_store::StoreError) -> CliError {
    CliError::Database {
        path: ctx.paths.db.clone(),
        source,
    }
}
pub(crate) fn read_store(ctx: &Ctx<'_>) -> Result<Store, CliError> {
    Store::from_read_only_connection(daemon::open_db(&ctx.paths)?).map_err(|e| store_error(ctx, e))
}
pub(crate) fn json_error(error: serde_json::Error) -> CliError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error).into()
}
#[must_use]
pub(crate) fn line(text: &str) -> String {
    tkm_core::inject::sanitize_line(text)
}
#[must_use]
pub(crate) fn host_name(host: Host) -> &'static str {
    match host {
        Host::Claude => "Claude Code",
        Host::Codex => "Codex",
        Host::Agy => "Antigravity",
        Host::Cursor => "Cursor",
        Host::Kimi => "Kimi Code",
        Host::Deepseek => "Codewhale",
        Host::Opencode => "opencode",
        Host::Pi => "Pi",
        _ => "unknown harness",
    }
}
// UTC civil dates without a new date/time dependency. Bound the supported calendar first.
#[must_use]
pub(crate) fn time(at: Option<Timestamp>) -> String {
    let Some(Timestamp(ms)) = at else {
        return "unknown time".into();
    };
    let seconds = ms.div_euclid(1000);
    let mut days = seconds.div_euclid(86_400);
    if !(-719_162..=2_932_896).contains(&days) {
        return "unknown time".into();
    }
    let mut year = 1970_i64;
    while days < 0 {
        year -= 1;
        days += year_days(year);
    }
    while days >= year_days(year) {
        days -= year_days(year);
        year += 1;
    }
    let leap = year_days(year) == 366;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for length in months {
        if days < length {
            break;
        }
        days -= length;
        month += 1;
    }
    let tod = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{:02} {:02}:{:02}",
        days + 1,
        tod / 3600,
        tod % 3600 / 60
    )
}
fn year_days(year: i64) -> i64 {
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        366
    } else {
        365
    }
}

#[cfg(test)]
mod tests {
    use super::time;
    use tkm_core::model::Timestamp;
    #[test]
    fn utc_calendar_handles_epoch_negative_and_leap_days() {
        assert_eq!(time(Some(Timestamp(0))), "1970-01-01 00:00");
        assert_eq!(time(Some(Timestamp(-1))), "1969-12-31 23:59");
        assert_eq!(time(Some(Timestamp(951_782_400_000))), "2000-02-29 00:00");
        assert_eq!(time(None), "unknown time");
        assert_eq!(time(Some(Timestamp(i64::MAX))), "unknown time");
    }
}
