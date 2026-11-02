//! On/off switch for every job that works through the history (`TRIFORCE_BACKFILL_<JOB>`).
//! `off` = the job only does recent work: games or matches from the last `RECENT_DAYS`.
//! A new history job adds a variant here, its name in `env::SETTINGS` and a line in
//! `env/example.env` (a test checks both)

use color_eyre::{Result, eyre::eyre};

/// What "recent" means for every backfill switch set to `off`
pub const RECENT_DAYS: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backfill {
    /// v2 matches full pass, `scheduleEvents` from 2023-10-01
    Matches,
    /// v2 game details queue
    Details,
    /// v2 game frames queue
    Frames,
}

impl Backfill {
    pub const ALL: [Backfill; 3] = [Backfill::Matches, Backfill::Details, Backfill::Frames];

    pub fn var(self) -> &'static str {
        match self {
            Backfill::Matches => "TRIFORCE_BACKFILL_MATCHES",
            Backfill::Details => "TRIFORCE_BACKFILL_DETAILS",
            Backfill::Frames => "TRIFORCE_BACKFILL_FRAMES",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Backfill::Matches => "matches",
            Backfill::Details => "details",
            Backfill::Frames => "frames",
        }
    }

    /// Matches and details did the whole history before the switch existed: they keep it
    pub fn default_on(self) -> bool {
        match self {
            Backfill::Matches | Backfill::Details => true,
            Backfill::Frames => false,
        }
    }

    /// `on`, `off`, or unset for the default
    pub fn parse(self, value: Option<&str>) -> Result<bool> {
        match value {
            None => Ok(self.default_on()),
            Some(v) => crate::env::parse_switch(self.var(), Some(v)),
        }
    }

    pub fn read(self) -> Result<bool> {
        match crate::env::var(self.var()) {
            Err(std::env::VarError::NotPresent) => self.parse(None),
            Err(e) => Err(eyre!("{}: {e}", self.var())),
            Ok(v) => self.parse(Some(&v)),
        }
    }
}

/// Every switch, read at start so a bad value stops the app before any work
pub fn all_from_env() -> Result<Vec<(Backfill, bool)>> {
    Backfill::ALL
        .into_iter()
        .map(|b| Ok((b, b.read()?)))
        .collect()
}

pub fn is_on(values: &[(Backfill, bool)], job: Backfill) -> bool {
    values
        .iter()
        .find(|(b, _)| *b == job)
        .map_or(job.default_on(), |(_, on)| *on)
}

/// `backfill: matches on, details on, frames off`
pub fn log_line(values: &[(Backfill, bool)]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|(b, on)| format!("{} {}", b.name(), if *on { "on" } else { "off" }))
        .collect();
    format!("backfill: {}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_values_and_defaults() {
        assert!(Backfill::Matches.parse(None).unwrap());
        assert!(Backfill::Details.parse(None).unwrap());
        assert!(!Backfill::Frames.parse(None).unwrap());
        assert!(Backfill::Frames.parse(Some("on")).unwrap());
        assert!(!Backfill::Matches.parse(Some(" OFF ")).unwrap());
        let e = Backfill::Details.parse(Some("yes")).unwrap_err();
        assert!(e.to_string().contains("TRIFORCE_BACKFILL_DETAILS"), "{e}");
    }

    #[test]
    fn every_switch_is_a_known_setting() {
        let example = include_str!("../env/example.env");
        for b in Backfill::ALL {
            assert!(crate::env::SETTINGS.contains(&b.var()), "{}", b.var());
            let line = format!("#{}={}", b.var(), if b.default_on() { "on" } else { "off" });
            assert!(example.lines().any(|l| l == line), "{line}");
        }
    }

    #[test]
    fn log_line_lists_every_switch() {
        let values = [
            (Backfill::Matches, true),
            (Backfill::Details, true),
            (Backfill::Frames, false),
        ];
        assert_eq!(
            log_line(&values),
            "backfill: matches on, details on, frames off"
        );
        assert!(is_on(&values, Backfill::Details));
        assert!(!is_on(&values, Backfill::Frames));
        assert!(!is_on(&[], Backfill::Frames));
    }
}
