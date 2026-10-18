//! Writes the v2 reference data in FK order

use std::collections::BTreeMap;

use canyon_sql::{connection::DbConnection, query::QueryParameter};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::client::log;

use super::{
    rows::ReferenceRows,
    upsert::{Col, MAX_PARAMS, Upsert, Val, run, t},
};

const LEAGUE: Upsert = Upsert {
    table: "v2_league",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("slug"),
        Col::Plain("image_url"),
        Col::Plain("dark_image_url"),
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const LEAGUE_I18N: Upsert = Upsert {
    table: "v2_league_i18n",
    cols: &[
        Col::ByExt {
            name: "league_id",
            table: "v2_league",
        },
        Col::Plain("locale"),
        Col::Plain("name"),
        Col::Plain("region"),
        Col::Plain("position"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
const SEASON: Upsert = Upsert {
    table: "v2_season",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("start_time"),
        Col::Plain("end_time"),
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const SEASON_I18N: Upsert = Upsert {
    table: "v2_season_i18n",
    cols: &[
        Col::ByExt {
            name: "season_id",
            table: "v2_season",
        },
        Col::Plain("locale"),
        Col::Plain("name"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
const SPLIT: Upsert = Upsert {
    table: "v2_split",
    cols: &[Col::Plain("ext_id")],
    conflict: &[0],
    update: false,
    guard: None,
};
const TOURNAMENT: Upsert = Upsert {
    table: "v2_tournament",
    cols: &[
        Col::Plain("ext_id"),
        Col::ByExt {
            name: "league_id",
            table: "v2_league",
        },
        Col::ByExt {
            name: "season_id",
            table: "v2_season",
        },
        Col::ByExt {
            name: "split_id",
            table: "v2_split",
        },
        Col::Plain("slug"),
        Col::Plain("start_time"),
        Col::Plain("end_time"),
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const TOURNAMENT_I18N: Upsert = Upsert {
    table: "v2_tournament_i18n",
    cols: &[
        Col::ByExt {
            name: "tournament_id",
            table: "v2_tournament",
        },
        Col::Plain("locale"),
        Col::Plain("name"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
const STAGE: Upsert = Upsert {
    table: "v2_stage",
    cols: &[
        Col::Plain("ext_id"),
        Col::ByExt {
            name: "tournament_id",
            table: "v2_tournament",
        },
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const STAGE_I18N: Upsert = Upsert {
    table: "v2_stage_i18n",
    cols: &[
        Col::ByExt {
            name: "stage_id",
            table: "v2_stage",
        },
        Col::Plain("locale"),
        Col::Plain("name"),
    ],
    conflict: &[0, 1],
    update: true,
    guard: None,
};
const TEAM: Upsert = Upsert {
    table: "v2_team",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("slug"),
        Col::Plain("name"),
        Col::Plain("code"),
        Col::Plain("image_url"),
        Col::Plain("bg_image_url"),
        // light_image_url: no source yet. alt_image_url comes from the old API (write_alt_images)
        Col::ByExt {
            name: "home_league_id",
            table: "v2_league",
        },
    ],
    conflict: &[0],
    update: true,
    guard: None,
};
const PLAYER: Upsert = Upsert {
    table: "v2_player",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("handle"),
        Col::Plain("first_name"),
        Col::Plain("last_name"),
        Col::Plain("image_url"),
        Col::Plain("role"),
    ],
    conflict: &[0],
    update: true,
    guard: None,
};

/// Leagues first (everything points at them), then two independent groups that both always
/// run: teams, players and rosters; seasons down to stages. Returns the first error and logs
/// the second one
pub async fn write_reference(rows: &ReferenceRows) -> Result<()> {
    run(
        &LEAGUE,
        rows.leagues
            .iter()
            .map(|l| {
                vec![
                    Val::I64(Some(l.ext_id)),
                    t(&l.slug),
                    t(&l.image_url),
                    Val::Text(l.dark_image_url.clone()),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &LEAGUE_I18N,
        rows.leagues
            .iter()
            .flat_map(|l| {
                l.texts.iter().map(move |x| {
                    vec![
                        Val::I64(Some(l.ext_id)),
                        t(&x.locale),
                        t(&x.name),
                        t(&x.region),
                        Val::I32(x.position),
                    ]
                })
            })
            .collect(),
    )
    .await?;
    let teams = write_teams(rows).await;
    let structure = write_structure(rows).await;
    match (teams, structure) {
        (Err(a), Err(b)) => {
            log(format!("v2 reference: {b:#}"));
            Err(a)
        }
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Ok(()), Ok(())) => Ok(()),
    }
}

/// Team, player and roster sync
async fn write_teams(rows: &ReferenceRows) -> Result<()> {
    run(
        &TEAM,
        rows.teams
            .iter()
            .map(|x| {
                vec![
                    Val::I64(Some(x.ext_id)),
                    t(&x.slug),
                    t(&x.name),
                    t(&x.code),
                    t(&x.image_url),
                    Val::Text(x.bg_image_url.clone()),
                    Val::I64(x.home_league_ext_id),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &PLAYER,
        rows.players
            .iter()
            .map(|p| {
                vec![
                    Val::I64(Some(p.ext_id)),
                    t(&p.handle),
                    t(&p.first_name),
                    t(&p.last_name),
                    Val::Text(p.image_url.clone()),
                    t(&p.role),
                ]
            })
            .collect(),
    )
    .await?;
    sync_team_players(rows)
        .await
        .wrap_err("v2_team_player sync")
}

/// Seasons, splits, tournaments and stages with their texts, in FK order
async fn write_structure(rows: &ReferenceRows) -> Result<()> {
    run(
        &SEASON,
        rows.seasons
            .iter()
            .map(|s| {
                vec![
                    Val::I64(Some(s.ext_id)),
                    Val::Time(s.start_time),
                    Val::Time(s.end_time),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &SEASON_I18N,
        rows.seasons
            .iter()
            .flat_map(|s| {
                s.texts
                    .iter()
                    .map(move |x| vec![Val::I64(Some(s.ext_id)), t(&x.locale), t(&x.name)])
            })
            .collect(),
    )
    .await?;
    run(
        &SPLIT,
        rows.splits
            .iter()
            .map(|id| vec![Val::I64(Some(*id))])
            .collect(),
    )
    .await?;
    run(
        &TOURNAMENT,
        rows.tournaments
            .iter()
            .map(|x| {
                vec![
                    Val::I64(Some(x.ext_id)),
                    Val::I64(Some(x.league_ext_id)),
                    Val::I64(x.season_ext_id),
                    Val::I64(x.split_ext_id),
                    t(&x.slug),
                    Val::Time(x.start_time),
                    Val::Time(x.end_time),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &TOURNAMENT_I18N,
        rows.tournaments
            .iter()
            .flat_map(|x| {
                x.texts
                    .iter()
                    .map(move |n| vec![Val::I64(Some(x.ext_id)), t(&n.locale), t(&n.name)])
            })
            .collect(),
    )
    .await?;
    run(
        &STAGE,
        rows.stages
            .iter()
            .map(|s| {
                vec![
                    Val::I64(Some(s.ext_id)),
                    Val::I64(Some(s.tournament_ext_id)),
                ]
            })
            .collect(),
    )
    .await?;
    run(
        &STAGE_I18N,
        rows.stages
            .iter()
            .flat_map(|s| {
                s.texts
                    .iter()
                    .map(move |n| vec![Val::I64(Some(s.ext_id)), t(&n.locale), t(&n.name)])
            })
            .collect(),
    )
    .await?;
    Ok(())
}

/// Insert the new pairs and delete the old ones of the same teams, in one statement (atomic, never empty)
fn team_players_sql(pairs: usize, teams: usize) -> String {
    let mut n = 0;
    let mut next = || {
        n += 1;
        format!("${n}::bigint")
    };
    let pairs: Vec<String> = (0..pairs)
        .map(|_| format!("({}, {})", next(), next()))
        .collect();
    let scope: Vec<String> = (0..teams).map(|_| next()).collect();
    let pairs_sql = if pairs.is_empty() {
        "SELECT NULL::bigint, NULL::bigint WHERE false".to_string()
    } else {
        format!("VALUES {}", pairs.join(", "))
    };
    format!(
        "WITH new_pairs (team_ext, player_ext) AS ({pairs_sql}), \
         resolved AS (SELECT t.id AS team_id, p.id AS player_id FROM new_pairs n \
           JOIN v2_team t ON t.ext_id = n.team_ext JOIN v2_player p ON p.ext_id = n.player_ext), \
         ins AS (INSERT INTO v2_team_player (team_id, player_id) SELECT team_id, player_id FROM resolved \
           ON CONFLICT (team_id, player_id) DO NOTHING) \
         DELETE FROM v2_team_player tp USING v2_team t \
         WHERE t.id = tp.team_id AND t.ext_id IN ({}) \
           AND NOT EXISTS (SELECT 1 FROM resolved r WHERE r.team_id = tp.team_id AND r.player_id = tp.player_id)",
        scope.join(", ")
    )
}

/// One statement per chunk of whole teams: each chunk is atomic for its own teams, there is no
/// transaction across chunks (Canyon has none)
async fn sync_team_players(rows: &ReferenceRows) -> Result<()> {
    // Postgres takes at most 65535 parameters per statement: chunks of whole teams
    let mut by_team: BTreeMap<i64, Vec<i64>> =
        rows.teams.iter().map(|t| (t.ext_id, vec![])).collect();
    for (team, player) in &rows.team_players {
        if let Some(players) = by_team.get_mut(team) {
            players.push(*player);
        }
    }
    let mut chunks: Vec<Vec<(i64, &Vec<i64>)>> = Vec::new();
    let mut used = 0;
    for (team, players) in &by_team {
        let cost = 2 * players.len() + 1;
        if chunks.is_empty() || used + cost > MAX_PARAMS {
            chunks.push(Vec::new());
            used = 0;
        }
        used += cost;
        chunks
            .last_mut()
            .expect("chunk just pushed")
            .push((*team, players));
    }
    for chunk in chunks {
        let teams: Vec<i64> = chunk.iter().map(|(t, _)| *t).collect();
        let pairs: Vec<(i64, i64)> = chunk
            .iter()
            .flat_map(|(t, ps)| ps.iter().map(move |p| (*t, *p)))
            .collect();
        let sql = team_players_sql(pairs.len(), teams.len());
        let mut params: Vec<&dyn QueryParameter> = Vec::new();
        for (team, player) in &pairs {
            params.push(team);
            params.push(player);
        }
        for team in &teams {
            params.push(team);
        }
        "".execute(&sql, &params).await.map_err(|e| eyre!(e))?;
    }
    Ok(())
}

/// `UPDATE … FROM (VALUES …)` for `teams` pairs of (ext_id, alt image), only rows that change
fn alt_images_sql(teams: usize) -> String {
    let values: Vec<String> = (0..teams)
        .map(|i| format!("(${}::bigint, ${}::text)", 2 * i + 1, 2 * i + 2))
        .collect();
    format!(
        "UPDATE v2_team t SET alt_image_url = v.alt FROM (VALUES {}) AS v (ext_id, alt) \
         WHERE t.ext_id = v.ext_id AND t.alt_image_url IS DISTINCT FROM v.alt",
        values.join(", ")
    )
}

/// The new API has no alternative image: it comes from the old API. Teams missing there keep theirs
pub async fn write_alt_images(alts: &[(i64, Option<String>)]) -> Result<()> {
    // one row per team, last one wins: `UPDATE … FROM` would pick any of the duplicates
    let alts: Vec<(i64, Option<String>)> = alts
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect();
    for chunk in alts.chunks(MAX_PARAMS / 2) {
        let mut params: Vec<&dyn QueryParameter> = Vec::with_capacity(chunk.len() * 2);
        for (team, alt) in chunk {
            params.push(team);
            params.push(alt);
        }
        "".execute(&alt_images_sql(chunk.len()), &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2_team alt images")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::v2::rows::*;
    use canyon_sql::connection::DbConnection;

    #[test]
    fn alt_images_sql_numbers_pairs() {
        let sql = alt_images_sql(2);
        assert!(sql.contains("VALUES ($1::bigint, $2::text), ($3::bigint, $4::text)"));
        assert!(sql.contains("IS DISTINCT FROM v.alt"));
    }

    fn rows(team_players: Vec<(i64, i64)>) -> ReferenceRows {
        ReferenceRows {
            leagues: vec![LeagueRow {
                ext_id: -101,
                slug: "test-league".into(),
                image_url: "i".into(),
                dark_image_url: None,
                texts: vec![
                    LeagueText {
                        locale: "en-US".into(),
                        name: "Test".into(),
                        region: "EMEA".into(),
                        position: Some(1),
                    },
                    LeagueText {
                        locale: "es-ES".into(),
                        name: "Prueba".into(),
                        region: "EMEA".into(),
                        position: Some(2),
                    },
                ],
            }],
            seasons: vec![SeasonRow {
                ext_id: -102,
                start_time: None,
                end_time: None,
                texts: vec![Text {
                    locale: "en-US".into(),
                    name: "2026".into(),
                }],
            }],
            splits: vec![-103],
            tournaments: vec![TournamentRow {
                ext_id: -104,
                league_ext_id: -101,
                season_ext_id: Some(-102),
                split_ext_id: Some(-103),
                slug: "test_2026".into(),
                start_time: None,
                end_time: None,
                texts: vec![Text {
                    locale: "en-US".into(),
                    name: "Summer".into(),
                }],
            }],
            stages: vec![StageRow {
                ext_id: -105,
                tournament_ext_id: -104,
                texts: vec![],
            }],
            teams: vec![TeamRow {
                ext_id: -106,
                slug: "t".into(),
                name: "T".into(),
                code: "T".into(),
                image_url: "i".into(),
                bg_image_url: None,
                home_league_ext_id: Some(-101),
            }],
            players: vec![
                PlayerRow {
                    ext_id: -107,
                    handle: "a".into(),
                    first_name: "A".into(),
                    last_name: "A".into(),
                    image_url: None,
                    role: "top".into(),
                },
                PlayerRow {
                    ext_id: -108,
                    handle: "b".into(),
                    first_name: "B".into(),
                    last_name: "B".into(),
                    image_url: None,
                    role: "mid".into(),
                },
            ],
            team_players,
        }
    }

    #[test]
    fn roster_sql_numbering() {
        let none = team_players_sql(0, 2);
        assert!(none.contains("WHERE false"));
        assert!(none.contains("IN ($1::bigint, $2::bigint)"));
        let some = team_players_sql(2, 1);
        assert!(some.contains("VALUES ($1::bigint, $2::bigint), ($3::bigint, $4::bigint)"));
        assert!(some.contains("IN ($5::bigint)"));
    }

    async fn count(sql: &str) -> i64 {
        "".query_one_for::<i64>(sql, &[]).await.unwrap()
    }

    async fn cleanup() {
        crate::dao::v2::test_cleanup().await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn writes_twice_without_duplicates() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        crate::dao::v2::schema::apply().await.unwrap();
        cleanup().await;
        write_reference(&rows(vec![(-106, -107), (-106, -108)]))
            .await
            .unwrap();
        let first_id = count("SELECT id::bigint FROM v2_tournament WHERE ext_id = -104").await;
        write_reference(&rows(vec![(-106, -107), (-106, -108)]))
            .await
            .unwrap();
        assert_eq!(
            count("SELECT id::bigint FROM v2_tournament WHERE ext_id = -104").await,
            first_id
        );
        assert_eq!(count("SELECT count(*) FROM v2_league_i18n i JOIN v2_league l ON l.id = i.league_id WHERE l.ext_id = -101").await, 2);
        assert_eq!(count("SELECT count(*) FROM v2_tournament t JOIN v2_league l ON l.id = t.league_id JOIN v2_season s ON s.id = t.season_id JOIN v2_split p ON p.id = t.split_id WHERE t.ext_id = -104 AND l.ext_id = -101 AND s.ext_id = -102 AND p.ext_id = -103").await, 1);
        assert_eq!(count("SELECT count(*) FROM v2_team t JOIN v2_league l ON l.id = t.home_league_id WHERE t.ext_id = -106").await, 1);
        assert_eq!(count("SELECT count(*) FROM v2_team_player tp JOIN v2_team t ON t.id = tp.team_id WHERE t.ext_id = -106").await, 2);
        // a player leaves the roster: the pair goes away, the table never gets emptied first
        write_reference(&rows(vec![(-106, -107)])).await.unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_team_player tp JOIN v2_team t ON t.id = tp.team_id WHERE t.ext_id = -106").await, 1);
        cleanup().await;
    }
}
