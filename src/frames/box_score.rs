//! What the game details store (the box score), from the last point of a game's timeline. The
//! frames job writes it for games with a chronobreak: there the feed's last frame isn't the box
//! score (its counts start again at 0 after the restore)

use canyon_sql::date_time::NaiveDateTime;
use color_eyre::{Result, eyre::eyre};
use serde::Serialize;

use super::timeline::{Side, Timeline};

#[derive(Debug, Clone, PartialEq)]
pub struct BoxScore {
    /// The last frame
    pub end_time: NaiveDateTime,
    /// A pause with a rollback or with the feed's counts started again
    pub chronobreak: bool,
    /// blue, red
    pub teams: [BoxTeam; 2],
    /// participants 1 to 10
    pub players: Vec<BoxPlayer>,
}

/// Column names as in `v2_game_team`
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BoxTeam {
    pub side: &'static str,
    pub gold: Option<i64>,
    pub kills: Option<i64>,
    pub towers: Option<i64>,
    pub inhibitors: Option<i64>,
    pub barons: Option<i64>,
    /// `infernal,ocean`
    pub dragons: Option<String>,
}

/// Column names as in `v2_game_player`
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BoxPlayer {
    pub participant_id: i16,
    pub level: Option<i64>,
    pub kills: Option<i64>,
    pub deaths: Option<i64>,
    pub assists: Option<i64>,
    pub gold: Option<i64>,
    pub cs: Option<i64>,
    pub wards_placed: Option<i64>,
    pub wards_destroyed: Option<i64>,
    pub damage_share_bp: Option<i64>,
    /// `None` when the feed's item list isn't the inventory (after a restart)
    pub items: Option<String>,
    pub rune_style: Option<i64>,
    pub rune_sub_style: Option<i64>,
    pub runes: Option<String>,
    pub skill_order: Option<String>,
}

fn last<T: Copy>(v: &[Option<T>]) -> Option<T> {
    v.last().copied().flatten()
}

/// `a,b,c`, `None` when empty (like the game details)
fn list<T: ToString>(items: impl IntoIterator<Item = T>) -> Option<String> {
    let v: Vec<String> = items.into_iter().map(|i| i.to_string()).collect();
    (!v.is_empty()).then(|| v.join(","))
}

pub fn box_score(tl: &Timeline) -> Result<BoxScore> {
    let c = &tl.chunk;
    let end_time = *c
        .wall_time
        .last()
        .ok_or_else(|| eyre!("timeline without points"))?;
    let team = |i: usize, side: Side, name: &'static str| {
        let t = &c.teams[i];
        let mut dragons: Vec<_> = c
            .markers
            .iter()
            .filter(|m| m.kind == "dragon" && m.side == side && m.undone_by_pause.is_none())
            .collect();
        dragons.sort_by_key(|m| m.game_ms);
        BoxTeam {
            side: name,
            gold: last(&t.gold),
            kills: last(&t.kills),
            towers: last(&t.towers),
            inhibitors: last(&t.inhibitors),
            barons: last(&t.barons),
            dragons: list(dragons.iter().filter_map(|m| m.detail.as_deref())),
        }
    };
    let players = c
        .players
        .iter()
        .map(|p| {
            // the items of the last point are the last `count` of the flat list
            let items = last(&p.item_counts).and_then(|n| {
                let n = usize::try_from(n).ok()?;
                let from = p.items.len().checked_sub(n)?;
                list(p.items[from..].iter().flatten())
            });
            let runes = c
                .runes
                .iter()
                .find(|r| r.participant_id == p.participant_id);
            BoxPlayer {
                participant_id: p.participant_id,
                level: last(&p.level),
                kills: last(&p.kills),
                deaths: last(&p.deaths),
                assists: last(&p.assists),
                gold: last(&p.gold_earned),
                cs: last(&p.cs),
                wards_placed: last(&p.wards_placed),
                wards_destroyed: last(&p.wards_destroyed),
                // a share is 0-1: 0-10000 always fits
                damage_share_bp: last(&p.damage_share).map(|d| (d * 10_000.0).round() as i64),
                items,
                rune_style: runes.and_then(|r| r.style),
                rune_sub_style: runes.and_then(|r| r.sub_style),
                runes: runes.and_then(|r| list(&r.perks)),
                skill_order: list(
                    c.skills
                        .iter()
                        .filter(|s| s.participant_id == p.participant_id)
                        .map(|s| &s.skill),
                ),
            }
        })
        .collect();
    Ok(BoxScore {
        end_time,
        chronobreak: c
            .pauses
            .iter()
            .any(|p| p.rollback_s.is_some() || p.counters_reset),
        teams: [team(0, Side::Blue, "blue"), team(1, Side::Red, "red")],
        players,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{hand, parse_frames, timeline::build};

    fn raws(v: &[serde_json::Value]) -> Vec<crate::frames::RawFrame> {
        v.iter().map(hand::raw).collect()
    }

    /// No pause: the last frame's numbers, items listed, no chronobreak
    #[test]
    fn last_point_of_a_normal_game() {
        let (mut w, d) = hand::game(95, true);
        for f in &mut w[31..] {
            f["redTeam"]["dragons"] = serde_json::json!(["ocean"]);
        }
        let tl = build(&raws(&w), &raws(&d), 10).unwrap();
        let b = box_score(&tl).unwrap();
        assert!(!b.chronobreak);
        assert_eq!(b.end_time, *tl.chunk.wall_time.last().unwrap());
        assert_eq!(b.teams[0].side, "blue");
        assert_eq!(b.teams[0].gold, *tl.chunk.teams[0].gold.last().unwrap());
        assert_eq!(b.teams[1].dragons.as_deref(), Some("ocean"));
        assert_eq!(b.teams[0].dragons, None);
        assert_eq!(b.players.len(), 10);
        let p = &b.players[0];
        assert_eq!(p.participant_id, 1);
        assert_eq!(p.gold, *tl.chunk.players[0].gold_earned.last().unwrap());
        assert!(p.items.is_some());
    }

    /// G2-MKOI 2025-09-14 g3 (trimmed): the undone kill is out, items unknown after the restart
    #[test]
    fn real_chronobreak() {
        let w = parse_frames(fixture!("feed/frames/chronobreak_window.json"))
            .unwrap()
            .1;
        let d = parse_frames(fixture!("feed/frames/chronobreak_details.json"))
            .unwrap()
            .1;
        let tl = build(&w, &d, 10).unwrap();
        let b = box_score(&tl).unwrap();
        assert!(b.chronobreak);
        assert_eq!(b.teams[0].kills, Some(4));
        assert_eq!(b.teams[0].towers, Some(3));
        assert!(b.players.iter().all(|p| p.items.is_none()));
        // skills from before the pause are kept
        assert!(
            b.players[0]
                .skill_order
                .as_deref()
                .is_some_and(|s| s.contains(','))
        );
    }

    /// A dragon taken back by the chronobreak isn't in the list
    #[test]
    fn undone_dragon_left_out() {
        let (mut w, d) = hand::game(95, true);
        for f in &mut w[31..] {
            f["redTeam"]["dragons"] = serde_json::json!(["ocean"]);
        }
        let mut tl = build(&raws(&w), &raws(&d), 10).unwrap();
        tl.chunk
            .markers
            .iter_mut()
            .filter(|m| m.kind == "dragon")
            .for_each(|m| m.undone_by_pause = Some(1));
        assert_eq!(box_score(&tl).unwrap().teams[1].dragons, None);
    }
}
