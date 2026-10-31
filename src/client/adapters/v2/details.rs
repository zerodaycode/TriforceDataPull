//! Feed answers (and `watchVodQuery` when there's no feed) → game details rows

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use color_eyre::{Result, eyre::eyre};

use crate::{
    client::{
        feed::models::{Details, GameMetadata, PlayerFrame, TeamFrame, TeamMeta, Window},
        gql::ops::watch_vod::VodMatchEvent,
    },
    dao::v2::rows::*,
};

use crate::client::adapters::ext_id as id;

/// The feed's summoner name starts with the team code (`"FUR Guigo"`): the handle is the rest
pub fn handle(summoner: &str, code: &str) -> String {
    summoner
        .strip_prefix(code)
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|rest| !rest.trim().is_empty())
        .unwrap_or(summoner)
        .to_string()
}

/// Without the team code: the first word when it looks like one (`"ATA Niki"` → `"Niki"`)
fn handle_any(summoner: &str) -> String {
    match summoner.split_once(' ') {
        Some((tag, rest))
            if (2..=5).contains(&tag.len())
                && tag
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && !rest.trim().is_empty() =>
        {
            rest.to_string()
        }
        _ => summoner.to_string(),
    }
}

fn list<T: ToString>(items: &[T]) -> Option<String> {
    (!items.is_empty()).then(|| {
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    })
}

fn naive(t: DateTime<Utc>) -> chrono::NaiveDateTime {
    t.naive_utc()
}

/// The match's team for blue and red, and (team in the answer, match team) when one was taken
pub(crate) type Matched = ([i64; 2], Option<(i64, i64)>);

/// The match's team for each feed side. When only one feed team is in the match, the other
/// one takes the match's remaining team: the schedule is what results and standings use (LIT
/// 2026: the game ran under Aeterna's id, the match lists TEAM AQUA)
pub(crate) fn match_teams(game: i64, feed: [i64; 2], event_teams: &[i64]) -> Result<Matched> {
    let known = |t: i64| event_teams.contains(&t);
    match (known(feed[0]), known(feed[1])) {
        (true, true) => Ok((feed, None)),
        (true, false) | (false, true) => {
            let (kept, unknown) = if known(feed[0]) { (0, 1) } else { (1, 0) };
            let others: Vec<i64> = event_teams
                .iter()
                .copied()
                .filter(|t| *t != feed[kept])
                .collect();
            let [other] = others[..] else {
                return Err(eyre!(
                    "game {game}: feed team {} is not in the match",
                    feed[unknown]
                ));
            };
            let mut ids = feed;
            ids[unknown] = other;
            Ok((ids, Some((feed[unknown], other))))
        }
        (false, false) => Err(eyre!(
            "game {game}: feed teams {} and {} are not in the match",
            feed[0],
            feed[1]
        )),
    }
}

/// Both windows are of `game`, and the game's metadata (from the first one, else the latest)
fn checked_meta<'a>(game: i64, first: &'a Window, last: &'a Window) -> Result<&'a GameMetadata> {
    for w in [first, last] {
        if id(&w.esports_game_id)? != game {
            return Err(eyre!(
                "feed answered game {} for game {game}",
                w.esports_game_id
            ));
        }
    }
    first
        .game_metadata
        .as_ref()
        .or(last.game_metadata.as_ref())
        .ok_or_else(|| eyre!("game {game}: feed without metadata"))
}

/// The match's team for the feed's blue and red team (`match_teams`), never the same twice
fn feed_teams(game: i64, meta: &GameMetadata, event_teams: &[i64]) -> Result<Matched> {
    let feed = [
        id(&meta.blue_team_metadata.esports_team_id)?,
        id(&meta.red_team_metadata.esports_team_id)?,
    ];
    let (ids, remapped) = match_teams(game, feed, event_teams)?;
    if ids[0] == ids[1] {
        return Err(eyre!("game {game}: feed has the same team on both sides"));
    }
    Ok((ids, remapped))
}

/// A team's totals in one frame
fn team_row(team_ext_id: i64, side: &'static str, total: &TeamFrame) -> GameTeamRow {
    GameTeamRow {
        team_ext_id,
        side,
        gold: total.total_gold,
        kills: total.total_kills,
        towers: total.towers,
        inhibitors: total.inhibitors,
        barons: total.barons,
        dragons: list(&total.dragons),
    }
}

/// `first`: window with no time (metadata, first frames). `last` and `details`: asked for a
/// time after the end (last frames). `event_teams`: (ext_id, code) of the match's teams. One feed
/// team not in the match takes the match's other team (`remapped`); both unknown, or no other
/// team to take (TBD slot), is an error and nothing gets written
pub fn from_feed(
    game_ext_id: i64,
    event_teams: &[(i64, String)],
    first: &Window,
    last: &Window,
    details: &Details,
) -> Result<GameDetailsRow> {
    let meta = checked_meta(game_ext_id, first, last)?;
    let code = |team: i64| {
        event_teams
            .iter()
            .find(|(t, _)| *t == team)
            .map(|(_, c)| c.as_str())
    };
    let sides: [(&'static str, &TeamMeta); 2] = [
        ("blue", &meta.blue_team_metadata),
        ("red", &meta.red_team_metadata),
    ];
    let ids: Vec<i64> = event_teams.iter().map(|(t, _)| *t).collect();
    let (team_ids, remapped) = feed_teams(game_ext_id, meta, &ids)?;
    // 5 players a side, participant ids 1-10 once each: anything else is a broken answer
    let mut seen = std::collections::HashSet::new();
    for (side, t) in &sides {
        if t.participant_metadata.len() != 5 {
            return Err(eyre!(
                "game {game_ext_id}: feed {side} has {} players",
                t.participant_metadata.len()
            ));
        }
        for p in &t.participant_metadata {
            if !(1..=10).contains(&p.participant_id) || !seen.insert(p.participant_id) {
                return Err(eyre!(
                    "game {game_ext_id}: feed participant id {} out of place",
                    p.participant_id
                ));
            }
        }
    }
    let frame = last
        .frames
        .last()
        .ok_or_else(|| eyre!("game {game_ext_id}: feed without frames"))?;
    // no stats frame: a try later, never `done` with every stat empty
    let stats: HashMap<i32, &PlayerFrame> = details
        .frames
        .last()
        .ok_or_else(|| eyre!("game {game_ext_id}: details without frames"))?
        .participants
        .iter()
        .map(|p| (p.participant_id, p))
        .collect();
    if let Some(missing) = seen.iter().find(|id| !stats.contains_key(id)) {
        return Err(eyre!(
            "game {game_ext_id}: details without stats for participant {missing}"
        ));
    }
    let totals = [&frame.blue_team, &frame.red_team];
    let mut teams = Vec::with_capacity(2);
    let mut players = Vec::with_capacity(10);
    let mut player_stubs = Vec::with_capacity(10);
    for (((side, t), team), total) in sides.iter().zip(&team_ids).zip(totals) {
        teams.push(team_row(*team, side, total));
        for p in &t.participant_metadata {
            // a player the feed has no id for (missing or "-") keeps the slot, without a player row
            let player = p.esports_player_id.as_deref().and_then(|x| id(x).ok());
            if let Some(ext) = player {
                player_stubs.push(PlayerStub {
                    ext_id: ext,
                    handle: if remapped.is_some_and(|(_, to)| to == *team) {
                        handle_any(&p.summoner_name)
                    } else {
                        handle(&p.summoner_name, code(*team).unwrap_or(""))
                    },
                    role: p.role.clone(),
                });
            }
            let s = stats.get(&p.participant_id);
            let perks = s.and_then(|s| s.perk_metadata.as_ref());
            players.push(GamePlayerRow {
                team_ext_id: *team,
                player_ext_id: player,
                summoner_name: p.summoner_name.clone(),
                participant_id: p.participant_id,
                role: p.role.clone(),
                champion: p.champion_id.clone(),
                level: s.and_then(|s| s.level),
                kills: s.and_then(|s| s.kills),
                deaths: s.and_then(|s| s.deaths),
                assists: s.and_then(|s| s.assists),
                gold: s.and_then(|s| s.total_gold_earned),
                cs: s.and_then(|s| s.creep_score),
                wards_placed: s.and_then(|s| s.wards_placed),
                wards_destroyed: s.and_then(|s| s.wards_destroyed),
                damage_share_bp: s
                    .and_then(|s| s.champion_damage_share)
                    .filter(|d| d.is_finite())
                    // a share is 0-1: 0-10000 always fits an i32
                    .map(|d| (d * 10_000.0).round() as i32),
                items: s.and_then(|s| list(&s.items)),
                rune_style: perks.and_then(|p| p.style_id),
                rune_sub_style: perks.and_then(|p| p.sub_style_id),
                runes: perks.and_then(|p| list(&p.perks)),
                skill_order: s.and_then(|s| list(&s.abilities)),
            });
        }
    }
    Ok(GameDetailsRow {
        game_ext_id,
        patch: meta.patch_version.clone(),
        start_time: first.frames.first().map(|f| naive(f.rfc460_timestamp)),
        end_time: Some(naive(frame.rfc460_timestamp)),
        teams,
        players,
        player_stubs,
        remapped,
    })
}

/// A game being played, from its first and its latest feed window: when it started, when it
/// ended (`finished` in the latest frame), the sides and each team's totals right now
pub fn from_live_feed(
    game_ext_id: i64,
    event_teams: &[i64],
    first: &Window,
    last: &Window,
) -> Result<LiveGameRow> {
    let meta = checked_meta(game_ext_id, first, last)?;
    let (team_ids, _) = feed_teams(game_ext_id, meta, event_teams)?;
    let start = first
        .frames
        .first()
        .ok_or_else(|| eyre!("game {game_ext_id}: feed without frames"))?;
    let frame = last.frames.last().unwrap_or(start);
    let totals = [&frame.blue_team, &frame.red_team];
    let teams = ["blue", "red"]
        .into_iter()
        .zip(team_ids)
        .zip(totals)
        .map(|((side, team), total)| team_row(team, side, total))
        .collect();
    Ok(LiveGameRow {
        game_ext_id,
        start_time: start.rfc460_timestamp.naive_utc(),
        end_time: (frame.game_state.as_deref() == Some("finished"))
            .then(|| frame.rfc460_timestamp.naive_utc()),
        teams,
    })
}

/// Sides of the event's completed games, for games the feed has nothing for. A game without
/// one blue and one red team is left out
pub fn from_vod(event: &VodMatchEvent) -> Vec<GameSides> {
    event
        .r#match
        .games
        .iter()
        .filter(|g| g.state == "completed")
        .filter_map(|g| super::game_sides(&g.id, &g.teams))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{
        ops::watch_vod::{GetWatchVod, VodEvent},
        parse_body,
    };

    const GAME: i64 = 115660588583171797;
    const FUR: i64 = 100205576309502431;
    const C9: i64 = 98767991877340524;

    fn feed() -> (Window, Window, Details) {
        (
            serde_json::from_str(fixture!("feed/window_first.json")).unwrap(),
            serde_json::from_str(fixture!("feed/window_last.json")).unwrap(),
            serde_json::from_str(fixture!("feed/details_last.json")).unwrap(),
        )
    }

    fn teams() -> Vec<(i64, String)> {
        vec![(FUR, "FUR".into()), (C9, "C9".into())]
    }

    #[test]
    fn live_game_from_the_feed() {
        let (first, last, _) = feed();
        // finished: start from the first frame, end from the last one, totals right now
        let row = from_live_feed(GAME, &[FUR, C9], &first, &last).unwrap();
        assert_eq!(row.start_time, first.frames[0].rfc460_timestamp.naive_utc());
        let end = last.frames.last().unwrap();
        assert_eq!(row.end_time, Some(end.rfc460_timestamp.naive_utc()));
        assert_eq!(row.teams.len(), 2);
        assert_eq!(row.teams[0].side, "blue");
        assert_eq!(row.teams[0].gold, end.blue_team.total_gold);
        assert_eq!(row.teams[1].kills, end.red_team.total_kills);
        let sides: Vec<i64> = row.teams.iter().map(|t| t.team_ext_id).collect();
        assert!(sides.contains(&FUR) && sides.contains(&C9));
        // still playing (the latest window is the first one): no end yet
        let row = from_live_feed(GAME, &[FUR, C9], &first, &first).unwrap();
        assert_eq!(row.end_time, None);
        // the latest window without frames: the first frame's totals, no end
        let mut empty = feed().1;
        empty.frames.clear();
        let row = from_live_feed(GAME, &[FUR, C9], &first, &empty).unwrap();
        assert_eq!(
            (row.end_time, row.teams[0].gold),
            (None, first.frames[0].blue_team.total_gold)
        );
        // paused: not the end
        let mut paused = feed().1;
        paused.frames.last_mut().unwrap().game_state = Some("paused".into());
        assert_eq!(
            from_live_feed(GAME, &[FUR, C9], &first, &paused)
                .unwrap()
                .end_time,
            None
        );
        // another game's feed, or teams that aren't the match's
        assert!(from_live_feed(GAME + 1, &[FUR, C9], &first, &last).is_err());
        assert!(from_live_feed(GAME, &[-1, -2], &first, &last).is_err());
    }

    #[test]
    fn game_3_of_fur_c9() {
        let (first, last, details) = feed();
        let row = from_feed(GAME, &teams(), &first, &last, &details).unwrap();
        assert_eq!(row.patch.as_deref(), Some("16.20.826.3891"));
        assert_eq!(
            row.start_time.unwrap().to_string(),
            "2026-10-16 22:47:31.641"
        );
        assert_eq!(row.end_time.unwrap().to_string(), "2026-10-16 23:21:44.779");
        let blue = &row.teams[0];
        assert_eq!((blue.team_ext_id, blue.side), (FUR, "blue"));
        assert_eq!(
            (blue.kills, blue.towers, blue.inhibitors, blue.barons),
            (Some(14), Some(10), Some(2), Some(2))
        );
        assert_eq!(blue.dragons.as_deref(), Some("infernal,infernal"));
        assert_eq!((row.teams[1].team_ext_id, row.teams[1].side), (C9, "red"));
        assert_eq!(row.players.len(), 10);
        let p1 = &row.players[0];
        assert_eq!(
            (
                p1.participant_id,
                p1.team_ext_id,
                p1.role.as_str(),
                p1.champion.as_str()
            ),
            (1, FUR, "top", "Volibear")
        );
        assert_eq!(
            (p1.kills, p1.deaths, p1.assists),
            (Some(2), Some(1), Some(8))
        );
        assert_eq!(p1.damage_share_bp, Some(2144));
        assert_eq!(p1.items.as_deref(), Some("1086,3158,3364,2502,3065,3053"));
        assert_eq!((p1.rune_style, p1.rune_sub_style), (Some(8400), Some(8300)));
        assert!(p1.skill_order.as_deref().unwrap().starts_with("W,E,Q"));
        assert_eq!(row.players[5].team_ext_id, C9);
        assert_eq!(row.player_stubs[0].handle, "Guigo");
        assert_eq!(row.player_stubs[5].handle, "Thanatos");
    }

    #[test]
    fn player_without_id_keeps_the_slot() {
        let (first, last, details) = feed();
        let mut first = first;
        first
            .game_metadata
            .as_mut()
            .unwrap()
            .red_team_metadata
            .participant_metadata[4]
            .esports_player_id = Some("-".into());
        first
            .game_metadata
            .as_mut()
            .unwrap()
            .red_team_metadata
            .participant_metadata[3]
            .esports_player_id = None;
        let row = from_feed(GAME, &teams(), &first, &last, &details).unwrap();
        assert_eq!(row.players.len(), 10);
        assert_eq!(row.players[9].player_ext_id, None);
        assert_eq!(row.players[8].player_ext_id, None);
        assert!(row.players[9].summoner_name.starts_with("C9 "));
        assert_eq!(row.player_stubs.len(), 8);
    }

    #[test]
    fn one_feed_team_not_in_the_match_takes_the_other_slot() {
        let (first, last, details) = feed();
        // the match lists TA where the feed has C9
        let event = vec![(FUR, "FUR".to_string()), (1, "TA".to_string())];
        let row = from_feed(GAME, &event, &first, &last, &details).unwrap();
        assert_eq!(row.remapped, Some((C9, 1)));
        assert_eq!((row.teams[1].team_ext_id, row.teams[1].side), (1, "red"));
        assert!(row.players[5..].iter().all(|p| p.team_ext_id == 1));
        assert_eq!(row.player_stubs[5].handle, "Thanatos");
        assert_eq!(row.players[0].team_ext_id, FUR);
    }

    #[test]
    fn match_teams_edges() {
        let ev = |v: &[i64]| v.to_vec();
        assert_eq!(
            match_teams(1, [2, 3], &ev(&[3, 2])).unwrap(),
            ([2, 3], None)
        );
        // unknown on blue
        assert_eq!(
            match_teams(1, [9, 2], &ev(&[2, 3])).unwrap(),
            ([3, 2], Some((9, 3)))
        );
        // the other slot is TBD: nothing to take
        assert!(match_teams(1, [2, 9], &ev(&[2])).is_err());
        // the same team twice in the match
        assert!(match_teams(1, [2, 9], &ev(&[2, 2])).is_err());
        assert!(match_teams(1, [2, 9], &ev(&[])).is_err());
        assert!(match_teams(1, [8, 9], &ev(&[2, 3])).is_err());
    }

    #[test]
    fn handles_without_a_code() {
        assert_eq!(handle_any("ATA Niki"), "Niki");
        assert_eq!(handle_any("Faker"), "Faker");
        assert_eq!(handle_any("Lee Sang"), "Lee Sang");
        assert_eq!(handle_any("T1 "), "T1 ");
    }

    #[test]
    fn feed_with_other_teams_is_an_error() {
        let (first, last, details) = feed();
        let other = vec![(1, "X".into()), (2, "Y".into())];
        assert!(from_feed(GAME, &other, &first, &last, &details).is_err());
        assert!(from_feed(GAME + 1, &teams(), &first, &last, &details).is_err());
    }

    #[test]
    fn broken_answers_are_errors() {
        let (first, last, details) = feed();
        // no stats frame
        assert!(from_feed(GAME, &teams(), &first, &last, &Details { frames: vec![] }).is_err());
        // a stats frame without the players
        let empty: Details = serde_json::from_str(r#"{"frames":[{"participants":null}]}"#).unwrap();
        assert!(from_feed(GAME, &teams(), &first, &last, &empty).is_err());
        // the same team on both sides
        let (mut f, _, _) = feed();
        let m = f.game_metadata.as_mut().unwrap();
        m.red_team_metadata.esports_team_id = m.blue_team_metadata.esports_team_id.clone();
        assert!(from_feed(GAME, &teams(), &f, &last, &details).is_err());
        // 4 players on a side
        let (mut f, _, _) = feed();
        f.game_metadata
            .as_mut()
            .unwrap()
            .blue_team_metadata
            .participant_metadata
            .pop();
        assert!(from_feed(GAME, &teams(), &f, &last, &details).is_err());
        // a participant id twice
        let (mut f, _, _) = feed();
        f.game_metadata
            .as_mut()
            .unwrap()
            .red_team_metadata
            .participant_metadata[0]
            .participant_id = 1;
        assert!(from_feed(GAME, &teams(), &f, &last, &details).is_err());
    }

    #[test]
    fn vod_game_without_one_blue_one_red_is_left_out() {
        let body = r#"{"data":{"scheduledEvent":{"__typename":"EventMatch","match":{"games":[{"id":"2","state":"completed","teams":[{"id":"1","side":"blue"},{"id":"3","side":"blue"}]}]}}}}"#;
        let Some(VodEvent::Match(e)) = parse_body::<GetWatchVod>(200, body)
            .unwrap()
            .scheduled_event
        else {
            panic!("a match")
        };
        assert!(from_vod(&e).is_empty());
        // the same team on both sides, or a side not picked (`null`)
        for teams in [
            r#"[{"id":"1","side":"blue"},{"id":"1","side":"red"}]"#,
            r#"[{"id":"1","side":"blue"},{"id":"3","side":null}]"#,
        ] {
            let body = body.replace(
                r#"[{"id":"1","side":"blue"},{"id":"3","side":"blue"}]"#,
                teams,
            );
            let Some(VodEvent::Match(e)) = parse_body::<GetWatchVod>(200, &body)
                .unwrap()
                .scheduled_event
            else {
                panic!("a match")
            };
            assert!(from_vod(&e).is_empty());
        }
    }

    #[test]
    fn handles() {
        assert_eq!(handle("C9 Thanatos", "C9"), "Thanatos");
        assert_eq!(handle("Faker", "T1"), "Faker");
        assert_eq!(handle("T1", "T1"), "T1");
        assert_eq!(handle("T1Faker", "T1"), "T1Faker");
    }

    #[test]
    fn vod_sides_of_completed_games() {
        let data = parse_body::<GetWatchVod>(200, fixture!("gql/watch_vod.json")).unwrap();
        let Some(VodEvent::Match(e)) = data.scheduled_event else {
            panic!("a match")
        };
        let sides = from_vod(&e);
        // game 5 was unneeded
        assert_eq!(sides.len(), 4);
        assert_eq!(sides[1].game_ext_id, 115660588583171796);
        assert_eq!(sides[1].sides, [(C9, "blue"), (FUR, "red")]);
    }
}
