//! v2 game frames after the game: every frame of each finished game from the live feed, kept
//! compressed, and the series built from them. The queue is in the DB (`v2_game.frames`):
//! stop and start any time. A failed game waits (10 min, 40 min, 2 h 40, 10 h 40), after 5
//! tries it's `no_feed`. `rebuild-timeline` builds the series again from the archive

use std::{collections::BTreeMap, time::Duration};

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use tokio::time::{Instant, sleep};

use crate::{
    client::{
        adapters::v2::details::match_teams,
        feed::{
            Transient,
            models::GameMetadata,
            walk::{FeedSource, HttpFeed, WalkConfig, WalkError, walk_game},
        },
        log,
    },
    dao::v2::frames::{
        FramesGame, MAX_TRIES, archived_games, bump_tries, event_teams, mark_no_feed, next_games,
        read_archive, write_final, write_rebuild,
    },
    env::Rebuild,
    frames::{
        pack::{pack, unpack},
        timeline::{STEP_S, Timeline, build},
    },
};

const BATCH: i64 = 20;
const IDLE: Duration = Duration::from_secs(5 * 60);
const LOG_EVERY: usize = 20;
const FAILS_BEFORE_PAUSE: usize = 5;
/// Between two games, on top of the walk's own pace
const PACE: Duration = Duration::from_secs(1);

/// The feed's blue and red teams as the match's: a team the match doesn't list takes the
/// match's other team, like the game details (logged). Same rule for the archive, live and
/// rebuild, so the series and markers are on the teams of `v2_game_team`
pub(crate) fn frames_teams(game: i64, feed: (i64, i64), event_teams: &[i64]) -> Result<(i64, i64)> {
    let ([blue, red], remapped) = match_teams(game, [feed.0, feed.1], event_teams)?;
    if blue == red {
        return Err(eyre!("game {game}: team {blue} on both sides"));
    }
    if let Some((from, to)) = remapped {
        log(format!(
            "v2 frames: game {game}: feed team {from} is not in the match, stored as {to}"
        ));
    }
    Ok((blue, red))
}

#[derive(Debug)]
pub struct Done {
    pub frames: usize,
    pub kb: usize,
    pub calls: u32,
    pub points: i64,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
enum Outcome {
    Done(Done),
    NoFeed(String),
}

/// The same warning at every point (an odd item list) is one line with its count
pub fn summarize(warnings: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut order = Vec::new();
    for w in warnings {
        let n = seen.entry(w).or_insert(0);
        if *n == 0 {
            order.push(w.as_str());
        }
        *n += 1;
    }
    order
        .into_iter()
        .map(|w| match seen[w] {
            1 => w.to_string(),
            n => format!("{w} (×{n})"),
        })
        .collect()
}

/// Pack and build off the async threads: zstd 19 takes about a second a game
async fn pack_and_build(
    walked: crate::client::feed::walk::Walked,
) -> Result<(
    crate::frames::pack::Packed,
    crate::frames::pack::Packed,
    Timeline,
    crate::client::feed::walk::Walked,
)> {
    tokio::task::spawn_blocking(move || {
        let pw = pack(Some(&walked.metadata), &walked.window)?;
        let pd = pack(None, &walked.details)?;
        let tl = build(&walked.window, &walked.details, STEP_S)?;
        Ok((pw, pd, tl, walked))
    })
    .await
    .map_err(|e| eyre!("pack task: {e}"))?
}

async fn one_game(src: &impl FeedSource, g: &FramesGame, cfg: &WalkConfig) -> Result<Outcome> {
    let walked = match walk_game(src, g.game_ext_id, g.end_time, cfg).await {
        Ok(w) => w,
        Err(WalkError::NoFeed) => return Ok(Outcome::NoFeed("no feed (204)".into())),
        // one normal try first, like the game details: a feed short only for a while is kept
        Err(e @ WalkError::ShortSides { .. }) if g.tries == 0 => return Err(eyre!("{e}")),
        Err(e @ WalkError::ShortSides { .. }) => return Ok(Outcome::NoFeed(e.to_string())),
        Err(WalkError::Failed(e)) => return Err(e),
    };
    // without a finished frame or the end the details saw, the walk may have stopped in a feed
    // hole: try again later (the details job fills the end), keep it on the last try
    let finished = walked
        .window
        .last()
        .is_some_and(|f| f.state.as_deref() == Some("finished"));
    if !finished && g.end_time.is_none() && g.tries + 1 < MAX_TRIES {
        return Err(eyre!(
            "no finished frame and no known end yet, {} frames, tried again later",
            walked.window.len()
        ));
    }
    let (blue, red) = frames_teams(
        g.game_ext_id,
        (walked.blue_team_ext_id, walked.red_team_ext_id),
        &g.event_teams,
    )?;
    let calls = walked.calls;
    let (pw, pd, tl, walked) = pack_and_build(walked).await?;
    write_final(g.game_ext_id, &pw, &pd, blue, red, &tl).await?;
    Ok(Outcome::Done(Done {
        frames: walked.window.len(),
        kb: (pw.data.len() + pd.data.len()) / 1024,
        calls,
        points: tl.points,
        warnings: tl.chunk.warnings,
    }))
}

/// Never returns. Each game in its own task: a panic is one failed try, not the loop
pub async fn frames_loop(backfill: bool) {
    let cfg = WalkConfig::default();
    let (mut done, mut no_feed, mut failed) = (0usize, 0usize, 0usize);
    let mut fails_in_a_row = 0;
    loop {
        let games = match next_games(BATCH, backfill).await {
            Ok(g) => g,
            Err(e) => {
                log(format!("v2 frames: queue not read: {e:#}"));
                sleep(IDLE).await;
                continue;
            }
        };
        if games.is_empty() {
            sleep(IDLE).await;
            continue;
        }
        'batch: for g in games {
            let started = Instant::now();
            let id = g.game_ext_id;
            let task_cfg = cfg.clone();
            let result =
                match tokio::spawn(async move { one_game(&HttpFeed, &g, &task_cfg).await }).await {
                    Ok(r) => r,
                    Err(e) => Err(eyre!("game {id}: task stopped: {e}")),
                };
            match result {
                Ok(Outcome::Done(d)) => {
                    done += 1;
                    fails_in_a_row = 0;
                    log(format!(
                        "v2 frames: game {id}: {} frames, {} points, {} KB, {} calls, {} s",
                        d.frames,
                        d.points,
                        d.kb,
                        d.calls,
                        started.elapsed().as_secs()
                    ));
                    for w in summarize(&d.warnings) {
                        log(format!("v2 frames: game {id}: {w}"));
                    }
                }
                Ok(Outcome::NoFeed(why)) => {
                    no_feed += 1;
                    fails_in_a_row = 0;
                    match mark_no_feed(id).await {
                        Ok(()) => log(format!("v2 frames: game {id}: no feed ({why})")),
                        Err(e) => log(format!(
                            "v2 frames: game {id}: no feed ({why}), not stored: {e:#}"
                        )),
                    }
                }
                // the feed or the network is down: not this game's fault, no try counted
                Err(e) if e.chain().any(|c| c.is::<Transient>()) => {
                    log(format!(
                        "v2 frames: game {id}: feed unavailable, no try counted, pausing {} min: {e:#}",
                        IDLE.as_secs() / 60
                    ));
                    sleep(IDLE).await;
                    break 'batch;
                }
                Err(e) => {
                    failed += 1;
                    fails_in_a_row += 1;
                    match bump_tries(id).await {
                        Ok(0) => log(format!(
                            "v2 frames: game {id} failed, but it was written meanwhile: {e:#}"
                        )),
                        Ok(n) if n >= MAX_TRIES => log(format!(
                            "v2 frames: game {id} is no_feed after {n} tries: {e:#}"
                        )),
                        Ok(_) => log(format!(
                            "v2 frames: game {id} failed, tried again later: {e:#}"
                        )),
                        Err(b) => log(format!(
                            "v2 frames: game {id} failed ({e:#}), try not counted: {b:#}"
                        )),
                    }
                }
            }
            if fails_in_a_row >= FAILS_BEFORE_PAUSE {
                log(format!(
                    "v2 frames: {fails_in_a_row} failures in a row, pausing {} min",
                    IDLE.as_secs() / 60
                ));
                fails_in_a_row = 0;
                sleep(IDLE).await;
                break 'batch;
            }
            if (done + no_feed + failed) % LOG_EVERY == 0 {
                log(format!(
                    "v2 frames: {done} done, {no_feed} no feed, {failed} failed"
                ));
            }
            sleep(PACE.saturating_sub(started.elapsed())).await;
        }
    }
}

/// Series built again from the archive, no feed calls
pub async fn rebuild(which: &Rebuild) -> Result<()> {
    let games = match which {
        Rebuild::All => archived_games().await?,
        Rebuild::Games(ids) => ids.clone(),
    };
    log(format!("rebuild-timeline: {} games", games.len()));
    let mut failed = 0;
    for id in &games {
        match rebuild_one(*id).await {
            Ok(points) => log(format!("rebuild-timeline: game {id}: {points} points")),
            Err(e) => {
                failed += 1;
                log(format!("rebuild-timeline: game {id} not rebuilt: {e:#}"));
            }
        }
    }
    if failed > 0 {
        return Err(eyre!("{failed} of {} games not rebuilt", games.len()));
    }
    Ok(())
}

async fn rebuild_one(id: i64) -> Result<i64> {
    let archive = read_archive(id).await?;
    let find = |ep: &str| {
        archive
            .iter()
            .find(|(e, _, _)| e == ep)
            .ok_or_else(|| eyre!("no {ep} archive"))
    };
    let (_, wf, wdata) = find("window")?;
    let (_, df, ddata) = find("details")?;
    let (wf, df, wdata, ddata) = (*wf, *df, wdata.clone(), ddata.clone());
    let (tl, feed) = tokio::task::spawn_blocking(move || -> Result<_> {
        let (meta, window) = unpack(wf, &wdata)?;
        let (_, details) = unpack(df, &ddata)?;
        let meta = meta.ok_or_else(|| eyre!("window archive without gameMetadata"))?;
        let feed = GameMetadata::parse(&meta)?.team_ext_ids()?;
        Ok((build(&window, &details, STEP_S)?, feed))
    })
    .await
    .map_err(|e| eyre!("rebuild task: {e}"))??;
    let teams = event_teams(&[id]).await?;
    let (blue, red) = frames_teams(id, feed, teams.get(&id).map_or(&[][..], Vec::as_slice))?;
    write_rebuild(id, blue, red, &tl)
        .await
        .wrap_err("series not written")?;
    Ok(tl.points)
}

/// `TRIFORCE_V2_FRAMES`: the after-game job and the live loop
pub fn enabled() -> Result<bool> {
    crate::env::switch("TRIFORCE_V2_FRAMES")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::feed::walk::mock::{Fail, MockFeed},
        frames::hand,
    };

    fn fast() -> WalkConfig {
        WalkConfig {
            pace: Duration::ZERO,
            backoff: [Duration::ZERO; 3],
            ..WalkConfig::default()
        }
    }

    fn game(tries: i32) -> FramesGame {
        FramesGame {
            game_ext_id: -420,
            frames: None,
            tries,
            end_time: None,
            event_teams: vec![-402, -403],
        }
    }

    #[test]
    fn repeated_warnings_once_with_a_count() {
        let w = vec!["a".to_string(), "b".into(), "a".into(), "a".into()];
        assert_eq!(summarize(&w), vec!["a (×3)", "b"]);
        assert!(summarize(&[]).is_empty());
    }

    #[tokio::test]
    async fn no_feed_and_short_sides_outcomes() {
        let r = one_game(&MockFeed::default(), &game(0), &fast())
            .await
            .unwrap();
        assert!(matches!(r, Outcome::NoFeed(_)));
        let mut meta = hand::metadata();
        meta["blueTeamMetadata"]["participantMetadata"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        let (w, d) = hand::game(20, true);
        let f = MockFeed::game(meta.clone(), w.clone(), d.clone());
        // first try: a failed try, later ones: no feed
        assert!(one_game(&f, &game(0), &fast()).await.is_err());
        let f = MockFeed::game(meta, w, d);
        let r = one_game(&f, &game(1), &fast()).await.unwrap();
        assert!(matches!(r, Outcome::NoFeed(why) if why.contains("1 blue and 5 red")));
    }

    #[tokio::test]
    async fn a_failing_walk_is_an_error() {
        let (w, d) = hand::game(20, true);
        let f = MockFeed::game(hand::metadata(), w, d).fail_on(2, Fail::Other);
        assert!(one_game(&f, &game(0), &fast()).await.is_err());
    }

    /// Without a finished frame or a known end the walk may be cut: tried again, kept on the
    /// last try
    #[tokio::test]
    async fn no_end_yet_waits_for_a_later_try() {
        let (w, d) = hand::game(40, false);
        let f = MockFeed::game(hand::metadata(), w, d);
        let e = one_game(&f, &game(0), &fast()).await.unwrap_err();
        assert!(e.to_string().contains("no known end yet"), "{e}");
    }

    /// The feed's team isn't the match's (LIT 2026): the archive and series go under the
    /// match's team, like the game details. Then `rebuild-timeline` from the archive
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn feed_team_not_in_the_match_then_rebuild() {
        use crate::dao::v2::frames::tests::{count, one, setup};
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        let mut meta = hand::metadata();
        meta["blueTeamMetadata"]["esportsTeamId"] = serde_json::json!("-499");
        let (w, d) = hand::game(60, true);
        let f = MockFeed::game(meta, w, d);
        let g = FramesGame {
            game_ext_id: -420,
            ..game(0)
        };
        let r = one_game(&f, &g, &fast()).await.unwrap();
        assert!(matches!(r, Outcome::Done(_)));
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -420").await,
            "done"
        );
        assert_eq!(one("SELECT tm.ext_id::text FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id JOIN v2_team tm ON tm.id = s.team_id WHERE g.ext_id = -420 AND s.side = 'blue'").await, "-402");
        // the rebuild reads the archive (the feed's -499 again) and maps it the same way
        rebuild(&Rebuild::Games(vec![-420])).await.unwrap();
        assert_eq!(one("SELECT tm.ext_id::text FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id JOIN v2_team tm ON tm.id = s.team_id WHERE g.ext_id = -420 AND s.side = 'blue'").await, "-402");
        assert_eq!(count("SELECT points FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -420").await, 7);
        // a game without an archive isn't rebuilt
        assert!(rebuild(&Rebuild::Games(vec![-421])).await.is_err());
        crate::dao::v2::test_cleanup().await;
    }
}
