//! v2 live: `watchLiveQuery` once a minute → state, score, games, sides and streams of the live
//! matches, and the winner of a game as soon as the score shows it. For each game being played,
//! the live feed: when it really started and ended, and each team's totals right now.
//! This query has no event state (everything in it is `inProgress`): when a match leaves it,
//! the schedule is read again until the match is `completed`

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use color_eyre::{Result, eyre::WrapErr};

use crate::{
    client::{
        adapters::v2::{details::from_live_feed, events::from_live},
        feed,
        gql::{
            Gql,
            ops::watch_live::{GetWatchLive, Vars},
        },
        log,
    },
    dao::v2::{
        details::{write_live_game, write_live_sides},
        events::{ended_games, stored_progress, write_events},
        rows::{EventRows, state_rank},
    },
};

/// How long a match that left the live answer is looked for in the schedule
const CONFIRM_FOR: Duration = Duration::from_secs(20 * 60);

/// Matches live on the last tick, the ones that left it and aren't `completed` yet, and the
/// games whose feed failure was already logged
#[derive(Debug, Default)]
struct Watch {
    live: HashSet<i64>,
    left: HashMap<i64, Instant>,
    feed_logged: HashSet<i64>,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

fn with_watch<T>(f: impl FnOnce(&mut Watch) -> T) -> T {
    let mut w = WATCH.lock().unwrap_or_else(|p| p.into_inner());
    f(w.get_or_insert_with(Watch::default))
}

impl Watch {
    /// A match live before and not now has left (from `at`); one live again hasn't
    fn update(&mut self, now_live: HashSet<i64>, at: Instant) {
        for gone in self.live.difference(&now_live) {
            self.left.entry(*gone).or_insert(at);
        }
        self.left.retain(|m, _| !now_live.contains(m));
        self.live = now_live;
    }

    /// Matches done waiting: `completed`, or `CONFIRM_FOR` gone by (whatever the schedule
    /// answered). (match, completed, minutes since it left)
    fn settle(&mut self, completed: impl Fn(i64) -> bool, now: Instant) -> Vec<(i64, bool, u64)> {
        let mut done = Vec::new();
        self.left.retain(|m, since| {
            let waited = now.saturating_duration_since(*since);
            let completed = completed(*m);
            if completed || waited >= CONFIRM_FOR {
                done.push((*m, completed, waited.as_secs() / 60));
                return false;
            }
            true
        });
        done.sort_unstable();
        done
    }
}

pub fn enabled() -> Result<bool> {
    crate::env::switch("TRIFORCE_V2_LIVE")
}

/// One tick. Nothing live and nothing waiting for its end: nothing written, nothing logged
pub async fn live_once(gql: &Gql) -> Result<()> {
    let data = gql
        .call::<GetWatchLive>(&Vars::default())
        .await
        .map_err(color_eyre::Report::new)
        .wrap_err("watchLiveQuery")?
        .data;
    let (rows, sides) = from_live(data);
    let live: HashSet<i64> = rows
        .events
        .iter()
        .filter(|e| e.kind == "match")
        .map(|e| e.ext_id)
        .collect();
    let left: Vec<i64> = with_watch(|w| {
        w.update(live, Instant::now());
        w.left.keys().copied().collect()
    });
    if !rows.events.is_empty() || rows.skipped > 0 {
        // sides first: a game's winner is only stored on its sides (games we don't have yet
        // are left out), then again for the games this answer added
        let mut side_rows = write_live_sides(&sides).await?;
        let s = write_events(&rows, "en-US")
            .await
            .wrap_err("write watchLiveQuery")?;
        side_rows += write_live_sides(&sides).await?;
        let f = feed_games(&rows).await;
        // a show alone, with nothing new, isn't worth a line every minute
        let news = side_rows > 0 || s.winners > 0 || s.went_back > 0 || rows.skipped > 0;
        if news || f.written > 0 || f.waiting > 0 || f.failed > 0 {
            log(format!(
                "v2 live: {} events, {} went back, {} skipped, {side_rows} side rows changed, {} game winners, {} games from the feed ({} without stats yet, {} failed)",
                s.written, s.went_back, rows.skipped, s.winners, f.written, f.waiting, f.failed
            ));
        }
    }
    if !left.is_empty() {
        // the writes of this tick are done: a failed end check is only logged
        if let Err(e) = confirm_ended(gql, &left).await {
            log(format!(
                "v2 live: end check failed, tried again next minute: {e:#}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct FeedStats {
    written: usize,
    waiting: usize,
    failed: usize,
}

/// The feed of every game the answer says is being played, all at once. A game without feed
/// yet (204, draft) is skipped, one without stats right now (`feed::NoStats`) waits; a failure is
/// logged once per game and run of the app. The next tick tries all of them again. A game whose
/// end is stored is left out: the API can say `inProgress` for minutes after the end
async fn feed_games(rows: &EventRows) -> FeedStats {
    let mut stats = FeedStats::default();
    let playing: Vec<(i64, Vec<i64>)> = rows
        .events
        .iter()
        .filter(|e| e.kind == "match")
        .flat_map(|e| {
            let teams: Vec<i64> = e.teams.iter().filter_map(|t| t.team_ext_id).collect();
            e.games
                .iter()
                .filter(|g| g.state == "inProgress")
                .map(move |g| (g.ext_id, teams.clone()))
        })
        .collect();
    let ids: Vec<i64> = playing.iter().map(|(g, _)| *g).collect();
    let ended = match ended_games(&ids).await {
        Ok(ended) => ended,
        Err(err) => {
            log(format!("v2 live: ended games not read: {err:#}"));
            stats.failed = playing.len();
            return stats;
        }
    };
    let mut tasks = tokio::task::JoinSet::new();
    for (game, teams) in playing.into_iter().filter(|(g, _)| !ended.contains(g)) {
        tasks.spawn(async move { (game, live_game(game, &teams).await) });
    }
    while let Some(done) = tasks.join_next().await {
        match done {
            Ok((_, Ok(true))) => stats.written += 1,
            Ok((_, Ok(false))) => {}
            Ok((_, Err(err))) if err.downcast_ref::<feed::NoStats>().is_some() => {
                stats.waiting += 1;
            }
            Ok((game, Err(err))) => {
                stats.failed += 1;
                if with_watch(|w| w.feed_logged.insert(game)) {
                    log(format!(
                        "v2 live: game {game} feed: {err:#} (next failures of this game only counted)"
                    ));
                }
            }
            Err(err) => {
                stats.failed += 1;
                log(format!("v2 live: feed task stopped: {err}"));
            }
        }
    }
    stats
}

/// `false` when the feed has nothing for the game yet, it already has its final details, or a
/// team of it isn't stored
async fn live_game(game: i64, teams: &[i64]) -> Result<bool> {
    let Some((first, now)) = feed::window(game, None).await? else {
        return Ok(false);
    };
    let last = feed::window(game, Some(&feed::starting_time(now)))
        .await?
        .map(|(w, _)| w);
    let row = from_live_feed(game, teams, &first, last.as_ref().unwrap_or(&first))?;
    Ok(write_live_game(&row).await? == 2)
}

/// Matches that left the live answer: the schedule again (en-US) while one of them isn't
/// `completed`, until it is or `CONFIRM_FOR` went by (then the matches loop has it on its next
/// tick). The time limit holds even while the schedule fails
async fn confirm_ended(gql: &Gql, left: &[i64]) -> Result<()> {
    let done = state_rank("completed");
    let mut stored = stored_progress(left).await?;
    let open = left
        .iter()
        .any(|m| stored.get(m).is_none_or(|p| p.state_rank != done));
    let checked = if open {
        let checked = super::v2_matches::recent_en_us(gql)
            .await
            .wrap_err("schedule for the matches that left the live answer");
        if checked.is_ok() {
            stored = stored_progress(left).await?;
        }
        checked
    } else {
        Ok(())
    };
    let settled = with_watch(|w| {
        w.settle(
            |m| stored.get(&m).is_some_and(|p| p.state_rank == done),
            Instant::now(),
        )
    });
    for (m, completed, mins) in settled {
        if completed {
            log(format!(
                "v2 live: match {m} completed ({mins} min after it left the live answer)"
            ));
        } else {
            log(format!(
                "v2 live: match {m} not completed {mins} min after it left the live answer, left to the matches loop"
            ));
        }
    }
    checked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_that_leave_the_live_answer() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(60);
        let mut w = Watch::default();
        w.update(HashSet::from([1, 2]), t0);
        assert!(w.left.is_empty());
        // 2 left
        w.update(HashSet::from([1]), t1);
        assert_eq!(w.left, HashMap::from([(2, t1)]));
        // still gone: keeps the first time it left
        w.update(HashSet::from([1]), t1 + Duration::from_secs(60));
        assert_eq!(w.left, HashMap::from([(2, t1)]));
        // back live (a cached old answer): not waiting anymore; 1 left
        w.update(HashSet::from([2]), t1 + Duration::from_secs(120));
        assert_eq!(w.left.keys().copied().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn left_matches_settle() {
        let t0 = Instant::now();
        let mut w = Watch::default();
        w.update(HashSet::from([1, 2]), t0);
        w.update(HashSet::new(), t0);
        // 1 completed right away, 2 still open
        assert_eq!(w.settle(|m| m == 1, t0), [(1, true, 0)]);
        // the schedule failing (nothing completed): 2 waits, then gives up at the limit
        assert!(w.settle(|_| false, t0 + Duration::from_secs(60)).is_empty());
        assert_eq!(w.settle(|_| false, t0 + CONFIRM_FOR), [(2, false, 20)]);
        assert!(w.left.is_empty());
    }
}
