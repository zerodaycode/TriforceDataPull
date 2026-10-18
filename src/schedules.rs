//! Loop schedules: defaults here, `TRIFORCE_CRON_<LOOP>` changes one (faster ticks for tests)

use std::str::FromStr;

use chrono::Utc;
use color_eyre::{Result, eyre::eyre};
use cron::Schedule;

use crate::client::log;

/// Every 2 hours at :15, away from the v1 loops
const V2: &str = "0 15 */2 ? * *";
/// Every 2 hours
const LEAGUES: &str = "0 0 */2 ? * *";
/// Every 2 hours
const TOURNAMENTS: &str = "0 0 */2 ? * *";
/// Every hour (cron can't express 90 min)
const TEAMS: &str = "0 0 * ? * *";
/// Every 30 minutes, current page of the schedule
const SCHEDULE: &str = "0 */30 * ? * *";
/// Every 2 minutes
const LIVE: &str = "0 */2 * ? * *";

/// Every loop by `TRIFORCE_CRON_<name>` with its default. A new loop goes here, in `Schedules`
/// and in `env/example.env` (a test checks the example)
pub const LOOPS: &[(&str, &str)] = &[
    ("V2", V2),
    ("LEAGUES", LEAGUES),
    ("TOURNAMENTS", TOURNAMENTS),
    ("TEAMS", TEAMS),
    ("SCHEDULE", SCHEDULE),
    ("LIVE", LIVE),
];

pub struct Schedules {
    pub v2: Schedule,
    pub leagues: Schedule,
    pub tournaments: Schedule,
    pub teams: Schedule,
    pub schedule: Schedule,
    pub live: Schedule,
}

/// Every loop schedule, read at start so a bad value stops the app before any work
pub fn all_from_env() -> Result<Schedules> {
    Ok(Schedules {
        v2: loop_from_env("V2")?,
        leagues: loop_from_env("LEAGUES")?,
        tournaments: loop_from_env("TOURNAMENTS")?,
        teams: loop_from_env("TEAMS")?,
        schedule: loop_from_env("SCHEDULE")?,
        live: loop_from_env("LIVE")?,
    })
}

/// A loop of `LOOPS` with its default
fn loop_from_env(name: &str) -> Result<Schedule> {
    let default = LOOPS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
        .ok_or_else(|| eyre!("loop {name} missing from LOOPS"))?;
    from_env(name, default)
}

/// `TRIFORCE_CRON_<name>` when set, else `default`
fn from_env(name: &str, default: &str) -> Result<Schedule> {
    let var = format!("TRIFORCE_CRON_{name}");
    let value = match crate::env::var(&var) {
        Ok(v) => Some(v),
        Err(std::env::VarError::NotPresent) => None,
        Err(e) => return Err(eyre!("{var}: {e}")),
    };
    parse(&var, value.as_deref(), default)
}

fn parse(var: &str, value: Option<&str>, default: &str) -> Result<Schedule> {
    let (text, overridden) = match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => (default, false),
        Some(v) => (v, true),
    };
    let schedule = Schedule::from_str(text)
        .map_err(|e| eyre!("{var}: {text:?} is not a cron expression: {e}"))?;
    // a past year or an impossible date: the loop would run with no pause
    if schedule.upcoming(Utc).next().is_none() {
        return Err(eyre!("{var}: {text:?} never fires"));
    }
    if overridden {
        log(format!("{var}: {text} (default {default})"));
    }
    Ok(schedule)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        for (name, default) in LOOPS {
            parse(name, None, default).unwrap();
        }
        // every Schedules field has its LOOPS entry
        all_from_env().unwrap();
        assert!(loop_from_env("NOPE").is_err());
    }

    #[test]
    fn default_or_env() {
        assert_eq!(
            parse("TRIFORCE_CRON_V2", None, V2).unwrap(),
            Schedule::from_str(V2).unwrap()
        );
        assert_eq!(
            parse("TRIFORCE_CRON_V2", Some(" "), V2).unwrap(),
            Schedule::from_str(V2).unwrap()
        );
        assert_eq!(
            parse("TRIFORCE_CRON_V2", Some("0 * * ? * *"), V2).unwrap(),
            Schedule::from_str("0 * * ? * *").unwrap()
        );
        let err = parse("TRIFORCE_CRON_V2", Some("every minute"), V2).unwrap_err();
        assert!(err.to_string().starts_with("TRIFORCE_CRON_V2:"));
    }

    #[test]
    fn never_fires_is_an_error() {
        let err = parse("TRIFORCE_CRON_LIVE", Some("0 0 0 1 1 ? 2020"), LIVE).unwrap_err();
        assert!(err.to_string().contains("never fires"));
    }
}
