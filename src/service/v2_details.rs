//! v2 game details: drains the queue of finished games (`v2_game.details IS NULL`) from the
//! live feed, ~1 game per second. All the progress is in the DB: stop and start any time.
//! A failed game waits (10 min, 40 min, 2 h 40, 10 h 40), after 5 tries it leaves the queue

use std::{sync::Arc, time::Duration};

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use tokio::time::{Instant, sleep};

use crate::{
    client::{
        adapters::v2::{
            details::{ends_paused, from_feed, from_vod, short_sides},
            remap_sides,
        },
        feed,
        gql::{
            Gql,
            ops::watch_vod::{self, GetWatchVod, VodEvent},
        },
        log,
    },
    dao::v2::details::{
        DetailsState, MAX_TRIES, QueueGame, bump_tries, next_games, write_details,
        write_player_stubs, write_vod_sides,
    },
};

/// Games per queue read
const BATCH: i64 = 20;
/// At most one game per second (3 feed calls)
const PACE: Duration = Duration::from_secs(1);
const IDLE: Duration = Duration::from_secs(5 * 60);
/// A progress line every this many games
const LOG_EVERY: usize = 100;
/// Failures in a row that look like the feed is down: the loop pauses (IDLE)
const FAILS_BEFORE_PAUSE: usize = 5;

#[derive(Debug, Default)]
struct Stats {
    done: usize,
    no_feed: usize,
    failed: usize,
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Done,
    NoFeed,
}

/// No feed (204), or one without 5 players a side: the sides of the match's finished games from
/// `watchVodQuery`
async fn from_watch_vod(gql: &Gql, g: &QueueGame) -> Result<Outcome> {
    let vars = watch_vod::Vars {
        event_id: g.event_ext_id.to_string(),
        hl: "en-US".into(),
    };
    let data = gql
        .call::<GetWatchVod>(&vars)
        .await
        .map_err(color_eyre::Report::new)?
        .data;
    let Some(VodEvent::Match(event)) = data.scheduled_event else {
        return Err(eyre!("watchVodQuery {}: not a match", g.event_ext_id));
    };
    // only this game: the other games of the match get their own feed try
    let mut sides: Vec<_> = from_vod(&event)
        .into_iter()
        .filter(|s| s.game_ext_id == g.game_ext_id)
        .collect();
    // same rule as the feed: a team the match doesn't list takes the match's other team
    let event_teams: Vec<i64> = g.teams.iter().map(|(t, _)| *t).collect();
    for s in &mut sides {
        if let Some((vod, event)) = remap_sides(s, &event_teams)? {
            log(format!(
                "v2 details: game {}: watchVodQuery team {vod} is not in the match, taken as {event}",
                s.game_ext_id
            ));
        }
    }
    if sides.is_empty() {
        return Err(eyre!(
            "game {}: no feed and no sides in watchVodQuery",
            g.game_ext_id
        ));
    }
    if write_vod_sides(&sides).await?.contains(&g.game_ext_id) {
        Ok(Outcome::NoFeed)
    } else {
        Err(eyre!("game {}: no feed, sides not stored", g.game_ext_id))
    }
}

async fn one_game(gql: &Gql, g: &QueueGame) -> Result<Outcome> {
    let Some((first, server_now)) = feed::window(g.game_ext_id, None).await? else {
        return from_watch_vod(gql, g).await;
    };
    let at = feed::starting_time(server_now);
    let (last, _) = feed::window(g.game_ext_id, Some(&at))
        .await?
        .ok_or_else(|| eyre!("game {}: last window empty", g.game_ext_id))?;
    if let Some([blue, red]) = short_sides(g.game_ext_id, &first, &last)? {
        // one normal try first: a feed short only for a while keeps its stats
        if g.tries == 0 {
            return Err(eyre!(
                "game {}: feed has {blue} blue and {red} red players",
                g.game_ext_id
            ));
        }
        log(format!(
            "v2 details: game {}: feed has {blue} blue and {red} red players, taken as no feed",
            g.game_ext_id
        ));
        return from_watch_vod(gql, g)
            .await
            .wrap_err_with(|| format!("feed with {blue} blue and {red} red players"));
    }
    let details = feed::details(g.game_ext_id, &at)
        .await?
        .ok_or_else(|| eyre!("game {}: details empty", g.game_ext_id))?;
    let row = from_feed(g.game_ext_id, &g.teams, &first, &last, &details)?;
    let (row, state) = if ends_paused(&last) {
        log(format!(
            "v2 details: game {}: feed ends paused, box score from the frames",
            g.game_ext_id
        ));
        (row.without_numbers(), DetailsState::WaitFrames)
    } else {
        (row, DetailsState::Done)
    };
    write_player_stubs(&row.player_stubs).await?;
    write_details(&row, state).await?;
    if let Some((feed, event)) = row.remapped {
        log(format!(
            "v2 details: game {}: feed team {feed} is not in the match, stored as {event}",
            g.game_ext_id
        ));
    }
    Ok(Outcome::Done)
}

/// Never returns. Each game in its own task: a panic is one failed try, not the loop
pub async fn details_loop(gql: Arc<Gql>, backfill: bool) {
    let mut stats = Stats::default();
    let mut fails_in_a_row = 0;
    loop {
        let games = match next_games(BATCH, backfill).await {
            Ok(g) => g,
            Err(e) => {
                log(format!("v2 details: queue not read: {e:#}"));
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
            let gql = gql.clone();
            let result = match tokio::spawn(async move { one_game(&gql, &g).await }).await {
                Ok(r) => r,
                Err(e) => Err(eyre!("game {id}: task stopped: {e}")),
            };
            match result {
                Ok(Outcome::Done) => {
                    stats.done += 1;
                    fails_in_a_row = 0;
                }
                Ok(Outcome::NoFeed) => {
                    stats.no_feed += 1;
                    fails_in_a_row = 0;
                }
                Err(e) => {
                    stats.failed += 1;
                    fails_in_a_row += 1;
                    match bump_tries(id).await {
                        Ok(n) if n >= MAX_TRIES => {
                            log(format!("v2 details: game {id} left after {n} tries: {e:#}"))
                        }
                        Ok(_) => log(format!(
                            "v2 details: game {id} failed, tried again later: {e:#}"
                        )),
                        Err(b) => log(format!(
                            "v2 details: game {id} failed ({e:#}), try not counted: {b:#}"
                        )),
                    }
                }
            }
            if fails_in_a_row >= FAILS_BEFORE_PAUSE {
                log(format!(
                    "v2 details: {fails_in_a_row} failures in a row, pausing {} min",
                    IDLE.as_secs() / 60
                ));
                fails_in_a_row = 0;
                sleep(IDLE).await;
                break 'batch;
            }
            let total = stats.done + stats.no_feed + stats.failed;
            if total % LOG_EVERY == 0 {
                log(format!(
                    "v2 details: {} done, {} no feed, {} failed",
                    stats.done, stats.no_feed, stats.failed
                ));
            }
            sleep(PACE.saturating_sub(started.elapsed())).await;
        }
    }
}

pub fn enabled() -> color_eyre::Result<bool> {
    crate::env::switch("TRIFORCE_V2_DETAILS")
}
