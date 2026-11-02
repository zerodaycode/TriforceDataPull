//! Every frame of a finished game: `window` without a time for the metadata and the first
//! frames, then `window` and `details` 10 s at a time until the `finished` frame

use std::{collections::BTreeMap, future::Future, time::Duration};

use chrono::{DateTime, DurationRound, NaiveDateTime, TimeDelta, Utc};
use color_eyre::{
    Report, Result,
    eyre::{WrapErr, eyre},
};
use serde_json::value::RawValue;

use super::{Transient, models::GameMetadata};
use crate::frames::{RawFrame, parse_frames};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endpoint {
    Window,
    Details,
}

impl Endpoint {
    pub fn path(self) -> &'static str {
        match self {
            Endpoint::Window => "window",
            Endpoint::Details => "details",
        }
    }
}

/// Where the answers come from: the feed, or a test's
pub trait FeedSource: Sync {
    /// The body, `None` on 204. `Transient` errors are worth another try
    fn get(
        &self,
        endpoint: Endpoint,
        game: i64,
        at: Option<&str>,
    ) -> impl Future<Output = Result<Option<String>>> + Send;
}

pub struct HttpFeed;

impl FeedSource for HttpFeed {
    async fn get(&self, endpoint: Endpoint, game: i64, at: Option<&str>) -> Result<Option<String>> {
        let path = match at {
            None => format!("{}/{game}", endpoint.path()),
            Some(t) => format!("{}/{game}?startingTime={t}", endpoint.path()),
        };
        Ok(super::get_raw(&path).await?.map(|(body, _)| body))
    }
}

#[derive(Debug, Clone)]
pub struct WalkConfig {
    pub step: TimeDelta,
    /// Between two calls
    pub pace: Duration,
    /// Waits before the 2nd, 3rd and 4th try of a call that failed with a `Transient` error
    pub backoff: [Duration; 3],
    /// Calls in a row without a newer `window` frame before the walk stops (games without a
    /// `finished` frame)
    pub stall_calls: u32,
    /// Feed time from the first frame after which the walk gives up. The longest game seen was
    /// 2:58 of wall time with its pauses (DK-DRX 2024-02-25 game 2, a DDoS)
    pub guard: TimeDelta,
    /// Feed time without a new frame after which the walk gives up (a game stuck `paused`),
    /// while the walk hasn't reached the game's known end. The longest seen without frames was
    /// 80 min (DK-DRX 2024-02-25 game 2)
    pub max_gap: TimeDelta,
}

impl Default for WalkConfig {
    fn default() -> Self {
        Self {
            step: TimeDelta::seconds(10),
            pace: Duration::from_millis(100),
            backoff: [
                Duration::from_secs(1),
                Duration::from_secs(4),
                Duration::from_secs(16),
            ],
            stall_calls: 3,
            guard: TimeDelta::hours(6),
            max_gap: TimeDelta::minutes(30),
        }
    }
}

#[derive(Debug)]
pub struct Walked {
    pub metadata: Box<RawValue>,
    pub blue_team_ext_id: i64,
    pub red_team_ext_id: i64,
    pub window: Vec<RawFrame>,
    pub details: Vec<RawFrame>,
    pub calls: u32,
}

#[derive(Debug)]
pub enum WalkError {
    /// 204: this game has no feed
    NoFeed,
    /// Metadata without 5 players a side
    ShortSides {
        blue: usize,
        red: usize,
    },
    Failed(Report),
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalkError::NoFeed => f.write_str("no feed (204)"),
            WalkError::ShortSides { blue, red } => {
                write!(f, "feed has {blue} blue and {red} red players")
            }
            WalkError::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl From<Report> for WalkError {
    fn from(e: Report) -> Self {
        WalkError::Failed(e)
    }
}

/// `window` and `details` must start and end within this of each other
const ENDS_MATCH: TimeDelta = TimeDelta::seconds(10);
/// A walk goes on until this close to the game's known end
const END_SLACK: TimeDelta = TimeDelta::seconds(10);

/// The game a `window` answer is about
fn esports_game_id(body: &str) -> Result<i64> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Head {
        esports_game_id: String,
    }
    let h: Head = serde_json::from_str(body).wrap_err("window answer")?;
    h.esports_game_id
        .parse()
        .wrap_err_with(|| format!("esportsGameId {:?}", h.esports_game_id))
}

pub fn feed_time(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

struct Caller<'a, S> {
    src: &'a S,
    game: i64,
    cfg: &'a WalkConfig,
    calls: u32,
}

impl<S: FeedSource> Caller<'_, S> {
    /// One call, `Transient` failures tried again after each backoff
    async fn call(&mut self, endpoint: Endpoint, at: Option<&str>) -> Result<Option<String>> {
        let mut waits = self.cfg.backoff.iter();
        loop {
            self.calls += 1;
            match self.src.get(endpoint, self.game, at).await {
                Ok(body) => return Ok(body),
                Err(e) if e.downcast_ref::<Transient>().is_some() => match waits.next() {
                    Some(wait) => tokio::time::sleep(*wait).await,
                    None => {
                        return Err(e.wrap_err(format!(
                            "{} at {}: still failing after {} tries",
                            endpoint.path(),
                            at.unwrap_or("start"),
                            self.cfg.backoff.len() + 1
                        )));
                    }
                },
                Err(e) => {
                    return Err(e.wrap_err(format!(
                        "{} at {}",
                        endpoint.path(),
                        at.unwrap_or("start")
                    )));
                }
            }
        }
    }
}

pub async fn walk_game(
    src: &impl FeedSource,
    game: i64,
    known_end: Option<NaiveDateTime>,
    cfg: &WalkConfig,
) -> Result<Walked, WalkError> {
    let mut caller = Caller {
        src,
        game,
        cfg,
        calls: 0,
    };
    let Some(first) = caller.call(Endpoint::Window, None).await? else {
        return Err(WalkError::NoFeed);
    };
    let answered = esports_game_id(&first)?;
    if answered != game {
        return Err(eyre!("feed answered game {answered} for game {game}").into());
    }
    let (metadata, first_frames) = parse_frames(&first)?;
    let metadata = metadata.ok_or_else(|| eyre!("first window without gameMetadata"))?;
    let meta = GameMetadata::parse(&metadata).wrap_err("first window")?;
    let blue = meta.blue_team_metadata.participant_metadata.len();
    let red = meta.red_team_metadata.participant_metadata.len();
    if blue != 5 || red != 5 {
        return Err(WalkError::ShortSides { blue, red });
    }
    let (blue_team_ext_id, red_team_ext_id) = meta.team_ext_ids()?;

    let mut window: BTreeMap<DateTime<Utc>, RawFrame> = BTreeMap::new();
    let mut details: BTreeMap<DateTime<Utc>, RawFrame> = BTreeMap::new();
    // the feed can send two frames with the same time (in game, then paused at 1 ms from the
    // play): the last one it sent is kept
    for f in first_frames {
        window.insert(f.ts, f);
    }
    let first_ts = *window
        .keys()
        .next()
        .ok_or_else(|| eyre!("first window without frames"))?;
    let mut at = first_ts
        .duration_trunc(cfg.step)
        .map_err(|e| eyre!("start time: {e}"))?;
    let mut stall = 0;
    loop {
        if let Some(last) = window.values().next_back()
            && at - last.ts > cfg.max_gap
            && !known_end.is_some_and(|end| at.naive_utc() <= end + END_SLACK)
        {
            return Err(eyre!(
                "no frame for {} min after {}, walk stopped",
                cfg.max_gap.num_minutes(),
                last.ts
            )
            .into());
        }
        if at - first_ts > cfg.guard {
            return Err(eyre!(
                "no end after {} min of frames, walk stopped",
                cfg.guard.num_minutes()
            )
            .into());
        }
        let t = feed_time(at);
        let newest = window.keys().next_back().copied();
        tokio::time::sleep(cfg.pace).await;
        // a 204 after the feed answered would leave a hole in the archive
        let body = caller
            .call(Endpoint::Window, Some(&t))
            .await?
            .ok_or_else(|| eyre!("window at {t}: 204 in the middle of the game"))?;
        for f in parse_frames(&body)?.1 {
            window.insert(f.ts, f);
        }
        tokio::time::sleep(cfg.pace).await;
        let body = caller
            .call(Endpoint::Details, Some(&t))
            .await?
            .ok_or_else(|| eyre!("details at {t}: 204 in the middle of the game"))?;
        for f in parse_frames(&body)?.1 {
            details.insert(f.ts, f);
        }
        if window
            .values()
            .next_back()
            .is_some_and(|f| f.state.as_deref() == Some("finished"))
        {
            break;
        }
        if window.keys().next_back().copied() == newest {
            stall += 1;
        } else {
            stall = 0;
        }
        // nothing new for a while is the end, unless the game is paused (the feed can go
        // minutes without a frame then) or hasn't reached its known end yet
        let last = window.values().next_back();
        let paused = last.is_some_and(|f| f.state.as_deref() == Some("paused"));
        let before_end = known_end
            .zip(last)
            .is_some_and(|(end, f)| f.ts.naive_utc() < end - END_SLACK);
        if stall >= cfg.stall_calls && !paused && !before_end {
            break;
        }
        at += cfg.step;
    }
    let window: Vec<RawFrame> = window.into_values().collect();
    let details: Vec<RawFrame> = details.into_values().collect();
    let (Some(w_first), Some(w_last)) = (window.first(), window.last()) else {
        return Err(eyre!("no window frames").into());
    };
    let (Some(d_first), Some(d_last)) = (details.first(), details.last()) else {
        return Err(eyre!("no details frames").into());
    };
    for (what, w, d) in [
        ("start", w_first.ts, d_first.ts),
        ("end", w_last.ts, d_last.ts),
    ] {
        let gap = (w - d).abs();
        if gap > ENDS_MATCH {
            return Err(eyre!(
                "window and details {what} {} ms apart",
                gap.num_milliseconds()
            )
            .into());
        }
    }
    Ok(Walked {
        metadata,
        blue_team_ext_id,
        red_team_ext_id,
        window,
        details,
        calls: caller.calls,
    })
}

#[cfg(test)]
pub(crate) mod mock {
    //! A feed for tests: serves a game's frames like the real one (`at` → the frames from
    //! `at` to `at + 10 s`, no time → metadata + the first 10 s), plus failures on given calls
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use chrono::{DateTime, TimeDelta, Utc};
    use color_eyre::{Result, eyre::eyre};
    use serde_json::{Value, json};

    use super::{Endpoint, FeedSource};
    use crate::client::feed::{NoStats, Transient};

    #[derive(Debug, Clone)]
    pub enum Fail {
        Transient,
        NoStats,
        Other,
        NoContent,
    }

    pub type CallLog = Vec<(Endpoint, Option<String>)>;

    #[derive(Default)]
    pub struct MockFeed {
        pub metadata: Option<Value>,
        pub window: Vec<Value>,
        pub details: Vec<Value>,
        /// Failures by call number (1 = first call)
        pub fail: Mutex<HashMap<u32, Fail>>,
        /// Every call: (endpoint, at)
        pub log: Arc<Mutex<CallLog>>,
        /// Frames after this time aren't served yet (a live game)
        pub now: Mutex<Option<DateTime<Utc>>>,
        /// Answer as this game instead of the one asked
        pub answer_as: Option<i64>,
    }

    fn ts(f: &Value) -> DateTime<Utc> {
        f["rfc460Timestamp"].as_str().unwrap().parse().unwrap()
    }

    impl MockFeed {
        pub fn game(metadata: Value, window: Vec<Value>, details: Vec<Value>) -> Self {
            Self {
                metadata: Some(metadata),
                window,
                details,
                ..Default::default()
            }
        }

        pub fn fail_on(self, call: u32, how: Fail) -> Self {
            self.fail.lock().unwrap().insert(call, how);
            self
        }

        pub fn calls(&self) -> usize {
            self.log.lock().unwrap().len()
        }

        pub fn set_now(&self, now: DateTime<Utc>) {
            *self.now.lock().unwrap() = Some(now);
        }

        fn slice(&self, frames: &[Value], from: Option<DateTime<Utc>>) -> Vec<Value> {
            let now = *self.now.lock().unwrap();
            let start = from.or_else(|| frames.first().map(ts));
            let Some(start) = start else { return vec![] };
            frames
                .iter()
                .filter(|f| {
                    let t = ts(f);
                    t >= start && t < start + TimeDelta::seconds(10) && now.is_none_or(|n| t <= n)
                })
                .cloned()
                .collect()
        }
    }

    impl FeedSource for MockFeed {
        async fn get(
            &self,
            endpoint: Endpoint,
            game: i64,
            at: Option<&str>,
        ) -> Result<Option<String>> {
            let n = {
                let mut log = self.log.lock().unwrap();
                log.push((endpoint, at.map(str::to_string)));
                log.len() as u32
            };
            if let Some(f) = self.fail.lock().unwrap().get(&n) {
                return match f {
                    Fail::Transient => Err(Transient::new("mock 503").into()),
                    Fail::NoStats => Err(NoStats {
                        path: "mock".into(),
                        status: reqwest::StatusCode::NOT_FOUND,
                    }
                    .into()),
                    Fail::Other => Err(eyre!("mock: HTTP 403")),
                    Fail::NoContent => Ok(None),
                };
            }
            if self.metadata.is_none() {
                return Ok(None);
            }
            let from = at.map(|t| t.parse::<DateTime<Utc>>().unwrap());
            // like the feed: a window must have ended 50 s ago
            if let (Some(from), Some(now)) = (from, *self.now.lock().unwrap())
                && from + TimeDelta::seconds(10) > now - TimeDelta::seconds(50)
            {
                return Err(NoStats {
                    path: "mock".into(),
                    status: reqwest::StatusCode::BAD_REQUEST,
                }
                .into());
            }
            let body = match endpoint {
                Endpoint::Window => json!({
                    "esportsGameId": self.answer_as.unwrap_or(game).to_string(),
                    "gameMetadata": self.metadata,
                    "frames": self.slice(&self.window, from),
                }),
                Endpoint::Details => json!({"frames": self.slice(&self.details, from)}),
            };
            Ok(Some(body.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        mock::{Fail, MockFeed},
        *,
    };
    use crate::frames::hand;

    fn fast() -> WalkConfig {
        WalkConfig {
            pace: Duration::ZERO,
            backoff: [Duration::ZERO; 3],
            ..WalkConfig::default()
        }
    }

    fn feed(seconds: i64, finished: bool) -> MockFeed {
        let (w, d) = hand::game(seconds, finished);
        MockFeed::game(hand::metadata(), w, d)
    }

    #[tokio::test]
    async fn walks_to_the_finished_frame() {
        let f = feed(95, true);
        let w = walk_game(&f, 1, None, &fast()).await.unwrap();
        assert_eq!(w.window.len(), 96);
        assert_eq!(w.details.len(), 96);
        assert_eq!(w.window.last().unwrap().state.as_deref(), Some("finished"));
        assert_eq!((w.blue_team_ext_id, w.red_team_ext_id), (-402, -403));
        // first call + 11 steps × 2: 23:05:07 floored to :00, the end (23:06:42) in :40
        assert_eq!(w.calls, 1 + 11 * 2);
        assert_eq!(w.calls as usize, f.calls());
        // sorted, no duplicates even though the first call and the first step overlap
        assert!(w.window.windows(2).all(|p| p[0].ts < p[1].ts));
    }

    #[tokio::test]
    async fn no_finished_frame_ends_after_three_empty_steps() {
        let f = feed(95, false);
        let w = walk_game(&f, 1, None, &fast()).await.unwrap();
        assert_eq!(w.window.len(), 96);
        assert_eq!(w.calls, 1 + (11 + 3) * 2);
    }

    #[tokio::test]
    async fn the_known_end_keeps_the_walk_going() {
        // frames until 95 s, the details job saw the game end at 200 s: the walk goes on past
        // the frames (no half game kept), then `max_gap` after the end stops it, not the guard
        let f = feed(95, false);
        let end = (hand::start() + TimeDelta::seconds(200)).naive_utc();
        let cfg = WalkConfig {
            max_gap: TimeDelta::minutes(2),
            ..fast()
        };
        let e = walk_game(&f, 1, Some(end), &cfg).await.unwrap_err();
        assert!(e.to_string().contains("no frame for 2 min"), "{e}");
        assert!(f.calls() < 60, "{}", f.calls());
        // the right end is fine
        let end = (hand::start() + TimeDelta::seconds(95)).naive_utc();
        assert!(walk_game(&f, 1, Some(end), &fast()).await.is_ok());
    }

    /// A long pause: minutes without a frame, then the game goes on
    #[tokio::test]
    async fn walks_through_a_pause_without_frames() {
        let (mut w, d) = hand::game(900, true);
        for f in &mut w[100..=700] {
            f["gameState"] = serde_json::json!("paused");
        }
        let start = hand::start();
        let keep = |f: &serde_json::Value| {
            let t = f["rfc460Timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap();
            t <= start + TimeDelta::seconds(100) || t > start + TimeDelta::seconds(700)
        };
        let f = MockFeed::game(
            hand::metadata(),
            w.into_iter().filter(keep).collect(),
            d.into_iter().filter(keep).collect(),
        );
        let walked = walk_game(&f, 1, None, &fast()).await.unwrap();
        assert_eq!(
            walked.window.last().unwrap().state.as_deref(),
            Some("finished")
        );
        assert_eq!(walked.window.len(), 101 + 200);
    }

    /// Two frames with the same time, in game then paused, and minutes without frames: the
    /// walk sees the pause and goes on (FUR-GAM 2025-06-28 game 5 stopped at the pause before)
    #[tokio::test]
    async fn same_time_in_game_then_paused_walks_on() {
        let (mut w, d) = hand::game(900, true);
        let mut dup = w[100].clone();
        dup["gameState"] = serde_json::json!("paused");
        let start = hand::start();
        let keep = |f: &serde_json::Value| {
            let t = f["rfc460Timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap();
            t <= start + TimeDelta::seconds(100) || t > start + TimeDelta::seconds(700)
        };
        w.insert(101, dup);
        let f = MockFeed::game(
            hand::metadata(),
            w.into_iter().filter(keep).collect(),
            d.into_iter().filter(keep).collect(),
        );
        let walked = walk_game(&f, 1, None, &fast()).await.unwrap();
        assert_eq!(
            walked.window.last().unwrap().state.as_deref(),
            Some("finished")
        );
    }

    #[tokio::test]
    async fn a_game_stuck_paused_stops_after_max_gap() {
        let (mut w, d) = hand::game(60, false);
        for f in &mut w[30..] {
            f["gameState"] = serde_json::json!("paused");
        }
        let f = MockFeed::game(hand::metadata(), w, d);
        let cfg = WalkConfig {
            max_gap: TimeDelta::minutes(2),
            ..fast()
        };
        let e = walk_game(&f, 1, None, &cfg).await.unwrap_err();
        assert!(e.to_string().contains("no frame for 2 min"), "{e}");
        // about 2 min of 10 s steps, not the 6 h guard
        assert!(f.calls() < 40, "{}", f.calls());
    }

    /// A pause longer than `max_gap` without frames (DK-DRX 2024-02-25 game 2 went 80 min): with
    /// the end known and still ahead, the walk waits it out
    #[tokio::test]
    async fn a_known_end_outlasts_max_gap() {
        let (mut w, d) = hand::game(900, true);
        for f in &mut w[100..=700] {
            f["gameState"] = serde_json::json!("paused");
        }
        let start = hand::start();
        let keep = |f: &serde_json::Value| {
            let t = f["rfc460Timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap();
            t <= start + TimeDelta::seconds(100) || t > start + TimeDelta::seconds(700)
        };
        let f = MockFeed::game(
            hand::metadata(),
            w.into_iter().filter(keep).collect(),
            d.into_iter().filter(keep).collect(),
        );
        let cfg = WalkConfig {
            max_gap: TimeDelta::minutes(2),
            ..fast()
        };
        let e = walk_game(&f, 1, None, &cfg).await.unwrap_err();
        assert!(e.to_string().contains("no frame for 2 min"), "{e}");
        let end = (start + TimeDelta::seconds(900)).naive_utc();
        let walked = walk_game(&f, 1, Some(end), &cfg).await.unwrap();
        assert_eq!(walked.window.len(), 101 + 200);
    }

    #[tokio::test]
    async fn an_answer_for_another_game_fails() {
        let mut f = feed(30, true);
        f.answer_as = Some(9);
        let e = walk_game(&f, 2, None, &fast()).await.unwrap_err();
        assert!(
            e.to_string().contains("feed answered game 9 for game 2"),
            "{e}"
        );
        assert!(esports_game_id(r#"{"esportsGameId":"x"}"#).is_err());
    }

    #[tokio::test]
    async fn guard_stops_an_endless_game() {
        let f = feed(120, false);
        let cfg = WalkConfig {
            guard: TimeDelta::seconds(60),
            ..fast()
        };
        let e = walk_game(&f, 1, None, &cfg).await.unwrap_err();
        assert!(e.to_string().contains("no end after"), "{e}");
    }

    #[tokio::test]
    async fn no_feed_and_short_sides() {
        let f = MockFeed::default();
        assert!(matches!(
            walk_game(&f, 1, None, &fast()).await,
            Err(WalkError::NoFeed)
        ));
        let mut meta = hand::metadata();
        meta["redTeamMetadata"]["participantMetadata"]
            .as_array_mut()
            .unwrap()
            .truncate(4);
        let (w, d) = hand::game(30, true);
        let f = MockFeed::game(meta, w, d);
        assert!(matches!(
            walk_game(&f, 1, None, &fast()).await,
            Err(WalkError::ShortSides { blue: 5, red: 4 })
        ));
        let mut meta = hand::metadata();
        meta["blueTeamMetadata"]["esportsTeamId"] = serde_json::json!("abc");
        let (w, d) = hand::game(30, true);
        assert!(
            walk_game(&MockFeed::game(meta, w, d), 1, None, &fast())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn transient_failures_are_tried_again() {
        // 3 failures in a row on call 2, then it answers
        let f = feed(25, true)
            .fail_on(2, Fail::Transient)
            .fail_on(3, Fail::Transient)
            .fail_on(4, Fail::Transient);
        let w = walk_game(&f, 1, None, &fast()).await.unwrap();
        assert_eq!(w.window.len(), 26);
        // 4 failures in a row: the try fails
        let f = feed(25, true)
            .fail_on(2, Fail::Transient)
            .fail_on(3, Fail::Transient)
            .fail_on(4, Fail::Transient)
            .fail_on(5, Fail::Transient);
        let e = walk_game(&f, 1, None, &fast()).await.unwrap_err();
        assert!(e.to_string().contains("after 4 tries"), "{e}");
        assert_eq!(f.calls(), 5);
    }

    #[tokio::test]
    async fn other_failures_are_not_tried_again() {
        for how in [Fail::NoStats, Fail::Other] {
            let f = feed(25, true).fail_on(3, how);
            let e = walk_game(&f, 1, None, &fast()).await.unwrap_err();
            assert!(matches!(e, WalkError::Failed(_)));
            assert_eq!(f.calls(), 3);
        }
    }

    #[tokio::test]
    async fn details_failing_or_empty_fails_the_walk() {
        // call 3 = the first details call
        let f = feed(25, true).fail_on(3, Fail::Other);
        assert!(walk_game(&f, 1, None, &fast()).await.is_err());
        let (w, _) = hand::game(25, true);
        let f = MockFeed::game(hand::metadata(), w, vec![]);
        let e = walk_game(&f, 1, None, &fast()).await.unwrap_err();
        assert!(e.to_string().contains("no details frames"), "{e}");
    }

    #[tokio::test]
    async fn window_and_details_must_line_up() {
        let (w, mut d) = hand::game(40, true);
        d.truncate(30);
        let f = MockFeed::game(hand::metadata(), w, d);
        let e = walk_game(&f, 1, None, &fast()).await.unwrap_err();
        assert!(e.to_string().contains("window and details end"), "{e}");
        // 1 frame (1 s) short is fine
        let (w, mut d) = hand::game(40, true);
        d.pop();
        let f = MockFeed::game(hand::metadata(), w, d);
        assert!(walk_game(&f, 1, None, &fast()).await.is_ok());
    }

    #[tokio::test]
    async fn a_204_in_the_middle_fails() {
        for call in [4, 5] {
            let f = feed(25, false).fail_on(call, Fail::NoContent);
            let e = walk_game(&f, 1, None, &fast()).await.unwrap_err();
            assert!(e.to_string().contains("204 in the middle"), "{e}");
        }
    }

    #[test]
    fn feed_time_format() {
        assert_eq!(feed_time(hand::start()), "2026-10-31T23:05:07.000Z");
    }
}
