//! Writes `scheduleEvents` pages: events never go back, unknown leagues/teams are only added

use std::collections::{HashMap, HashSet};

use canyon_sql::{connection::DbConnection, date_time::NaiveDateTime, query::QueryParameter};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::client::log;

use super::{
    rows::{EventRow, EventRows, Progress, StreamRow, state_rank},
    upsert::{Col, MAX_PARAMS, Upsert, Val, run, t},
};

const LEAGUE_STUB: Upsert = Upsert {
    table: "v2_league",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("slug"),
        Col::Plain("image_url"),
    ],
    conflict: &[0],
    update: false,
    guard: None,
};
const LEAGUE_STUB_I18N: Upsert = Upsert {
    table: "v2_league_i18n",
    cols: &[
        Col::ByExt {
            name: "league_id",
            table: "v2_league",
        },
        Col::Plain("locale"),
        Col::Plain("name"),
        Col::Plain("region"),
    ],
    conflict: &[0, 1],
    update: false,
    guard: None,
};
const TEAM_STUB: Upsert = Upsert {
    table: "v2_team",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("slug"),
        Col::Plain("name"),
        Col::Plain("code"),
        Col::Plain("image_url"),
    ],
    conflict: &[0],
    update: false,
    guard: None,
};
const EVENT: Upsert = Upsert {
    table: "v2_event",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("kind"),
        Col::ByExt {
            name: "league_id",
            table: "v2_league",
        },
        Col::ByExt {
            name: "tournament_id",
            table: "v2_tournament",
        },
        Col::Plain("start_time"),
        Col::Plain("state"),
        Col::Plain("strategy_type"),
        Col::Plain("strategy_count"),
        // watchLiveQuery has no match type
        Col::KeepIfNull("match_type"),
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const EVENT_I18N: Upsert = Upsert {
    table: "v2_event_i18n",
    cols: &[
        Col::ByExt {
            name: "event_id",
            table: "v2_event",
        },
        Col::Plain("locale"),
        Col::Plain("block_name"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
const EVENT_TEAM: Upsert = Upsert {
    table: "v2_event_team",
    cols: &[
        Col::ByExt {
            name: "event_id",
            table: "v2_event",
        },
        Col::Plain("slot"),
        Col::ByExt {
            name: "team_id",
            table: "v2_team",
        },
        Col::Plain("game_wins"),
        Col::Plain("outcome"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
/// By event and number: a game id the API changes for the same game moves with it
const GAME: Upsert = Upsert {
    table: "v2_game",
    cols: &[
        Col::ByExt {
            name: "event_id",
            table: "v2_event",
        },
        Col::Plain("number"),
        Col::Plain("ext_id"),
        Col::Plain("state"),
    ],
    conflict: &[0, 1],
    update: true,
    // v2_state_rank() is in schema.sql, the same ranks as rows::state_rank
    guard: Some("v2_state_rank(EXCLUDED.state) >= v2_state_rank(v2_game.state)"),
};
const JOB_RUN: Upsert = Upsert {
    table: "v2_job_run",
    cols: &[Col::Plain("job"), Col::Plain("last_ok_time")],
    conflict: &[0],
    update: true,
    guard: None,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteStats {
    pub written: usize,
    /// Answers with a lower state, score or outcomes than what we have: skipped
    pub went_back: usize,
    pub unknown_tournaments: usize,
    /// Game ids that another event or number already has: skipped, logged
    pub moved_games: usize,
    /// Game winners stored from a score change (`score_winner`)
    pub winners: usize,
}

impl std::ops::AddAssign for WriteStats {
    fn add_assign(&mut self, o: Self) {
        self.written += o.written;
        self.went_back += o.went_back;
        self.unknown_tournaments += o.unknown_tournaments;
        self.moved_games += o.moved_games;
        self.winners += o.winners;
    }
}

fn bigint_list(from: usize, n: usize) -> String {
    (from..from + n)
        .map(|i| format!("${i}::bigint"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Games whose end is stored (the feed's `finished` frame, or their details)
pub async fn ended_games(ext_ids: &[i64]) -> Result<HashSet<i64>> {
    let mut out = HashSet::new();
    for chunk in ext_ids.chunks(MAX_PARAMS) {
        let sql = format!(
            "SELECT ext_id FROM v2_game WHERE ext_id IN ({}) AND end_time IS NOT NULL",
            bigint_list(1, chunk.len())
        );
        let params: Vec<&dyn QueryParameter> =
            chunk.iter().map(|x| x as &dyn QueryParameter).collect();
        let rows = ""
            .query_rows(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_game ended")?;
        for row in rows.get_postgres_rows().map_err(|e| eyre!(e))? {
            out.insert(row.try_get::<_, i64>(0).wrap_err("v2_game.ext_id")?);
        }
    }
    Ok(out)
}

/// Progress of the events we have, by ext_id
pub async fn stored_progress(ext_ids: &[i64]) -> Result<HashMap<i64, Progress>> {
    let mut out = HashMap::new();
    for chunk in ext_ids.chunks(MAX_PARAMS) {
        let sql = format!(
            "SELECT e.ext_id, e.state, COALESCE(SUM(t.game_wins), 0)::bigint, COUNT(t.outcome) \
             FROM v2_event e LEFT JOIN v2_event_team t ON t.event_id = e.id \
             WHERE e.ext_id IN ({}) GROUP BY e.ext_id, e.state",
            bigint_list(1, chunk.len())
        );
        let params: Vec<&dyn QueryParameter> =
            chunk.iter().map(|x| x as &dyn QueryParameter).collect();
        let rows = ""
            .query_rows(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_event stored progress")?;
        for row in rows.get_postgres_rows().map_err(|e| eyre!(e))? {
            let state: String = row.try_get(1).wrap_err("v2_event.state")?;
            out.insert(
                row.try_get::<_, i64>(0).wrap_err("v2_event.ext_id")?,
                Progress {
                    state_rank: state_rank(&state),
                    wins: row.try_get(2).wrap_err("v2_event_team wins")?,
                    outcomes: row.try_get(3).wrap_err("v2_event_team outcomes")?,
                },
            );
        }
    }
    Ok(out)
}

/// Wins of each team of the events we have: event ext_id → team ext_id → wins
async fn stored_team_wins(ext_ids: &[i64]) -> Result<HashMap<i64, HashMap<i64, i32>>> {
    let mut out: HashMap<i64, HashMap<i64, i32>> = HashMap::new();
    for chunk in ext_ids.chunks(MAX_PARAMS) {
        let sql = format!(
            "SELECT e.ext_id, t.ext_id, et.game_wins FROM v2_event e \
             JOIN v2_event_team et ON et.event_id = e.id JOIN v2_team t ON t.id = et.team_id \
             WHERE e.ext_id IN ({}) AND et.game_wins IS NOT NULL",
            bigint_list(1, chunk.len())
        );
        let params: Vec<&dyn QueryParameter> =
            chunk.iter().map(|x| x as &dyn QueryParameter).collect();
        let rows = ""
            .query_rows(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_event_team stored wins")?;
        for row in rows.get_postgres_rows().map_err(|e| eyre!(e))? {
            out.entry(row.try_get(0).wrap_err("v2_event.ext_id")?)
                .or_default()
                .insert(
                    row.try_get(1).wrap_err("v2_team.ext_id")?,
                    row.try_get(2).wrap_err("v2_event_team.game_wins")?,
                );
        }
    }
    Ok(out)
}

/// `(game ext_id, winner team ext_id)` when the score shows who won a game: between the stored
/// wins and the answer exactly one team got exactly one more, the other(s) the same. The game
/// is number `total wins` (games are played in order), and it must be live or finished. A
/// missed update (+2, both +1) or a team we didn't have before gives nothing
pub fn score_winner(before: &HashMap<i64, i32>, row: &EventRow) -> Option<(i64, i64)> {
    if row.teams.len() != 2 {
        return None;
    }
    let mut up = None;
    for t in &row.teams {
        let team = t.team_ext_id?;
        match t.game_wins? - *before.get(&team)? {
            0 => {}
            1 if up.is_none() => up = Some(team),
            _ => return None,
        }
    }
    let team = up?;
    let total: i32 = row.teams.iter().filter_map(|t| t.game_wins).sum();
    let game = row.games.iter().find(|g| g.number == total)?;
    matches!(game.state.as_str(), "inProgress" | "completed").then_some((game.ext_id, team))
}

/// `win`/`loss` on the 2 sides of a game that has no result yet. Needs the sides (`side` can't
/// be NULL): false when they aren't stored, the winner isn't one of them, or it has a result
async fn write_winner(game: i64, team: i64) -> Result<bool> {
    let n = ""
        .execute(
            "UPDATE v2_game_team gt SET result = CASE WHEN t.ext_id = $2 THEN 'win' ELSE 'loss' END \
             FROM v2_game g, v2_team t \
             WHERE g.ext_id = $1 AND gt.game_id = g.id AND t.id = gt.team_id AND gt.result IS NULL \
               AND (SELECT count(*) FROM v2_game_team c WHERE c.game_id = g.id AND c.result IS NULL) = 2 \
               AND EXISTS (SELECT 1 FROM v2_game_team w JOIN v2_team wt ON wt.id = w.team_id \
                 WHERE w.game_id = g.id AND wt.ext_id = $2)",
            &[&game, &team],
        )
        .await
        .map_err(|e| eyre!(e))
        .wrap_err_with(|| format!("v2 winner of game {game}"))?;
    Ok(n == 2)
}

/// Insert/update the streams of `events` and delete their other streams, one statement
fn streams_sql(streams: usize, events: usize) -> String {
    let rows: Vec<String> = (0..streams)
        .map(|i| {
            let b = i * 5;
            format!(
                "(${}::bigint, ${}::text, ${}::text, ${}::text, ${}::text)",
                b + 1,
                b + 2,
                b + 3,
                b + 4,
                b + 5
            )
        })
        .collect();
    let new_rows = if rows.is_empty() {
        "SELECT NULL::bigint, NULL::text, NULL::text, NULL::text, NULL::text WHERE false"
            .to_string()
    } else {
        format!("VALUES {}", rows.join(", "))
    };
    format!(
        "WITH new_s (event_ext, provider, parameter, locale, english_name) AS ({new_rows}), \
         resolved AS (SELECT e.id AS event_id, n.provider, n.parameter, n.locale, n.english_name \
           FROM new_s n JOIN v2_event e ON e.ext_id = n.event_ext), \
         ins AS (INSERT INTO v2_stream (event_id, provider, parameter, locale, english_name) \
           SELECT event_id, provider, parameter, locale, english_name FROM resolved \
           ON CONFLICT (event_id, provider, parameter) DO UPDATE SET locale = EXCLUDED.locale, \
           english_name = EXCLUDED.english_name \
           WHERE (v2_stream.locale, v2_stream.english_name) IS DISTINCT FROM (EXCLUDED.locale, EXCLUDED.english_name)) \
         DELETE FROM v2_stream s USING v2_event e WHERE e.id = s.event_id AND e.ext_id IN ({}) \
           AND NOT EXISTS (SELECT 1 FROM resolved r WHERE r.event_id = s.event_id \
           AND r.provider = s.provider AND r.parameter = s.parameter)",
        bigint_list(streams * 5 + 1, events)
    )
}

/// The streams of these events become exactly `events[].streams` (none → all deleted)
async fn replace_streams(events: &[&EventRow]) -> Result<()> {
    // params of one event: 5 per stream + its ext_id
    let cost = |e: &EventRow| e.streams.len() * 5 + 1;
    let mut start = 0;
    while start < events.len() {
        // whole events per statement, under the parameter limit
        let mut end = start + 1;
        let mut used = cost(events[start]);
        while end < events.len() && used + cost(events[end]) <= MAX_PARAMS {
            used += cost(events[end]);
            end += 1;
        }
        let chunk = &events[start..end];
        let mut streams: Vec<(i64, &StreamRow)> = Vec::new();
        let mut seen = HashSet::new();
        for e in chunk {
            for s in &e.streams {
                // the same stream twice in one answer would make the insert fail
                if seen.insert((e.ext_id, &s.provider, &s.parameter)) {
                    streams.push((e.ext_id, s));
                }
            }
        }
        let sql = streams_sql(streams.len(), chunk.len());
        let mut params: Vec<&dyn QueryParameter> =
            Vec::with_capacity(streams.len() * 5 + chunk.len());
        for (ext, s) in &streams {
            params.push(ext);
            params.push(&s.provider);
            params.push(&s.parameter);
            params.push(&s.locale);
            params.push(&s.english_name);
        }
        for e in chunk {
            params.push(&e.ext_id);
        }
        "".execute(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_stream replace")?;
        start = end;
    }
    Ok(())
}

/// (event, stored, new) already logged as going back
static LOGGED_BACK: std::sync::Mutex<Option<HashSet<(i64, Progress, Progress)>>> =
    std::sync::Mutex::new(None);

fn first_time_back(event: i64, stored: Progress, new: Progress) -> bool {
    let mut seen = LOGGED_BACK.lock().unwrap_or_else(|p| p.into_inner());
    seen.get_or_insert_with(HashSet::new)
        .insert((event, stored, new))
}

static LOGGED_MOVED: std::sync::Mutex<Option<HashSet<i64>>> = std::sync::Mutex::new(None);

fn first_time_moved(game: i64) -> bool {
    let mut seen = LOGGED_MOVED.lock().unwrap_or_else(|p| p.into_inner());
    seen.get_or_insert_with(HashSet::new).insert(game)
}

/// Ids of `(game ext, event ext, number)` that another event or number already holds. The
/// upsert goes by (event, number) and `ext_id` is unique too: writing them would fail the page
async fn moved_games(games: &[(i64, i64, i32)]) -> Result<HashSet<i64>> {
    let mut out = HashSet::new();
    for chunk in games.chunks(MAX_PARAMS / 3) {
        let values: Vec<String> = (0..chunk.len())
            .map(|i| {
                format!(
                    "(${}::bigint, ${}::bigint, ${}::int)",
                    3 * i + 1,
                    3 * i + 2,
                    3 * i + 3
                )
            })
            .collect();
        let sql = format!(
            "SELECT g.ext_id FROM v2_game g JOIN v2_event e ON e.id = g.event_id \
             JOIN (VALUES {}) AS n (ext, ev, num) ON n.ext = g.ext_id \
             WHERE e.ext_id <> n.ev OR g.number <> n.num",
            values.join(", ")
        );
        let mut params: Vec<&dyn QueryParameter> = Vec::with_capacity(chunk.len() * 3);
        for (g, e, n) in chunk {
            params.push(g);
            params.push(e);
            params.push(n);
        }
        let rows = ""
            .query_rows(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_game moved ids")?;
        for row in rows.get_postgres_rows().map_err(|e| eyre!(e))? {
            out.insert(row.try_get::<_, i64>(0).wrap_err("v2_game.ext_id")?);
        }
    }
    Ok(out)
}

async fn known_tournaments(ext_ids: &[i64]) -> Result<HashSet<i64>> {
    let mut out = HashSet::new();
    for chunk in ext_ids.chunks(MAX_PARAMS) {
        let sql = format!(
            "SELECT ext_id FROM v2_tournament WHERE ext_id IN ({})",
            bigint_list(1, chunk.len())
        );
        let params: Vec<&dyn QueryParameter> =
            chunk.iter().map(|x| x as &dyn QueryParameter).collect();
        let rows = ""
            .query_rows(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_tournament known")?;
        for row in rows.get_postgres_rows().map_err(|e| eyre!(e))? {
            out.insert(row.try_get::<_, i64>(0).wrap_err("v2_tournament.ext_id")?);
        }
    }
    Ok(out)
}

/// One en-US writer at a time (matches and live loops): the went-back check reads, then writes.
/// Only DB work runs under it, never HTTP
static EN_US_WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One page. en-US writes everything; another locale only its block names, for stored events
pub async fn write_events(rows: &EventRows, locale: &str) -> Result<WriteStats> {
    let _one_writer = if locale == "en-US" {
        Some(EN_US_WRITE.lock().await)
    } else {
        None
    };
    let mut stats = WriteStats::default();
    let ids: Vec<i64> = rows.events.iter().map(|e| e.ext_id).collect();
    let stored = stored_progress(&ids).await?;
    if locale != "en-US" {
        let i18n: Vec<Vec<Val>> = rows
            .events
            .iter()
            .filter(|e| stored.contains_key(&e.ext_id))
            .filter_map(|e| {
                let b = e.block_name.as_ref()?;
                Some(vec![Val::I64(Some(e.ext_id)), t(locale), t(b)])
            })
            .collect();
        stats.written = i18n.len();
        run(&EVENT_I18N, i18n).await?;
        return Ok(stats);
    }
    let mut accepted: Vec<&EventRow> = Vec::with_capacity(rows.events.len());
    for e in &rows.events {
        let new = e.progress();
        match stored.get(&e.ext_id) {
            Some(old) if new.went_back_from(old) => {
                stats.went_back += 1;
                // the same old copy comes back on every tick: logged once per run of the app
                if first_time_back(e.ext_id, *old, new) {
                    log(format!(
                        "v2 matches: event {} goes back ({old:?} → {new:?}), kept the stored one",
                        e.ext_id
                    ));
                }
            }
            _ => accepted.push(e),
        }
    }
    stats.written = accepted.len();
    // before the new wins are written: a score change gives a game winner
    let accepted_ids: Vec<i64> = accepted.iter().map(|e| e.ext_id).collect();
    let before_wins = stored_team_wins(&accepted_ids).await?;
    let page_games: Vec<(i64, i64, i32)> = accepted
        .iter()
        .flat_map(|e| e.games.iter().map(|g| (g.ext_id, e.ext_id, g.number)))
        .collect();
    // an id under two (event, number) keys in the same answer would fail the insert too
    let mut keys: HashMap<i64, (i64, i32)> = HashMap::new();
    let twice: Vec<i64> = page_games
        .iter()
        .filter(|(g, ev, n)| *keys.entry(*g).or_insert((*ev, *n)) != (*ev, *n))
        .map(|(g, _, _)| *g)
        .collect();
    let mut moved = moved_games(&page_games).await?;
    moved.extend(twice);
    stats.moved_games = moved.len();
    for g in &moved {
        // two games that swapped ids stay as stored and come back on every tick: logged once
        if first_time_moved(*g) {
            log(format!(
                "v2 matches: game {g} is under another event or number, skipped (kept as stored)"
            ));
        }
    }
    // winners first: if a later write fails, the next tick still sees the score change
    for e in &accepted {
        let Some((game, team)) = before_wins.get(&e.ext_id).and_then(|b| score_winner(b, e)) else {
            continue;
        };
        if moved.contains(&game) {
            continue;
        }
        if write_winner(game, team).await? {
            stats.winners += 1;
            log(format!("v2: game {game} won by team {team} (score change)"));
        } else {
            log(format!(
                "v2: game {game} won by team {team} (score change), not stored: no sides for it yet, or it already has a result"
            ));
        }
    }
    run(
        &LEAGUE_STUB,
        rows.league_stubs
            .iter()
            .map(|l| vec![Val::I64(Some(l.ext_id)), t(&l.slug), t(&l.image_url)])
            .collect(),
    )
    .await?;
    run(
        &LEAGUE_STUB_I18N,
        rows.league_stubs
            .iter()
            .map(|l| vec![Val::I64(Some(l.ext_id)), t("en-US"), t(&l.name), t("")])
            .collect(),
    )
    .await?;
    run(
        &TEAM_STUB,
        rows.team_stubs
            .iter()
            .map(|x| {
                vec![
                    Val::I64(Some(x.ext_id)),
                    t(""),
                    t(&x.name),
                    t(&x.code),
                    t(&x.image_url),
                ]
            })
            .collect(),
    )
    .await?;
    let tournaments: Vec<i64> = accepted
        .iter()
        .filter_map(|e| e.tournament_ext_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let known = known_tournaments(&tournaments).await?;
    stats.unknown_tournaments = accepted
        .iter()
        .filter(|e| e.tournament_ext_id.is_some_and(|x| !known.contains(&x)))
        .count();
    run(
        &EVENT,
        accepted
            .iter()
            .map(|e| {
                vec![
                    Val::I64(Some(e.ext_id)),
                    t(e.kind),
                    Val::I64(Some(e.league_ext_id)),
                    Val::I64(e.tournament_ext_id),
                    Val::Time(e.start_time),
                    t(&e.state),
                    Val::Text(e.strategy_type.clone()),
                    Val::I32(e.strategy_count),
                    Val::Text(e.match_type.clone()),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &EVENT_I18N,
        accepted
            .iter()
            .filter_map(|e| {
                let b = e.block_name.as_ref()?;
                Some(vec![Val::I64(Some(e.ext_id)), t("en-US"), t(b)])
            })
            .collect(),
    )
    .await?;
    run(
        &EVENT_TEAM,
        accepted
            .iter()
            .flat_map(|e| {
                e.teams.iter().map(|x| {
                    vec![
                        Val::I64(Some(e.ext_id)),
                        Val::I32(Some(x.slot)),
                        Val::I64(x.team_ext_id),
                        Val::I32(x.game_wins),
                        Val::Text(x.outcome.clone()),
                    ]
                })
            })
            .collect(),
    )
    .await?;
    run(
        &GAME,
        accepted
            .iter()
            .flat_map(|e| {
                e.games
                    .iter()
                    .filter(|g| !moved.contains(&g.ext_id))
                    .map(|g| {
                        vec![
                            Val::I64(Some(e.ext_id)),
                            Val::I32(Some(g.number)),
                            Val::I64(Some(g.ext_id)),
                            t(&g.state),
                        ]
                    })
            })
            .collect(),
    )
    .await?;
    drop_unneeded_sides(&page_games).await?;
    replace_streams(&accepted).await?;
    Ok(stats)
}

/// The `api` sides of this page's games that ended up `unneeded` (written by the live loop
/// before the game, never played)
async fn drop_unneeded_sides(page_games: &[(i64, i64, i32)]) -> Result<u64> {
    let ids: Vec<i64> = page_games.iter().map(|(g, _, _)| *g).collect();
    let mut dropped = 0;
    for chunk in ids.chunks(MAX_PARAMS) {
        let sql = format!(
            "DELETE FROM v2_game_team gt USING v2_game g \
             WHERE gt.game_id = g.id AND g.ext_id IN ({}) AND g.state = 'unneeded' \
               AND gt.side_source = 'api' AND gt.result IS NULL",
            bigint_list(1, chunk.len())
        );
        let params: Vec<&dyn QueryParameter> =
            chunk.iter().map(|x| x as &dyn QueryParameter).collect();
        dropped += ""
            .execute(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2 sides of unneeded games")?;
    }
    Ok(dropped)
}

/// A locale the reference loop stored names for (it skips the ones answering in English)
pub async fn locale_known(locale: &str) -> Result<bool> {
    let n: i64 = ""
        .query_one_for(
            "SELECT count(*) FROM v2_league_i18n WHERE locale = $1",
            &[&locale.to_string()],
        )
        .await
        .map_err(|e| eyre!(e))?;
    Ok(n > 0)
}

pub async fn job_last_ok(job: &str) -> Result<Option<NaiveDateTime>> {
    let job = job.to_string();
    let rows = ""
        .query_rows(
            "SELECT last_ok_time FROM v2_job_run WHERE job = $1",
            &[&job],
        )
        .await
        .map_err(|e| eyre!(e))?;
    rows.get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .first()
        .map(|r| r.try_get::<_, NaiveDateTime>(0))
        .transpose()
        .wrap_err("v2_job_run.last_ok_time")
}

pub async fn set_job_ok(job: &str, at: NaiveDateTime) -> Result<()> {
    run(&JOB_RUN, vec![vec![t(job), Val::Time(Some(at))]])
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::v2::rows::*;

    #[test]
    fn streams_sql_numbering() {
        let none = streams_sql(0, 2);
        assert!(none.contains("WHERE false"));
        assert!(none.contains("IN ($1::bigint, $2::bigint)"));
        let some = streams_sql(2, 1);
        assert!(some.contains("($1::bigint, $2::text, $3::text, $4::text, $5::text), ($6::bigint"));
        assert!(some.contains("IN ($11::bigint)"));
    }

    #[test]
    fn event_keeps_match_type_on_null() {
        // watchLiveQuery has no match type: a NULL never wipes a stored `tiebreaker`
        let sql = crate::dao::v2::upsert::build_sql(&EVENT, 1);
        assert!(sql.contains("match_type = COALESCE(EXCLUDED.match_type, v2_event.match_type)"));
    }

    #[test]
    fn game_guard_is_in_the_sql() {
        let sql = crate::dao::v2::upsert::build_sql(&GAME, 1);
        assert!(sql.contains("ON CONFLICT (event_id, number) DO UPDATE"));
        assert!(
            sql.contains("AND (v2_state_rank(EXCLUDED.state) >= v2_state_rank(v2_game.state))")
        );
    }

    // DB tests: ext_ids -299..-200, cleaned before and after
    async fn count(sql: &str) -> i64 {
        "".query_one_for::<i64>(sql, &[]).await.unwrap()
    }

    async fn cleanup() {
        crate::dao::v2::test_cleanup().await;
    }

    fn event(state: &str, wins: [Option<i32>; 2], games: &[&str], streams: usize) -> EventRow {
        EventRow {
            ext_id: -210,
            kind: "match",
            league_ext_id: -201,
            tournament_ext_id: Some(-299),
            start_time: None,
            state: state.into(),
            strategy_type: Some("bestOf".into()),
            strategy_count: Some(3),
            match_type: Some("normal".into()),
            block_name: Some("Week 1".into()),
            teams: vec![
                EventTeamRow {
                    slot: 1,
                    team_ext_id: Some(-202),
                    game_wins: wins[0],
                    outcome: None,
                },
                EventTeamRow {
                    slot: 2,
                    team_ext_id: None,
                    game_wins: wins[1],
                    outcome: None,
                },
            ],
            games: games
                .iter()
                .enumerate()
                .map(|(i, s)| GameRow {
                    ext_id: -220 - i as i64,
                    number: i as i32 + 1,
                    state: s.to_string(),
                })
                .collect(),
            streams: (0..streams)
                .map(|i| StreamRow {
                    provider: "twitch".into(),
                    parameter: format!("chan{i}"),
                    locale: "en-US".into(),
                    english_name: "English".into(),
                })
                .collect(),
        }
    }

    fn page(events: Vec<EventRow>, team_name: &str) -> EventRows {
        EventRows {
            events,
            league_stubs: vec![LeagueStub {
                ext_id: -201,
                slug: "test-l".into(),
                name: "Test L".into(),
                image_url: "i".into(),
            }],
            team_stubs: vec![TeamStub {
                ext_id: -202,
                name: team_name.into(),
                code: "TT".into(),
                image_url: "i".into(),
            }],
            skipped: 0,
        }
    }

    const EVENT_STATE: &str = "SELECT (CASE state WHEN 'completed' THEN 2 WHEN 'inProgress' THEN 1 ELSE 0 END)::bigint FROM v2_event WHERE ext_id = -210";
    const GAME1_STATE: &str = "SELECT (CASE state WHEN 'completed' THEN 2 WHEN 'inProgress' THEN 1 ELSE 0 END)::bigint FROM v2_game WHERE ext_id = -220";
    const STREAMS: &str = "SELECT count(*) FROM v2_stream s JOIN v2_event e ON e.id = s.event_id WHERE e.ext_id = -210";

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn events_never_go_back() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        crate::dao::v2::schema::apply().await.unwrap();
        cleanup().await;
        let first = page(
            vec![event(
                "inProgress",
                [Some(1), Some(0)],
                &["completed", "inProgress", "unstarted"],
                2,
            )],
            "Team A",
        );
        let s = write_events(&first, "en-US").await.unwrap();
        // unknown tournament -299: stored as NULL and counted
        assert_eq!(
            s,
            WriteStats {
                written: 1,
                went_back: 0,
                unknown_tournaments: 1,
                moved_games: 0,
                winners: 0
            }
        );
        let id = count("SELECT id::bigint FROM v2_event WHERE ext_id = -210").await;
        assert_eq!(
            count("SELECT count(*) FROM v2_event WHERE ext_id = -210 AND tournament_id IS NULL")
                .await,
            1
        );
        assert_eq!(count(STREAMS).await, 2);
        // TBD slot: team NULL
        assert_eq!(count("SELECT count(*) FROM v2_event_team t JOIN v2_event e ON e.id = t.event_id WHERE e.ext_id = -210 AND t.team_id IS NULL").await, 1);
        // same answer again: same ids, nothing doubled
        write_events(&first, "en-US").await.unwrap();
        assert_eq!(
            count("SELECT id::bigint FROM v2_event WHERE ext_id = -210").await,
            id
        );
        assert_eq!(count("SELECT count(*) FROM v2_game g JOIN v2_event e ON e.id = g.event_id WHERE e.ext_id = -210").await, 3);
        // an older copy (0-0) is skipped whole
        let old = page(
            vec![event(
                "inProgress",
                [Some(0), Some(0)],
                &["inProgress", "unstarted", "unstarted"],
                0,
            )],
            "Team A",
        );
        assert_eq!(write_events(&old, "en-US").await.unwrap().went_back, 1);
        assert_eq!(count(GAME1_STATE).await, 2);
        assert_eq!(count(STREAMS).await, 2);
        // same progress, game 1 says inProgress again: the game keeps completed
        let same = page(
            vec![event(
                "inProgress",
                [Some(1), Some(0)],
                &["inProgress", "inProgress", "unstarted"],
                1,
            )],
            "Team B",
        );
        assert_eq!(write_events(&same, "en-US").await.unwrap().went_back, 0);
        assert_eq!(count(GAME1_STATE).await, 2);
        assert_eq!(count(STREAMS).await, 1);
        // a stub never renames a team we have
        assert_eq!(
            count("SELECT count(*) FROM v2_team WHERE ext_id = -202 AND name = 'Team A'").await,
            1
        );
        // done: completed 2-0, streams gone
        let done = page(
            vec![event(
                "completed",
                [Some(2), Some(0)],
                &["completed", "completed", "unneeded"],
                0,
            )],
            "Team A",
        );
        write_events(&done, "en-US").await.unwrap();
        assert_eq!(count(EVENT_STATE).await, 2);
        assert_eq!(count(STREAMS).await, 0);
        // a playAll 1-1 ends in a tie for both
        let mut tie = event(
            "completed",
            [Some(1), Some(1)],
            &["completed", "completed"],
            0,
        );
        tie.ext_id = -212;
        for g in &mut tie.games {
            g.ext_id -= 10;
        }
        for x in &mut tie.teams {
            x.outcome = Some("tie".into());
        }
        write_events(&page(vec![tie], "Team A"), "en-US")
            .await
            .unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_event_team t JOIN v2_event e ON e.id = t.event_id WHERE e.ext_id = -212 AND t.outcome = 'tie'").await, 2);
        // and a stale inProgress after it is skipped
        let stale = page(
            vec![event(
                "inProgress",
                [Some(2), Some(0)],
                &["completed", "completed", "unstarted"],
                1,
            )],
            "Team A",
        );
        assert_eq!(write_events(&stale, "en-US").await.unwrap().went_back, 1);
        assert_eq!(count(EVENT_STATE).await, 2);
        // a game id stored under event -210 shows up in event -213: skipped, the page goes on
        let mut other = event("unstarted", [None, None], &["unstarted"], 0);
        other.ext_id = -213;
        let s = write_events(&page(vec![other], "Team A"), "en-US")
            .await
            .unwrap();
        assert_eq!((s.written, s.moved_games), (1, 1));
        assert_eq!(
            count("SELECT count(*) FROM v2_game g JOIN v2_event e ON e.id = g.event_id WHERE e.ext_id = -213")
                .await,
            0
        );
        // games 1 and 2 swap ids in a newer answer: both kept as stored, the page goes on
        let mut swapped = event(
            "completed",
            [Some(2), Some(0)],
            &["completed", "completed", "unneeded"],
            0,
        );
        swapped.games.swap(0, 1);
        swapped.games[0].number = 1;
        swapped.games[1].number = 2;
        let s = write_events(&page(vec![swapped], "Team A"), "en-US")
            .await
            .unwrap();
        assert_eq!(s.moved_games, 2);
        assert_eq!(count("SELECT g.ext_id FROM v2_game g JOIN v2_event e ON e.id = g.event_id WHERE e.ext_id = -210 AND g.number = 1").await, -220);
        cleanup().await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn other_locale_only_block_names_of_stored_events() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        crate::dao::v2::schema::apply().await.unwrap();
        cleanup().await;
        write_events(
            &page(
                vec![event("unstarted", [None, None], &["unstarted"], 0)],
                "Team A",
            ),
            "en-US",
        )
        .await
        .unwrap();
        let mut es = event("unstarted", [None, None], &["unstarted"], 0);
        es.block_name = Some("Semana 1".into());
        let mut unknown = es.clone();
        unknown.ext_id = -211;
        let s = write_events(&page(vec![es, unknown], "Otro"), "es-ES")
            .await
            .unwrap();
        assert_eq!(s.written, 1);
        assert_eq!(count("SELECT count(*) FROM v2_event_i18n i JOIN v2_event e ON e.id = i.event_id WHERE e.ext_id = -210 AND i.locale = 'es-ES' AND i.block_name = 'Semana 1'").await, 1);
        assert_eq!(
            count("SELECT count(*) FROM v2_event WHERE ext_id = -211").await,
            0
        );
        assert_eq!(
            count("SELECT count(*) FROM v2_team WHERE ext_id = -202 AND name = 'Team A'").await,
            1
        );
        cleanup().await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn job_run_set_and_read() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        crate::dao::v2::schema::apply().await.unwrap();
        cleanup().await;
        assert_eq!(job_last_ok("test_job").await.unwrap(), None);
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 18)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        set_job_ok("test_job", at).await.unwrap();
        set_job_ok("test_job", at + chrono::TimeDelta::hours(1))
            .await
            .unwrap();
        assert_eq!(
            job_last_ok("test_job").await.unwrap(),
            Some(at + chrono::TimeDelta::hours(1))
        );
        cleanup().await;
    }

    /// `event()` with a second team (-203), for the winner rule
    fn two_teams(wins: [i32; 2], games: &[&str]) -> EventRow {
        let mut e = event("inProgress", [Some(wins[0]), Some(wins[1])], games, 0);
        e.teams[1].team_ext_id = Some(-203);
        e
    }

    fn before(a: i32, b: i32) -> HashMap<i64, i32> {
        HashMap::from([(-202, a), (-203, b)])
    }

    #[test]
    fn winner_from_one_more_win() {
        let live = ["completed", "inProgress", "unstarted"];
        // 1-0 → 1-1: game 2 is the one just won
        let done = ["completed", "completed", "unstarted"];
        assert_eq!(
            score_winner(&before(1, 0), &two_teams([1, 1], &done)),
            Some((-221, -203))
        );
        // the score can come before the game state: game 2 still inProgress counts
        assert_eq!(
            score_winner(&before(1, 0), &two_teams([1, 1], &live)),
            Some((-221, -203))
        );
        // 0-0 → 1-0: game 1
        assert_eq!(
            score_winner(&before(0, 0), &two_teams([1, 0], &live)),
            Some((-220, -202))
        );
    }

    #[test]
    fn no_winner_when_unsure() {
        let g = ["completed", "completed", "inProgress"];
        // same score, missed update (+2), both +1, going back
        assert_eq!(score_winner(&before(1, 0), &two_teams([1, 0], &g)), None);
        assert_eq!(score_winner(&before(0, 0), &two_teams([2, 0], &g)), None);
        assert_eq!(score_winner(&before(0, 0), &two_teams([1, 1], &g)), None);
        assert_eq!(score_winner(&before(1, 1), &two_teams([1, 0], &g)), None);
        // a team we didn't have before (was TBD)
        assert_eq!(
            score_winner(&HashMap::from([(-202, 0)]), &two_teams([0, 1], &g)),
            None
        );
        // game `total wins` not played (unstarted) or not in the answer
        let unstarted = ["completed", "unstarted", "unstarted"];
        assert_eq!(
            score_winner(&before(1, 0), &two_teams([1, 1], &unstarted)),
            None
        );
        assert_eq!(
            score_winner(&before(1, 0), &two_teams([1, 1], &["completed"])),
            None
        );
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn winner_written_from_a_score_change() {
        use crate::dao::v2::{details::write_live_sides, rows::GameSides};
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        crate::dao::v2::schema::apply().await.unwrap();
        cleanup().await;
        let answer = |wins: [i32; 2], games: &[&str]| {
            let mut p = page(vec![two_teams(wins, games)], "Team A");
            p.team_stubs.push(TeamStub {
                ext_id: -203,
                name: "Team B".into(),
                code: "TB".into(),
                image_url: "i".into(),
            });
            p
        };
        let live = ["inProgress", "unstarted", "unstarted"];
        let mut first = answer([0, 0], &live);
        first.team_stubs.push(TeamStub {
            ext_id: -204,
            name: "Team C".into(),
            code: "TC".into(),
            image_url: "i".into(),
        });
        write_events(&first, "en-US").await.unwrap();
        // game 3 before it's played; team C is stored but not in the match: never written
        let game_3 = GameSides {
            game_ext_id: -222,
            sides: vec![(-204, "red"), (-202, "blue")],
        };
        assert_eq!(
            write_live_sides(std::slice::from_ref(&game_3))
                .await
                .unwrap(),
            1
        );
        // sides of game 1 while it's played; game 2 never gets sides
        let sides = |game, a| GameSides {
            game_ext_id: game,
            sides: vec![(-202, a), (-203, if a == "blue" { "red" } else { "blue" })],
        };
        assert_eq!(write_live_sides(&[sides(-220, "red")]).await.unwrap(), 2);
        // sides can still change before the game: the last one stays
        assert_eq!(write_live_sides(&[sides(-220, "blue")]).await.unwrap(), 2);
        assert_eq!(write_live_sides(&[sides(-220, "blue")]).await.unwrap(), 0);
        let results = "SELECT string_agg(t.ext_id || ':' || gt.side || ':' || coalesce(gt.result, '-'), ',' ORDER BY t.ext_id) \
            FROM v2_game_team gt JOIN v2_team t ON t.id = gt.team_id JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -220";
        let r: String = "".query_one_for(results, &[]).await.unwrap();
        assert_eq!(r, "-203:red:-,-202:blue:-");
        // 1-0: team A won game 1
        let s = write_events(
            &answer([1, 0], &["completed", "inProgress", "unstarted"]),
            "en-US",
        )
        .await
        .unwrap();
        assert_eq!(s.winners, 1);
        let r: String = "".query_one_for(results, &[]).await.unwrap();
        assert_eq!(r, "-203:red:loss,-202:blue:win");
        // the same answer again: nothing new
        let s = write_events(
            &answer([1, 0], &["completed", "inProgress", "unstarted"]),
            "en-US",
        )
        .await
        .unwrap();
        assert_eq!(s.winners, 0);
        // 1-1, but game 2 has no sides: nothing stored, nothing fails
        let s = write_events(
            &answer([1, 1], &["completed", "completed", "inProgress"]),
            "en-US",
        )
        .await
        .unwrap();
        assert_eq!(s.winners, 0);
        assert_eq!(
            count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -221").await,
            0
        );
        // no match type in the answer (watchLiveQuery): the stored one stays
        let mut p = answer([1, 1], &["completed", "completed", "inProgress"]);
        p.events[0].match_type = None;
        write_events(&p, "en-US").await.unwrap();
        assert_eq!(
            count("SELECT count(*) FROM v2_event WHERE ext_id = -210 AND match_type = 'normal'")
                .await,
            1
        );
        // game 3 ends up unneeded: its sides from before the game go, and never come back
        let three = "SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -222";
        assert_eq!(count(three).await, 1);
        write_events(
            &answer([2, 0], &["completed", "completed", "unneeded"]),
            "en-US",
        )
        .await
        .unwrap();
        assert_eq!(count(three).await, 0);
        assert_eq!(write_live_sides(&[game_3]).await.unwrap(), 0);
        cleanup().await;
    }
}
