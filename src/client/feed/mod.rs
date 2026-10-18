//! Live feed `feed.lolesports.com/livestats/v1`, no key. It also answers for finished games:
//! no time = the first frames, a time after the end = the last ones

pub mod models;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use serde::de::DeserializeOwned;

use self::models::{Details, Window};
use crate::client::{http, read_text_max};

const FEED_URL: &str = "https://feed.lolesports.com/livestats/v1";
/// A normal answer is 17-70 KB
const MAX_BODY: usize = 4 * 1024 * 1024;

/// CloudFront sends `Date` = when the answer was made, so now is `Date + Age`
fn server_now(date: Option<&str>, age: Option<&str>, received: DateTime<Utc>) -> DateTime<Utc> {
    let Some(date) = date.and_then(|d| DateTime::parse_from_rfc2822(d).ok()) else {
        return received;
    };
    let age = age
        .and_then(|a| a.trim().parse::<u32>().ok())
        .and_then(|a| TimeDelta::try_seconds(a.into()))
        .unwrap_or_default();
    date.with_timezone(&Utc)
        .checked_add_signed(age)
        .unwrap_or(received)
}

/// A window must have ended 20 s ago: a minute back, on a 10 s boundary
pub fn starting_time(server_now: DateTime<Utc>) -> String {
    let t = server_now - TimeDelta::seconds(60);
    let t = t.duration_trunc(TimeDelta::seconds(10)).unwrap_or(t);
    t.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

/// `None` on 204 (no feed for this game). Also returns the server's now
async fn get<T: DeserializeOwned>(path: &str) -> Result<Option<(T, DateTime<Utc>)>> {
    let response = http()
        .get(format!("{FEED_URL}/{path}"))
        .send()
        .await
        .map_err(|e| e.without_url())
        .wrap_err_with(|| format!("feed {path}"))?;
    let received = Utc::now();
    let status = response.status();
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(eyre!("feed {path}: HTTP {status}"));
    }
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let now = server_now(
        header(reqwest::header::DATE).as_deref(),
        header(reqwest::header::AGE).as_deref(),
        received,
    );
    let body = read_text_max(response, MAX_BODY)
        .await
        .wrap_err_with(|| format!("feed {path}"))?;
    let parsed = serde_json::from_str(&body).wrap_err_with(|| format!("feed {path}"))?;
    Ok(Some((parsed, now)))
}

/// Metadata + frames. `at` = `None`: the first frames, a time after the end: the last ones
pub async fn window(game: i64, at: Option<&str>) -> Result<Option<(Window, DateTime<Utc>)>> {
    let path = match at {
        None => format!("window/{game}"),
        Some(t) => format!("window/{game}?startingTime={t}"),
    };
    get(&path).await
}

/// Per player frames from `at`
pub async fn details(game: i64, at: &str) -> Result<Option<Details>> {
    Ok(get(&format!("details/{game}?startingTime={at}"))
        .await?
        .map(|(d, _)| d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fixtures_parse() {
        let w: Window = serde_json::from_str(fixture!("feed/window_first.json")).unwrap();
        let meta = w.game_metadata.unwrap();
        assert_eq!(
            meta.blue_team_metadata.esports_team_id,
            "100205576309502431"
        );
        assert_eq!(meta.red_team_metadata.participant_metadata.len(), 5);
        let w: Window = serde_json::from_str(fixture!("feed/window_last.json")).unwrap();
        let last = w.frames.last().unwrap();
        assert_eq!(last.blue_team.towers, Some(10));
        let d: Details = serde_json::from_str(fixture!("feed/details_last.json")).unwrap();
        assert_eq!(d.frames.last().unwrap().participants.len(), 10);
    }

    #[test]
    fn starting_time_is_a_minute_back_on_10_s() {
        let now =
            Utc.with_ymd_and_hms(2026, 10, 16, 23, 22, 47).unwrap() + TimeDelta::milliseconds(500);
        assert_eq!(starting_time(now), "2026-10-16T23:21:40.000Z");
    }

    #[test]
    fn server_now_is_date_plus_age() {
        let received = Utc.with_ymd_and_hms(2026, 10, 18, 13, 40, 0).unwrap();
        assert_eq!(
            server_now(Some("Sun, 18 Oct 2026 13:39:46 GMT"), Some("10"), received),
            Utc.with_ymd_and_hms(2026, 10, 18, 13, 39, 56).unwrap()
        );
        assert_eq!(server_now(None, Some("10"), received), received);
    }
}
