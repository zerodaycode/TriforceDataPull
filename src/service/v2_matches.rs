//! v2 matches: `scheduleEvents` → `v2_event*`, `v2_game`, `v2_stream`, on their own loop.
//! The last 3 days and the next year on every tick, the whole history at most once a day

use std::{collections::HashSet, time::Duration};

use canyon_sql::date_time::NaiveDateTime;
use chrono::{NaiveDate, TimeDelta, Utc};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::{
    client::{
        adapters::v2::events::from_schedule,
        gql::{
            Gql, GqlError,
            ops::schedule_events::{self, GetScheduleEvents},
        },
        log,
    },
    dao::v2::{
        details::sweep_results,
        events::{WriteStats, job_last_ok, locale_known, set_job_ok, write_events},
    },
};

/// `scheduleEvents` has nothing before Worlds 2023 (2023-10-15)
const HISTORY_START: &str = "2023-10-01";
const FULL_JOB: &str = "matches_full";
/// Last failed full pass: a broken one is tried again after FULL_RETRY, not on every tick
const FULL_TRY_JOB: &str = "matches_full_try";
/// A few minutes under 24 h: tick jitter doesn't push the full pass to the next tick
const FULL_EVERY: TimeDelta = TimeDelta::minutes(24 * 60 - 5);
const FULL_RETRY: TimeDelta = TimeDelta::hours(6);
const RECENT_DAYS: i64 = 3;
const AHEAD_DAYS: i64 = 365;
/// ~115 pages today: far more means the paging is broken
const MAX_PAGES: usize = 3_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// From 2023-10-01
    Full,
    /// The last 3 days and the next year
    Recent,
}

impl std::fmt::Display for Pass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Pass::Full => "full",
            Pass::Recent => "recent",
        })
    }
}

/// The full pass is due when the last good one is ~24 h old (or never ran) and no failed one
/// was tried in the last 6 h
pub fn full_due(
    last_ok: Option<NaiveDateTime>,
    last_try: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> bool {
    let old = |t: Option<NaiveDateTime>, every: TimeDelta| t.is_none_or(|t| now - t >= every);
    old(last_ok, FULL_EVERY) && old(last_try, FULL_RETRY)
}

/// `eventDateStart`, `eventDateEnd` (both needed by the API)
pub fn date_range(pass: Pass, now: NaiveDateTime) -> (String, String) {
    let day = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let start = match pass {
        Pass::Full => HISTORY_START.to_string(),
        Pass::Recent => day((now - TimeDelta::days(RECENT_DAYS)).date()),
    };
    (start, day((now + TimeDelta::days(AHEAD_DAYS)).date()))
}

/// Errors that can go away: network (timeouts were already retried by the client), body, 5xx
fn worth_retrying(e: &GqlError) -> bool {
    match e {
        GqlError::Network(n) => !n.is_timeout(),
        GqlError::Body(_) => true,
        GqlError::Http { status } => *status >= 500,
        _ => false,
    }
}

/// One retry after 5 s for errors that can go away, the others at once
async fn page(gql: &Gql, vars: &schedule_events::Vars) -> Result<schedule_events::Data, GqlError> {
    match gql.call::<GetScheduleEvents>(vars).await {
        Ok(a) => Ok(a.data),
        Err(e) if worth_retrying(&e) => {
            log(format!("v2 matches: page failed, trying again: {e}"));
            tokio::time::sleep(Duration::from_secs(5)).await;
            gql.call::<GetScheduleEvents>(vars).await.map(|a| a.data)
        }
        Err(e) => Err(e),
    }
}

/// The token of the next page, `None` after the last one. A repeated token or too many pages
/// mean the paging is broken
fn next_token(
    newer: Option<String>,
    seen: &mut HashSet<String>,
    pages: usize,
) -> Result<Option<String>> {
    match newer {
        None => Ok(None),
        Some(t) if !seen.insert(t.clone()) => Err(eyre!("page token repeated")),
        Some(_) if pages >= MAX_PAGES => Err(eyre!("over {MAX_PAGES} pages")),
        Some(t) => Ok(Some(t)),
    }
}

#[derive(Debug, Default)]
struct LocaleStats {
    pages: usize,
    skipped: usize,
    write: WriteStats,
}

/// Every page of one locale, written as it comes
async fn pass_locale(gql: &Gql, locale: &str, start: &str, end: &str) -> Result<LocaleStats> {
    let mut stats = LocaleStats::default();
    let mut token: Option<String> = None;
    let mut seen = HashSet::new();
    loop {
        let vars = schedule_events::Vars::new(locale, start, end, token.take());
        let mut data = page(gql, &vars)
            .await
            .map_err(color_eyre::Report::new)
            .wrap_err_with(|| format!("scheduleEvents {locale}, page {}", stats.pages + 1))?;
        stats.pages += 1;
        let newer = data.esports.pages.newer.take();
        let rows = from_schedule(data);
        stats.skipped += rows.skipped;
        stats.write += write_events(&rows, locale)
            .await
            .wrap_err_with(|| format!("write scheduleEvents {locale}, page {}", stats.pages))?;
        match next_token(newer, &mut seen, stats.pages)
            .wrap_err_with(|| format!("scheduleEvents {locale}"))?
        {
            None => return Ok(stats),
            Some(t) => token = Some(t),
        }
    }
}

/// One pass over the locales. en-US is the structure: if it fails, the pass fails. `who` names
/// the loop in the log
async fn pass(
    gql: &Gql,
    who: &str,
    locales: &[String],
    pass: Pass,
    now: NaiveDateTime,
) -> Result<()> {
    let (start, end) = date_range(pass, now);
    for locale in locales {
        if locale != "en-US" && !locale_known(locale).await? {
            log(format!(
                "{who}: {locale} skipped, the reference loop has no names in it"
            ));
            continue;
        }
        match pass_locale(gql, locale, &start, &end).await {
            Ok(s) => log(format!(
                "{who} ({pass}) {locale}: {} pages, {} events, {} went back, {} skipped (bad id or value), {} moved games, {} unknown tournaments",
                s.pages,
                s.write.written,
                s.write.went_back,
                s.skipped,
                s.write.moved_games,
                s.write.unknown_tournaments
            )),
            Err(e) if locale == "en-US" => return Err(e),
            Err(e) => log(format!("{who} ({pass}) {locale} stopped: {e:#}")),
        }
    }
    Ok(())
}

/// The recent pass in en-US only: the live loop's check that a match that left the live
/// answer is `completed` (state and outcome don't depend on the locale)
pub async fn recent_en_us(gql: &Gql) -> Result<()> {
    pass(
        gql,
        "v2 live end check",
        &["en-US".to_string()],
        Pass::Recent,
        Utc::now().naive_utc(),
    )
    .await
}

/// The recent pass on every tick, so live and upcoming matches never wait on the history.
/// Then the full pass when it's due
pub async fn matches_once(gql: &Gql, locales: &[String]) -> Result<()> {
    let now = Utc::now().naive_utc();
    pass(gql, "v2 matches", locales, Pass::Recent, now).await?;
    // game winners of sweeps, for games whose sides are already stored
    match sweep_results().await {
        Ok(0) => {}
        Ok(n) => log(format!("v2 matches: {n} game results from sweeps")),
        Err(e) => log(format!("v2 matches: sweep results not written: {e:#}")),
    }
    if !full_due(
        job_last_ok(FULL_JOB).await?,
        job_last_ok(FULL_TRY_JOB).await?,
        now,
    ) {
        return Ok(());
    }
    match pass(gql, "v2 matches", locales, Pass::Full, now).await {
        Ok(()) => set_job_ok(FULL_JOB, now).await,
        Err(e) => {
            if let Err(w) = set_job_ok(FULL_TRY_JOB, now).await {
                log(format!(
                    "v2 matches: the failed full pass wasn't recorded: {w:#}"
                ));
            }
            Err(e.wrap_err(format!(
                "full pass, next try in {} h",
                FULL_RETRY.num_hours()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn full_once_a_day() {
        let now = at("2026-10-18 12:00");
        assert!(full_due(None, None, now));
        assert!(!full_due(Some(at("2026-10-17 13:00")), None, now));
        assert!(full_due(Some(at("2026-10-17 12:00")), None, now));
        // a few minutes early (tick jitter) still counts as a day
        assert!(full_due(Some(at("2026-10-17 12:04")), None, now));
        // a clock that went back doesn't start a full pass on every tick
        assert!(!full_due(Some(at("2026-10-19 12:00")), None, now));
        // a full pass that failed 2 h ago waits, 6 h ago goes again
        assert!(!full_due(None, Some(at("2026-10-18 10:00")), now));
        assert!(full_due(None, Some(at("2026-10-18 06:00")), now));
    }

    #[test]
    fn paging_stops() {
        let mut seen = HashSet::new();
        assert_eq!(next_token(None, &mut seen, 1).unwrap(), None);
        assert_eq!(
            next_token(Some("a".into()), &mut seen, 1).unwrap(),
            Some("a".into())
        );
        assert!(next_token(Some("a".into()), &mut seen, 2).is_err());
        assert!(next_token(Some("b".into()), &mut seen, MAX_PAGES).is_err());
    }

    #[test]
    fn retries_only_what_can_go_away() {
        assert!(worth_retrying(&GqlError::Http { status: 503 }));
        assert!(!worth_retrying(&GqlError::Http { status: 404 }));
        assert!(!worth_retrying(&GqlError::UnknownHash { op: "x" }));
    }

    #[test]
    fn ranges() {
        let now = at("2026-10-18 12:00");
        assert_eq!(
            date_range(Pass::Full, now),
            ("2023-10-01".to_string(), "2027-10-18".to_string())
        );
        assert_eq!(
            date_range(Pass::Recent, now),
            ("2026-10-15".to_string(), "2027-10-18".to_string())
        );
    }
}
