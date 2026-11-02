//! v2 game frames while a game is played: every 10 s, the feed's newest frames of each game
//! `inProgress` become new points of its series (`frames = 'live'`). The feed answers only for
//! windows that ended 50 s ago, so the points are 60-90 s behind the game. A game seen for the
//! first time, or after missed ticks, is caught up from its first frame or last point, a few
//! steps per tick. After the game the archive replaces all of it (`v2_frames`)

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use color_eyre::{Result, eyre::eyre};

use crate::{
    client::{
        adapters::v2::details::match_teams,
        feed::{
            NoStats,
            models::GameMetadata,
            walk::{Endpoint, FeedSource, HttpFeed, feed_time},
        },
        log,
    },
    dao::v2::frames::{event_teams, live_games, write_live},
    frames::{
        RawFrame, parse_frames,
        timeline::{Builder, Chunk, LiveState, STEP_S},
    },
    service::v2_frames::summarize,
};

pub const TICK: Duration = Duration::from_secs(10);
/// Steps of 10 s fetched per game and tick: catching up 2 min of feed per 10 s
const MAX_STEPS: usize = 12;
/// The newest window the feed serves: ended 50 s ago, a bit more for its clock
const LAG: TimeDelta = TimeDelta::seconds(70);
const STEP: TimeDelta = TimeDelta::seconds(10);
/// A game whose feed or write failed waits this long before its next try
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// What a tick got for one game, ready to write
#[derive(Debug)]
pub struct Got {
    pub blue: i64,
    pub red: i64,
    pub chunk: Chunk,
    pub state: LiveState,
}

/// Why a tick got nothing new
#[derive(Debug, PartialEq)]
pub enum Nothing {
    /// 204: no feed for this game (yet)
    NoFeed,
    /// The feed has nothing newer for now (400 too recent, 404 stats disabled, no new frames)
    NotYet,
}

fn no_stats(e: &color_eyre::Report) -> bool {
    e.downcast_ref::<NoStats>().is_some()
}

/// The newest frames of one game since `state`, up to `now - LAG`
pub async fn collect(
    src: &impl FeedSource,
    game: i64,
    state: Option<LiveState>,
    now: DateTime<Utc>,
) -> Result<Result<Got, Nothing>> {
    let mut teams: Option<(i64, i64)> = None;
    let mut window: Vec<RawFrame> = Vec::new();
    let mut details: Vec<RawFrame> = Vec::new();
    let last = state
        .as_ref()
        .and_then(|s| s.cursor().or_else(|| s.last_frame()));
    let mut cursor = match last {
        Some(t) => t,
        None => {
            // the first tick: from the game's first frame
            let body = match src.get(Endpoint::Window, game, None).await {
                Ok(Some(b)) => b,
                Ok(None) => return Ok(Err(Nothing::NoFeed)),
                Err(e) if no_stats(&e) => return Ok(Err(Nothing::NotYet)),
                Err(e) => return Err(e),
            };
            // only the start and the teams: its frames come again with their details below
            let (meta, frames) = parse_frames(&body)?;
            if let Some(m) = meta {
                teams = Some(GameMetadata::parse(&m)?.team_ext_ids()?);
            }
            let Some(first) = frames.first().map(|f| f.ts) else {
                return Ok(Err(Nothing::NotYet));
            };
            first
        }
    }
    .duration_trunc(STEP)
    .map_err(|e| eyre!("feed time: {e}"))?;
    let newest = now - LAG;
    let mut steps = 0;
    for _ in 0..MAX_STEPS {
        if cursor + STEP > newest {
            break;
        }
        let at = feed_time(cursor);
        let body = match src.get(Endpoint::Window, game, Some(&at)).await {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) if no_stats(&e) => break,
            Err(e) => return Err(e),
        };
        let (meta, frames) = parse_frames(&body)?;
        if teams.is_none()
            && let Some(m) = meta
        {
            teams = Some(GameMetadata::parse(&m)?.team_ext_ids()?);
        }
        let body = match src.get(Endpoint::Details, game, Some(&at)).await {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) if no_stats(&e) => break,
            Err(e) => return Err(e),
        };
        // a window without its details would build points without player stats: both or none
        window.extend(frames);
        details.extend(parse_frames(&body)?.1);
        cursor += STEP;
        steps += 1;
    }
    // no window old enough yet (tick jitter), or the feed refused it: nothing new, nothing saved
    if steps == 0 {
        return Ok(Err(Nothing::NotYet));
    }
    let mut builder = match state {
        Some(s) => Builder::resume(s)?,
        None => Builder::new(STEP_S),
    };
    let chunk = builder.push(&window, &details)?;
    builder.set_cursor(cursor);
    if builder.state().first_frame().is_none() {
        return Ok(Err(Nothing::NotYet));
    }
    let Some((blue, red)) = teams else {
        return Err(eyre!("game {game}: no gameMetadata in the feed answers"));
    };
    Ok(Ok(Got {
        blue,
        red,
        chunk,
        state: builder.state().clone(),
    }))
}

/// One tick: every live game. Errors are per game; a game that failed waits `RETRY_AFTER`
/// (one try and one log line a minute, not 24 calls every 10 s). Returns games written
pub async fn tick(
    src: &impl FeedSource,
    now: DateTime<Utc>,
    failed: &mut HashMap<i64, Instant>,
) -> Result<usize> {
    let games = live_games().await?;
    let ids: Vec<i64> = games.iter().map(|(g, _)| *g).collect();
    let teams = event_teams(&ids).await?;
    failed.retain(|g, _| ids.contains(g));
    let mut written = 0;
    for (game, state) in games {
        if failed.get(&game).is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            continue;
        }
        let event_teams = teams.get(&game).map_or(&[][..], Vec::as_slice);
        let result = match collect(src, game, state, now).await {
            Ok(Ok(got)) => {
                for w in summarize(&got.chunk.warnings) {
                    log(format!("v2 frames live: game {game}: {w}"));
                }
                // like the archive: a feed team the match doesn't list takes its other team
                match match_teams(game, [got.blue, got.red], event_teams) {
                    Ok(([blue, red], _)) if blue != red => {
                        write_live(game, blue, red, &got.chunk, &got.state)
                            .await
                            .map(|n| written += usize::from(n > 0))
                    }
                    Ok(([blue, _], _)) => Err(eyre!("game {game}: team {blue} on both sides")),
                    Err(e) => Err(e),
                }
            }
            Ok(Err(_)) => Ok(()),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                failed.remove(&game);
            }
            Err(e) => {
                failed.insert(game, Instant::now());
                log(format!(
                    "v2 frames live: game {game}: {e:#} (tried again in {} s)",
                    RETRY_AFTER.as_secs()
                ));
            }
        }
    }
    Ok(written)
}

/// Never returns. Each tick in its own task: a panic skips that tick, not the loop. A tick
/// that takes longer than 10 s skips the ticks it missed
pub async fn live_loop() {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut failed = HashMap::new();
    let mut last_log: Option<Instant> = None;
    loop {
        interval.tick().await;
        let mut map = std::mem::take(&mut failed);
        let run = tokio::spawn(async move {
            let r = tick(&HttpFeed, Utc::now(), &mut map).await;
            (r, map)
        });
        let error = match run.await {
            Ok((r, map)) => {
                failed = map;
                r.err().map(|e| format!("tick failed: {e:#}"))
            }
            Err(e) => Some(format!("tick stopped, skipped: {e}")),
        };
        if let Some(e) = error
            && last_log.is_none_or(|t| t.elapsed() >= RETRY_AFTER)
        {
            last_log = Some(Instant::now());
            log(format!("v2 frames live: {e}"));
        }
    }
}

/// Live points need both `TRIFORCE_V2_FRAMES` and `TRIFORCE_V2_FRAMES_LIVE`
pub fn live_on(frames: bool, live: bool) -> bool {
    frames && live
}

pub fn enabled() -> Result<bool> {
    crate::env::switch("TRIFORCE_V2_FRAMES_LIVE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::feed::walk::mock::{Fail, MockFeed},
        frames::{hand, timeline::build},
    };

    fn at(s: i64) -> DateTime<Utc> {
        hand::start() + TimeDelta::seconds(s)
    }

    fn feed(seconds: i64) -> MockFeed {
        let (w, d) = hand::game(seconds, true);
        MockFeed::game(hand::metadata(), w, d)
    }

    fn raws(v: &[serde_json::Value]) -> Vec<RawFrame> {
        v.iter().map(hand::raw).collect()
    }

    /// Ticks every 10 s while the game is played, then the archive's points: the same
    #[tokio::test]
    async fn ticks_give_the_archive_points() {
        let f = feed(300);
        let mut state = None;
        let mut chunk = Chunk::default();
        let mut tick_at = at(0);
        while tick_at < at(300) + TimeDelta::seconds(120) {
            f.set_now(tick_at);
            if let Ok(got) = collect(&f, 1, state.clone(), tick_at).await.unwrap() {
                assert_eq!((got.blue, got.red), (-402, -403));
                chunk.append(got.chunk);
                state = Some(got.state);
            }
            tick_at += TimeDelta::seconds(10);
        }
        let mut b = Builder::resume(state.unwrap()).unwrap();
        chunk.append(b.finish().unwrap());
        let (w, d) = hand::game(300, true);
        let whole = build(&raws(&w), &raws(&d), STEP_S).unwrap();
        assert_eq!(chunk, whole.chunk);
    }

    /// Missed ticks (network, a restart) are caught up from the last point, no holes
    #[tokio::test]
    async fn missed_ticks_are_caught_up() {
        let f = feed(300);
        f.set_now(at(100));
        let got = collect(&f, 1, None, at(100)).await.unwrap().unwrap();
        let first_points = got.chunk.points();
        assert!(first_points > 0);
        // nothing for 2 minutes, then one tick: 12 steps at most
        f.set_now(at(250));
        let before = f.calls();
        let got2 = collect(&f, 1, Some(got.state.clone()), at(250))
            .await
            .unwrap()
            .unwrap();
        assert!(f.calls() - before <= 2 * 12);
        let mut chunk = got.chunk;
        chunk.append(got2.chunk);
        assert!(chunk.teams[0].gold.iter().all(Option::is_some));
        // the points go on from where they were: 10 s apart in game time
        assert_eq!(chunk.points() as i64, got2.state.points());
    }

    /// A game first seen in the middle starts at its first frame, not now
    #[tokio::test]
    async fn first_tick_starts_at_the_game_start() {
        let f = feed(600);
        f.set_now(at(400));
        let got = collect(&f, 1, None, at(400)).await.unwrap().unwrap();
        assert_eq!(got.state.first_frame(), Some(at(0)));
        assert_eq!(got.chunk.teams[0].gold[0], Some(2500));
    }

    /// 10 min without a single frame (a long pause): live goes on after it
    #[tokio::test]
    async fn minutes_without_frames() {
        let (w, d) = hand::game(900, true);
        let keep = |f: &serde_json::Value| {
            let t = f["rfc460Timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap();
            t < at(100) || t > at(700)
        };
        let f = MockFeed::game(
            hand::metadata(),
            w.into_iter().filter(keep).collect(),
            d.into_iter().filter(keep).collect(),
        );
        let mut state = None;
        let mut now = at(0);
        let mut last_ts = None;
        while now < at(1100) {
            f.set_now(now);
            if let Ok(got) = collect(&f, 1, state.clone(), now).await.unwrap() {
                last_ts = got.state.last_frame();
                state = Some(got.state);
            }
            now += TimeDelta::seconds(10);
        }
        assert_eq!(last_ts, Some(at(900)));
    }

    /// A tick with no window old enough (tick jitter) is quiet, nothing to save
    #[tokio::test]
    async fn no_step_this_tick_is_not_yet() {
        let f = feed(300);
        f.set_now(at(100));
        let got = collect(&f, 1, None, at(100)).await.unwrap().unwrap();
        let calls = f.calls();
        let again = collect(&f, 1, Some(got.state), at(100)).await.unwrap();
        assert_eq!(again.unwrap_err(), Nothing::NotYet);
        assert_eq!(f.calls(), calls);
    }

    #[tokio::test]
    async fn nothing_yet_or_no_feed() {
        // the game just started: the feed has no window old enough
        let f = feed(300);
        f.set_now(at(5));
        let got = collect(&f, 1, None, at(5)).await.unwrap().unwrap_err();
        assert_eq!(got, Nothing::NotYet);
        assert_eq!(f.calls(), 1);
        // 204
        assert_eq!(
            collect(&MockFeed::default(), 1, None, at(100))
                .await
                .unwrap()
                .unwrap_err(),
            Nothing::NoFeed
        );
        // 404 stats disabled on the first call
        let f = feed(300).fail_on(1, Fail::NoStats);
        f.set_now(at(100));
        assert_eq!(
            collect(&f, 1, None, at(100)).await.unwrap().unwrap_err(),
            Nothing::NotYet
        );
        // any other failure is an error for the tick
        let f = feed(300).fail_on(2, Fail::Other);
        f.set_now(at(100));
        assert!(collect(&f, 1, None, at(100)).await.is_err());
    }

    #[test]
    fn switches() {
        assert!(live_on(true, true));
        assert!(!live_on(false, true));
        assert!(!live_on(true, false));
    }

    /// Replay: the real fixture game served as if live, written tick by tick (one tick
    /// missed), then the game ends and the after-game job writes the archive
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn replay_a_real_game() {
        use canyon_sql::connection::DbConnection;

        use crate::{
            client::feed::walk::{WalkConfig, walk_game},
            dao::v2::frames::{
                tests::{count, one, setup},
                write_final,
            },
            frames::pack::pack,
        };
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        let frames = |s: &str| -> Vec<serde_json::Value> {
            serde_json::from_str::<serde_json::Value>(s).unwrap()["frames"]
                .as_array()
                .unwrap()
                .clone()
        };
        let w = frames(fixture!("feed/frames/window.json"));
        let d = frames(fixture!("feed/frames/details.json"));
        let ts = |f: &serde_json::Value| {
            f["rfc460Timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap()
        };
        let (start, end) = (ts(&w[0]), ts(w.last().unwrap()));
        let f = MockFeed::game(hand::metadata(), w.clone(), d.clone());
        let mut logged = HashMap::new();
        let mut now = start;
        let mut ticks = 0;
        while now < end + TimeDelta::seconds(120) {
            f.set_now(now);
            ticks += 1;
            // one tick lost to the network
            if ticks != 7 {
                tick(&f, now, &mut logged).await.unwrap();
            }
            now += TimeDelta::seconds(10);
        }
        assert!(logged.is_empty(), "{logged:?}");
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -423").await,
            "live"
        );
        assert_eq!(one("SELECT source FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, "live");
        let live_points = count("SELECT points FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await;
        assert!(live_points > 100, "{live_points}");
        assert!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -423").await >= 3);
        // the game ends: the after-game walk (the feed serves everything now) and its write
        "".execute(
            "UPDATE v2_game SET state = 'completed' WHERE ext_id = -423",
            &[],
        )
        .await
        .unwrap();
        *f.now.lock().unwrap() = None;
        let cfg = WalkConfig {
            pace: Duration::ZERO,
            backoff: [Duration::ZERO; 3],
            ..WalkConfig::default()
        };
        let walked = walk_game(&f, -423, Some(end.naive_utc()), &cfg)
            .await
            .unwrap();
        let tl = build(&walked.window, &walked.details, STEP_S).unwrap();
        let pw = pack(Some(&walked.metadata), &walked.window).unwrap();
        let pd = pack(None, &walked.details).unwrap();
        write_final(
            -423,
            &pw,
            &pd,
            walked.blue_team_ext_id,
            walked.red_team_ext_id,
            &tl,
        )
        .await
        .unwrap();
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -423").await,
            "done"
        );
        assert_eq!(count("SELECT points FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, tl.points);
        // the live points were the archive's, up to the last live one
        assert!(tl.points >= live_points);
        assert_eq!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -423").await, tl.chunk.markers.len() as i64);
        crate::dao::v2::test_cleanup().await;
    }
}
