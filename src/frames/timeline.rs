//! Frames → one point every `step_s` of game time, plus markers (dragons, barons, towers,
//! inhibitors), pauses and chronobreaks. The same builder takes a whole game (after it ends,
//! from the archive) or 10 s at a time (live); its state is saved between live ticks, so both
//! give the same points, except after a chronobreak: only the archive keeps the frames to go
//! back to the snapshot.
//!
//! Game time: wall time between two `window` frames counts only while the earlier one is in
//! game (`paused` doesn't count, `finished` is the end). Point k = the last frame with game
//! time ≤ k × step. A gap of more than `MAX_CARRY` between two frames leaves the points inside
//! it empty

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use serde::{Deserialize, Serialize};

use super::RawFrame;

pub const STEP_S: i32 = 10;
/// Longer gaps between frames give empty points instead of the last value
const MAX_CARRY: TimeDelta = TimeDelta::seconds(30);
/// No game is this long in wall time, pauses included (2:58 the longest seen, DK-DRX 2024-02-25
/// game 2): a frame further from the first one is a feed error (and would make millions of points)
const MAX_WALL: TimeDelta = TimeDelta::hours(6);
/// Before this much game time, frames with everything at 0 are the game starting, not a server
/// starting again after a chronobreak
const RESTART_AFTER_MS: i64 = 60_000;
/// A chronobreak goes back seconds (5 and 8 s seen): the snapshot is searched this far back in
/// game time, and the archive build keeps only these frames
const MAX_ROLLBACK_MS: i64 = 5 * 60_000;
/// More objectives of one kind between two frames is a feed error, no markers for it
const MAX_JUMP: i64 = 11;

// The feed's frames, only what the series use. Everything optional: a field missing in one
// frame is an empty value, not a failed game

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WindowFrame {
    rfc460_timestamp: DateTime<Utc>,
    game_state: Option<String>,
    #[serde(default)]
    blue_team: TeamFrame,
    #[serde(default)]
    red_team: TeamFrame,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TeamFrame {
    total_gold: Option<i64>,
    inhibitors: Option<i64>,
    towers: Option<i64>,
    barons: Option<i64>,
    total_kills: Option<i64>,
    /// `None` when the frame has no list: unknown, not "no dragons"
    #[serde(default)]
    dragons: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    participants: Vec<WindowPlayer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WindowPlayer {
    participant_id: i64,
    total_gold: Option<i64>,
    level: Option<i64>,
    kills: Option<i64>,
    deaths: Option<i64>,
    assists: Option<i64>,
    creep_score: Option<i64>,
    current_health: Option<i64>,
    max_health: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct DetailsFrame {
    rfc460_timestamp: DateTime<Utc>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    participants: Vec<DetailsPlayer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct DetailsPlayer {
    participant_id: i64,
    total_gold_earned: Option<i64>,
    kill_participation: Option<f64>,
    champion_damage_share: Option<f64>,
    wards_placed: Option<i64>,
    wards_destroyed: Option<i64>,
    attack_damage: Option<i64>,
    ability_power: Option<i64>,
    critical_chance: Option<f64>,
    attack_speed: Option<i64>,
    life_steal: Option<i64>,
    armor: Option<i64>,
    magic_resistance: Option<i64>,
    tenacity: Option<f64>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    items: Vec<i64>,
    /// After a restart the feed lists only what was bought since: the items aren't known
    #[serde(default)]
    items_unknown: bool,
    perk_metadata: Option<Perks>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    abilities: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Perks {
    style_id: Option<i64>,
    sub_style_id: Option<i64>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    perks: Vec<i64>,
}

// What a build adds: one value per point in each column, column names as in the tables

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TeamCols {
    pub gold: Vec<Option<i64>>,
    pub kills: Vec<Option<i64>>,
    pub towers: Vec<Option<i64>>,
    pub inhibitors: Vec<Option<i64>>,
    pub barons: Vec<Option<i64>>,
    pub dragons: Vec<Option<i64>>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct PlayerCols {
    pub participant_id: i16,
    pub gold: Vec<Option<i64>>,
    pub level: Vec<Option<i64>>,
    pub kills: Vec<Option<i64>>,
    pub deaths: Vec<Option<i64>>,
    pub assists: Vec<Option<i64>>,
    pub cs: Vec<Option<i64>>,
    pub hp: Vec<Option<i64>>,
    pub max_hp: Vec<Option<i64>>,
    pub gold_earned: Vec<Option<i64>>,
    pub kill_participation: Vec<Option<f64>>,
    pub damage_share: Vec<Option<f64>>,
    pub wards_placed: Vec<Option<i64>>,
    pub wards_destroyed: Vec<Option<i64>>,
    pub attack_damage: Vec<Option<i64>>,
    pub ability_power: Vec<Option<i64>>,
    pub crit_chance: Vec<Option<f64>>,
    pub attack_speed: Vec<Option<i64>>,
    pub life_steal: Vec<Option<i64>>,
    pub armor: Vec<Option<i64>>,
    pub magic_resist: Vec<Option<i64>>,
    pub tenacity: Vec<Option<f64>>,
    /// The items of every point one after the other, as many as the feed lists (one per unit
    /// of a stack: 11 seen). `item_counts` says how many belong to each point
    pub items: Vec<Option<i64>>,
    pub item_counts: Vec<Option<i64>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Blue,
    Red,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Marker {
    pub game_time_s: i64,
    /// The same in ms, to compare with a chronobreak's snapshot
    #[serde(skip)]
    pub game_ms: i64,
    pub kind: &'static str,
    pub side: Side,
    pub detail: Option<String>,
    /// Taken back by the chronobreak at the end of this pause
    pub undone_by_pause: Option<i16>,
}

/// A pause of the game: from the first `paused` frame to the next frame in play
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Pause {
    /// 1, 2, … in the order of the game
    pub number: i16,
    pub game_time_s: i64,
    pub started_at: DateTime<Utc>,
    /// `None` while the game is still paused
    pub ended_at: Option<DateTime<Utc>>,
    /// Chronobreak: game seconds the game went back when it came back. Only after the game
    /// (live doesn't keep the frames to find where it went back to)
    #[serde(default)]
    pub rollback_s: Option<i64>,
    /// The feed started its counts (kills, towers, dragons, wards, skills…) again at 0
    #[serde(default)]
    pub counters_reset: bool,
    /// What the chronobreak took back, with `rollback_s`
    #[serde(default)]
    pub undone: Option<Undone>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Undone {
    /// blue, red
    pub teams: [TeamUndone; 2],
    /// participants 1 to 10
    pub players: Vec<PlayerUndone>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TeamUndone {
    pub kills: i64,
    pub gold: i64,
    pub towers: i64,
    pub inhibitors: i64,
    pub barons: i64,
    pub dragons: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlayerUndone {
    pub participant_id: i16,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub gold: i64,
    pub cs: i64,
}

/// What the feed lost when it started its counts again (a chronobreak), added back to every
/// frame after it so the series keep adding up
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Offsets {
    /// blue, red
    team: [TeamOffset; 2],
    dragons: [Vec<String>; 2],
    /// participant id → kills, deaths, assists
    player: HashMap<i64, [i64; 3]>,
    /// participant id → wards placed, wards destroyed
    wards: HashMap<i64, [i64; 2]>,
    abilities: HashMap<i64, Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
struct TeamOffset {
    kills: i64,
    towers: i64,
    inhibitors: i64,
    barons: i64,
}

fn add(v: &mut Option<i64>, off: i64) {
    if let Some(x) = v {
        *x += off;
    }
}

/// `true` when the feed restarted this count: 0 now, more before
fn restarted(now: Option<i64>, before: Option<i64>) -> bool {
    now == Some(0) && before.is_some_and(|b| b > 0)
}

impl Offsets {
    fn window(&self, w: &mut WindowFrame) {
        for (i, t) in [&mut w.blue_team, &mut w.red_team].into_iter().enumerate() {
            let o = self.team[i];
            add(&mut t.total_kills, o.kills);
            add(&mut t.towers, o.towers);
            add(&mut t.inhibitors, o.inhibitors);
            add(&mut t.barons, o.barons);
            if !self.dragons[i].is_empty()
                && let Some(d) = &mut t.dragons
            {
                let mut all = self.dragons[i].clone();
                all.append(d);
                *d = all;
            }
            for p in &mut t.participants {
                if let Some(o) = self.player.get(&p.participant_id) {
                    add(&mut p.kills, o[0]);
                    add(&mut p.deaths, o[1]);
                    add(&mut p.assists, o[2]);
                }
            }
        }
    }

    fn details(&self, d: &mut DetailsFrame) {
        for p in &mut d.participants {
            if let Some(o) = self.wards.get(&p.participant_id) {
                add(&mut p.wards_placed, o[0]);
                add(&mut p.wards_destroyed, o[1]);
            }
            if let Some(a) = self.abilities.get(&p.participant_id) {
                let mut all = a.clone();
                all.append(&mut p.abilities);
                p.abilities = all;
            }
        }
    }

    /// The counts `raw` / `raw_d` (the first frames after the restart, as the feed sent them)
    /// restarted from `snap` / `snap_d` (the frames the game went back to, already with the old
    /// offsets) become offsets. `true` if any did
    fn restart(
        &mut self,
        snap: &WindowFrame,
        snap_d: Option<&DetailsFrame>,
        raw: &WindowFrame,
        raw_d: Option<&DetailsFrame>,
    ) -> bool {
        let mut any = false;
        for (i, (s, r)) in [
            (&snap.blue_team, &raw.blue_team),
            (&snap.red_team, &raw.red_team),
        ]
        .into_iter()
        .enumerate()
        {
            for (k, (now, before)) in [
                (r.total_kills, s.total_kills),
                (r.towers, s.towers),
                (r.inhibitors, s.inhibitors),
                (r.barons, s.barons),
            ]
            .into_iter()
            .enumerate()
            {
                if restarted(now, before) {
                    let o = &mut self.team[i];
                    *[
                        &mut o.kills,
                        &mut o.towers,
                        &mut o.inhibitors,
                        &mut o.barons,
                    ][k] = before.unwrap_or(0);
                    any = true;
                }
            }
            if r.dragons.as_ref().is_some_and(Vec::is_empty)
                && let Some(d) = s.dragons.as_ref().filter(|d| !d.is_empty())
            {
                self.dragons[i] = d.clone();
                any = true;
            }
            for rp in &r.participants {
                let Some(sp) = s
                    .participants
                    .iter()
                    .find(|q| q.participant_id == rp.participant_id)
                else {
                    continue;
                };
                let o = self.player.entry(rp.participant_id).or_default();
                for (k, (now, before)) in [
                    (rp.kills, sp.kills),
                    (rp.deaths, sp.deaths),
                    (rp.assists, sp.assists),
                ]
                .into_iter()
                .enumerate()
                {
                    if restarted(now, before) {
                        o[k] = before.unwrap_or(0);
                        any = true;
                    }
                }
            }
        }
        // before any kill or objective only the details show it: wards back to 0, a shorter
        // skill list
        if let (Some(s), Some(r)) = (snap_d, raw_d) {
            any |= r.participants.iter().any(|rp| {
                s.participants
                    .iter()
                    .find(|sp| sp.participant_id == rp.participant_id)
                    .is_some_and(|sp| {
                        restarted(rp.wards_placed, sp.wards_placed)
                            || rp.abilities.len() < sp.abilities.len()
                    })
            });
        }
        // the details restart with the window (seen together): wards and skills from the
        // frame the game went back to
        if any && let Some(d) = snap_d {
            for p in &d.participants {
                self.wards.insert(
                    p.participant_id,
                    [p.wards_placed.unwrap_or(0), p.wards_destroyed.unwrap_or(0)],
                );
                self.abilities.insert(p.participant_id, p.abilities.clone());
            }
        }
        any
    }
}

/// Both teams' gold, `None` when a team's is missing (no restart is seen from such a frame)
fn gold_sum(w: &WindowFrame) -> Option<i64> {
    Some(w.blue_team.total_gold? + w.red_team.total_gold?)
}

/// A frame of a server starting again after a chronobreak: every player level 1, no gold
fn zero_window(w: &WindowFrame) -> bool {
    let mut ps = w
        .blue_team
        .participants
        .iter()
        .chain(&w.red_team.participants)
        .peekable();
    ps.peek().is_some() && ps.all(|p| p.total_gold == Some(0) && p.level.unwrap_or(0) <= 1)
}

fn zero_details(d: &DetailsFrame) -> bool {
    !d.participants.is_empty()
        && d.participants
            .iter()
            .all(|p| p.total_gold_earned == Some(0))
}

/// Taken back between `snap` and `pause` (both with the offsets)
fn undone(pause: &WindowFrame, snap: &WindowFrame) -> Undone {
    let team = |a: &TeamFrame, b: &TeamFrame| {
        let d = |x: Option<i64>, y: Option<i64>| x.unwrap_or(0) - y.unwrap_or(0);
        let n = |t: &TeamFrame| {
            t.dragons
                .as_ref()
                .map_or(0, |d| i64::try_from(d.len()).unwrap_or(0))
        };
        TeamUndone {
            kills: d(a.total_kills, b.total_kills),
            gold: d(a.total_gold, b.total_gold),
            towers: d(a.towers, b.towers),
            inhibitors: d(a.inhibitors, b.inhibitors),
            barons: d(a.barons, b.barons),
            dragons: n(a) - n(b),
        }
    };
    let players = pause
        .blue_team
        .participants
        .iter()
        .chain(&pause.red_team.participants)
        .map(|a| {
            let b = snap
                .blue_team
                .participants
                .iter()
                .chain(&snap.red_team.participants)
                .find(|q| q.participant_id == a.participant_id)
                .cloned()
                .unwrap_or_default();
            let d = |x: Option<i64>, y: Option<i64>| x.unwrap_or(0) - y.unwrap_or(0);
            PlayerUndone {
                participant_id: i16::try_from(a.participant_id).unwrap_or_default(),
                kills: d(a.kills, b.kills),
                deaths: d(a.deaths, b.deaths),
                assists: d(a.assists, b.assists),
                gold: d(a.total_gold, b.total_gold),
                cs: d(a.creep_score, b.creep_score),
            }
        })
        .collect();
    Undone {
        teams: [
            team(&pause.blue_team, &snap.blue_team),
            team(&pause.red_team, &snap.red_team),
        ],
        players,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Skill {
    pub participant_id: i16,
    pub skill: String,
    pub game_time_s: i64,
    #[serde(skip)]
    pub game_ms: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Runes {
    pub participant_id: i16,
    pub style: Option<i64>,
    pub sub_style: Option<i64>,
    pub perks: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Chunk {
    pub wall_time: Vec<NaiveDateTime>,
    /// blue, red
    pub teams: [TeamCols; 2],
    /// participants 1 to 10
    pub players: Vec<PlayerCols>,
    pub markers: Vec<Marker>,
    /// A pause shows up once when it starts and again when it ends, the last one counts
    pub pauses: Vec<Pause>,
    pub skills: Vec<Skill>,
    pub runes: Vec<Runes>,
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            wall_time: vec![],
            teams: Default::default(),
            players: (1..=10)
                .map(|p| PlayerCols {
                    participant_id: p,
                    ..Default::default()
                })
                .collect(),
            markers: vec![],
            pauses: vec![],
            skills: vec![],
            runes: vec![],
            warnings: vec![],
        }
    }
}

impl Chunk {
    pub fn points(&self) -> usize {
        self.wall_time.len()
    }

    /// A chronobreak: only the first `keep` points stay, markers after `snap_ms` are marked as
    /// undone by `pause` (kept), skills after it are dropped
    fn rewind(&mut self, keep: usize, snap_ms: i64, pause: i16) {
        self.wall_time.truncate(keep);
        for t in &mut self.teams {
            for v in [
                &mut t.gold,
                &mut t.kills,
                &mut t.towers,
                &mut t.inhibitors,
                &mut t.barons,
                &mut t.dragons,
            ] {
                v.truncate(keep);
            }
        }
        for p in &mut self.players {
            for v in [
                &mut p.gold,
                &mut p.level,
                &mut p.kills,
                &mut p.deaths,
                &mut p.assists,
                &mut p.cs,
                &mut p.hp,
                &mut p.max_hp,
                &mut p.gold_earned,
                &mut p.wards_placed,
                &mut p.wards_destroyed,
                &mut p.attack_damage,
                &mut p.ability_power,
                &mut p.attack_speed,
                &mut p.life_steal,
                &mut p.armor,
                &mut p.magic_resist,
            ] {
                v.truncate(keep);
            }
            for v in [
                &mut p.kill_participation,
                &mut p.damage_share,
                &mut p.crit_chance,
                &mut p.tenacity,
            ] {
                v.truncate(keep);
            }
            let items: i64 = p.item_counts.iter().take(keep).flatten().sum();
            p.items.truncate(usize::try_from(items).unwrap_or(0));
            p.item_counts.truncate(keep);
        }
        for m in &mut self.markers {
            if m.game_ms > snap_ms && m.undone_by_pause.is_none() {
                m.undone_by_pause = Some(pause);
            }
        }
        self.skills.retain(|s| s.game_ms <= snap_ms);
    }

    /// A pause, replacing the one with its number (it started in an earlier frame)
    fn push_pause(&mut self, p: Pause) {
        match self.pauses.iter_mut().find(|q| q.number == p.number) {
            Some(q) => *q = p,
            None => self.pauses.push(p),
        }
    }

    /// `next` after `self`, as if built in one go
    pub fn append(&mut self, next: Chunk) {
        self.wall_time.extend(next.wall_time);
        for (a, b) in self.teams.iter_mut().zip(next.teams) {
            a.gold.extend(b.gold);
            a.kills.extend(b.kills);
            a.towers.extend(b.towers);
            a.inhibitors.extend(b.inhibitors);
            a.barons.extend(b.barons);
            a.dragons.extend(b.dragons);
        }
        for (a, b) in self.players.iter_mut().zip(next.players) {
            a.gold.extend(b.gold);
            a.level.extend(b.level);
            a.kills.extend(b.kills);
            a.deaths.extend(b.deaths);
            a.assists.extend(b.assists);
            a.cs.extend(b.cs);
            a.hp.extend(b.hp);
            a.max_hp.extend(b.max_hp);
            a.gold_earned.extend(b.gold_earned);
            a.kill_participation.extend(b.kill_participation);
            a.damage_share.extend(b.damage_share);
            a.wards_placed.extend(b.wards_placed);
            a.wards_destroyed.extend(b.wards_destroyed);
            a.attack_damage.extend(b.attack_damage);
            a.ability_power.extend(b.ability_power);
            a.crit_chance.extend(b.crit_chance);
            a.attack_speed.extend(b.attack_speed);
            a.life_steal.extend(b.life_steal);
            a.armor.extend(b.armor);
            a.magic_resist.extend(b.magic_resist);
            a.tenacity.extend(b.tenacity);
            a.items.extend(b.items);
            a.item_counts.extend(b.item_counts);
        }
        self.markers.extend(next.markers);
        for p in next.pauses {
            self.push_pause(p);
        }
        self.skills.extend(next.skills);
        self.runes.extend(next.runes);
        self.warnings.extend(next.warnings);
    }

    fn push_point(
        &mut self,
        wall: DateTime<Utc>,
        w: Option<&WindowFrame>,
        d: Option<&DetailsFrame>,
    ) -> Result<()> {
        self.wall_time.push(wall.naive_utc());
        for (cols, team) in self
            .teams
            .iter_mut()
            .zip([w.map(|w| &w.blue_team), w.map(|w| &w.red_team)])
        {
            cols.gold.push(team.and_then(|t| t.total_gold));
            cols.kills.push(team.and_then(|t| t.total_kills));
            cols.towers.push(team.and_then(|t| t.towers));
            cols.inhibitors.push(team.and_then(|t| t.inhibitors));
            cols.barons.push(team.and_then(|t| t.barons));
            cols.dragons.push(
                team.and_then(|t| t.dragons.as_ref())
                    .map(|d| i64::try_from(d.len()).unwrap_or(i64::MAX)),
            );
        }
        for cols in &mut self.players {
            let id = i64::from(cols.participant_id);
            let wp = w.and_then(|w| {
                w.blue_team
                    .participants
                    .iter()
                    .chain(&w.red_team.participants)
                    .find(|p| p.participant_id == id)
            });
            let dp = d.and_then(|d| d.participants.iter().find(|p| p.participant_id == id));
            cols.gold.push(wp.and_then(|p| p.total_gold));
            cols.level.push(wp.and_then(|p| p.level));
            cols.kills.push(wp.and_then(|p| p.kills));
            cols.deaths.push(wp.and_then(|p| p.deaths));
            cols.assists.push(wp.and_then(|p| p.assists));
            cols.cs.push(wp.and_then(|p| p.creep_score));
            cols.hp.push(wp.and_then(|p| p.current_health));
            cols.max_hp.push(wp.and_then(|p| p.max_health));
            cols.gold_earned.push(dp.and_then(|p| p.total_gold_earned));
            cols.kill_participation
                .push(dp.and_then(|p| finite(p.kill_participation)));
            cols.damage_share
                .push(dp.and_then(|p| finite(p.champion_damage_share)));
            cols.wards_placed.push(dp.and_then(|p| p.wards_placed));
            cols.wards_destroyed
                .push(dp.and_then(|p| p.wards_destroyed));
            cols.attack_damage.push(dp.and_then(|p| p.attack_damage));
            cols.ability_power.push(dp.and_then(|p| p.ability_power));
            cols.crit_chance
                .push(dp.and_then(|p| finite(p.critical_chance)));
            cols.attack_speed.push(dp.and_then(|p| p.attack_speed));
            cols.life_steal.push(dp.and_then(|p| p.life_steal));
            cols.armor.push(dp.and_then(|p| p.armor));
            cols.magic_resist.push(dp.and_then(|p| p.magic_resistance));
            cols.tenacity.push(dp.and_then(|p| finite(p.tenacity)));
            match dp {
                Some(p) if p.items_unknown => cols.item_counts.push(None),
                Some(p) => {
                    cols.items.extend(p.items.iter().map(|i| Some(*i)));
                    cols.item_counts
                        .push(Some(i64::try_from(p.items.len()).unwrap_or(i64::MAX)));
                }
                None => cols.item_counts.push(None),
            }
        }
        Ok(())
    }
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite())
}

/// Where a build is: enough to go on with the next frames (saved as JSON between live ticks)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiveState {
    step_s: i32,
    first_frame: Option<DateTime<Utc>>,
    /// Game time of the last `window` frame, ms
    game_ms: i64,
    last_window: Option<WindowFrame>,
    /// The last `details` frame at or before the last `window` frame
    last_details: Option<DetailsFrame>,
    /// `details` frames newer than the last `window` frame, used when the window catches up
    pending_details: Vec<DetailsFrame>,
    next_point: i64,
    skills_seen: Vec<usize>,
    runes_set: Vec<bool>,
    finished: bool,
    warned: Vec<String>,
    /// Live: the next feed time to ask for. Not the last frame: the feed can go minutes
    /// without one (a long pause)
    #[serde(default)]
    cursor: Option<DateTime<Utc>>,
    /// Highest towers, inhibitors, barons and dragons seen per side (blue, red): a count that
    /// drops and comes back makes no new markers
    #[serde(default)]
    max_seen: [[i64; 4]; 2],
    /// Pauses so far, the last one still open while the game is paused
    #[serde(default)]
    pauses: Vec<Pause>,
    #[serde(default)]
    offsets: Offsets,
    /// Since a restart the feed's item lists aren't the inventory
    #[serde(default)]
    items_unknown: bool,
}

impl LiveState {
    pub fn first_frame(&self) -> Option<DateTime<Utc>> {
        self.first_frame
    }

    /// Game time of the last frame, s
    pub fn duration_s(&self) -> i64 {
        self.game_ms / 1000
    }

    pub fn points(&self) -> i64 {
        self.next_point
    }

    pub fn step_s(&self) -> i32 {
        self.step_s
    }

    pub fn cursor(&self) -> Option<DateTime<Utc>> {
        self.cursor
    }

    /// Wall time of the last `window` frame
    pub fn last_frame(&self) -> Option<DateTime<Utc>> {
        self.last_window.as_ref().map(|w| w.rfc460_timestamp)
    }
}

/// What coming back from a chronobreak changed
struct Restart {
    /// The frame the counts go on from (the snapshot), `None` live
    values_from: Option<WindowFrame>,
    /// Game seconds gone back and what was taken back, only after the game
    back: Option<(i64, Undone)>,
    /// The feed started its counts again
    reset: bool,
}

pub struct Builder {
    state: LiveState,
    /// After the game only: every window frame with its game time (ms), and the details, to
    /// find where a chronobreak went back to
    history: Option<VecDeque<(i64, WindowFrame)>>,
    details_history: VecDeque<DetailsFrame>,
}

fn parse<T: for<'de> Deserialize<'de>>(f: &RawFrame, what: &str) -> Result<T> {
    serde_json::from_str(f.raw.get()).wrap_err_with(|| format!("{what} frame at {}", f.ts))
}

impl Builder {
    pub fn new(step_s: i32) -> Self {
        Self {
            state: LiveState {
                step_s,
                first_frame: None,
                game_ms: 0,
                last_window: None,
                last_details: None,
                pending_details: vec![],
                next_point: 0,
                skills_seen: vec![0; 10],
                runes_set: vec![false; 10],
                finished: false,
                warned: vec![],
                cursor: None,
                max_seen: [[0; 4]; 2],
                pauses: vec![],
                offsets: Offsets::default(),
                items_unknown: false,
            },
            history: None,
            details_history: VecDeque::new(),
        }
    }

    /// Go on from a saved state (live). A state of another shape is refused, not trusted
    pub fn resume(state: LiveState) -> Result<Self> {
        if state.skills_seen.len() != 10 || state.runes_set.len() != 10 || state.step_s <= 0 {
            return Err(eyre!(
                "live state of a bad shape (skills {}, runes {}, step {})",
                state.skills_seen.len(),
                state.runes_set.len(),
                state.step_s
            ));
        }
        Ok(Self {
            state,
            history: None,
            details_history: VecDeque::new(),
        })
    }

    /// A whole game: keeps the frames, so a chronobreak goes back to where the game did
    pub fn for_archive(step_s: i32) -> Self {
        let mut b = Self::new(step_s);
        b.history = Some(VecDeque::new());
        b
    }

    pub fn state(&self) -> &LiveState {
        &self.state
    }

    pub fn set_cursor(&mut self, at: DateTime<Utc>) {
        self.state.cursor = Some(at);
    }

    fn step_ms(&self) -> i64 {
        i64::from(self.state.step_s) * 1000
    }

    fn warn_once(&mut self, key: &str, msg: String, chunk: &mut Chunk) {
        if !self.state.warned.iter().any(|w| w == key) {
            self.state.warned.push(key.to_string());
            chunk.warnings.push(msg);
        }
    }

    /// New frames, in any order, repeats and frames already seen skipped
    pub fn push(&mut self, window: &[RawFrame], details: &[RawFrame]) -> Result<Chunk> {
        let mut chunk = Chunk::default();
        let last_ts = self.state.last_window.as_ref().map(|w| w.rfc460_timestamp);
        let mut ws: Vec<WindowFrame> = window
            .iter()
            .filter(|f| last_ts.is_none_or(|l| f.ts > l))
            .map(|f| parse(f, "window"))
            .collect::<Result<_>>()?;
        // two frames with the same time (in game, then paused): the last one the feed sent
        ws.sort_by_key(|w| w.rfc460_timestamp);
        let ws = ws.into_iter().fold(Vec::<WindowFrame>::new(), |mut v, w| {
            match v.last_mut() {
                Some(l) if l.rfc460_timestamp == w.rfc460_timestamp => *l = w,
                _ => v.push(w),
            }
            v
        });
        let seen_details = self.state.last_details.as_ref().map(|d| d.rfc460_timestamp);
        for f in details {
            if seen_details.is_some_and(|l| f.ts <= l) {
                continue;
            }
            self.state.pending_details.push(parse(f, "details")?);
        }
        // stable: of two frames with the same time the last one sent stays
        self.state
            .pending_details
            .sort_by_key(|d| d.rfc460_timestamp);
        let pending = std::mem::take(&mut self.state.pending_details);
        self.state.pending_details =
            pending
                .into_iter()
                .fold(vec![], |mut v: Vec<DetailsFrame>, d| {
                    match v.last_mut() {
                        Some(l) if l.rfc460_timestamp == d.rfc460_timestamp => *l = d,
                        _ => v.push(d),
                    }
                    v
                });
        for w in ws {
            for p in w
                .blue_team
                .participants
                .iter()
                .chain(&w.red_team.participants)
            {
                if !(1..=10).contains(&p.participant_id) {
                    return Err(eyre!(
                        "window frame at {}: participant id {}",
                        w.rfc460_timestamp,
                        p.participant_id
                    ));
                }
            }
            self.window_frame(w, &mut chunk)?;
        }
        Ok(chunk)
    }

    fn window_frame(&mut self, w: WindowFrame, chunk: &mut Chunk) -> Result<()> {
        if self.state.finished {
            self.warn_once(
                "after_finished",
                format!(
                    "frames after the finished one (first at {}), skipped",
                    w.rfc460_timestamp
                ),
                chunk,
            );
            return Ok(());
        }
        if let Some(first) = self.state.first_frame
            && (w.rfc460_timestamp - first).abs() > MAX_WALL
        {
            return Err(eyre!(
                "window frame at {}: {} h from the first one ({first})",
                w.rfc460_timestamp,
                (w.rfc460_timestamp - first).num_hours()
            ));
        }
        if self.state.game_ms > RESTART_AFTER_MS && zero_window(&w) {
            self.warn_once(
                "zero_frames",
                format!(
                    "frames with every player at level 1 and no gold from {} (a server starting again), skipped",
                    w.rfc460_timestamp
                ),
                chunk,
            );
            return Ok(());
        }
        let Some(prev) = self.state.last_window.take() else {
            // the first frame: game time 0, what's already taken makes no markers
            self.state.first_frame = Some(w.rfc460_timestamp);
            self.state.game_ms = 0;
            for (i, team) in [&w.blue_team, &w.red_team].into_iter().enumerate() {
                for (k, c) in counts(team).into_iter().enumerate() {
                    self.state.max_seen[i][k] = c.unwrap_or(0);
                }
            }
            self.take_details(w.rfc460_timestamp, 0, chunk)?;
            self.pause_edges(None, &w, 0, chunk);
            self.after_frame(&w);
            self.remember(0, &w);
            self.state.last_window = Some(w);
            return Ok(());
        };
        let mut w = w;
        // a chronobreak: back from a pause with less gold than before it
        let restart = (prev.game_state.as_deref() == Some("paused")
            && w.game_state.as_deref() != Some("paused")
            && matches!((gold_sum(&w), gold_sum(&prev)), (Some(a), Some(b)) if a < b))
        .then(|| self.restart(&prev, &w, chunk));
        let values_from = restart
            .as_ref()
            .and_then(|r| r.values_from.clone())
            .unwrap_or_else(|| prev.clone());
        self.state.offsets.window(&mut w);
        let wall_gap = w.rfc460_timestamp - prev.rfc460_timestamp;
        let counts = match prev.game_state.as_deref() {
            Some("paused") => false,
            Some("in_game") => true,
            other => {
                let s = other.unwrap_or("none").to_string();
                self.warn_once(
                    &format!("state:{s}"),
                    format!("game state {s:?} counted as in game"),
                    chunk,
                );
                true
            }
        };
        let g0 = self.state.game_ms;
        let g1 = if counts {
            g0 + wall_gap.num_milliseconds()
        } else {
            g0
        };
        // points before this frame's game time take the previous frame (empty in a long gap)
        let long_gap = wall_gap > MAX_CARRY && counts;
        if long_gap {
            chunk.warnings.push(format!(
                "feed gap of {} s at game time {}",
                wall_gap.num_seconds(),
                fmt_game_time(g0)
            ));
        }
        while self.state.next_point * self.step_ms() < g1 {
            let at = self.state.next_point * self.step_ms();
            if long_gap && at > g0 {
                let wall = prev.rfc460_timestamp + TimeDelta::milliseconds(at - g0);
                chunk.push_point(wall, None, None)?;
            } else {
                chunk.push_point(
                    prev.rfc460_timestamp,
                    Some(&prev),
                    self.state.last_details.as_ref(),
                )?;
            }
            self.state.next_point += 1;
        }
        self.markers(&values_from, &w, g1, chunk);
        self.pause_edges(Some(&prev), &w, g1, chunk);
        if let Some(r) = restart
            && let Some(p) = self.state.pauses.last_mut()
        {
            p.counters_reset = r.reset;
            if let Some((s, u)) = r.back {
                p.rollback_s = Some(s);
                p.undone = Some(u);
            }
            let p = p.clone();
            chunk.push_pause(p);
        }
        self.remember(g1, &w);
        self.state.game_ms = g1;
        self.take_details(w.rfc460_timestamp, g1, chunk)?;
        self.after_frame(&w);
        self.state.last_window = Some(w);
        Ok(())
    }

    /// The archive build keeps the frames of the last `MAX_ROLLBACK_MS` of game time
    fn remember(&mut self, game_ms: i64, w: &WindowFrame) {
        let Some(h) = &mut self.history else {
            return;
        };
        h.push_back((game_ms, w.clone()));
        while h
            .front()
            .is_some_and(|(ms, _)| *ms < game_ms - MAX_ROLLBACK_MS)
        {
            h.pop_front();
        }
        let oldest = h.front().map(|(_, f)| f.rfc460_timestamp);
        // keep one details frame at or before the oldest window frame
        while self.details_history.len() > 1
            && self.details_history[1].rfc460_timestamp
                <= oldest.unwrap_or(DateTime::<Utc>::MAX_UTC)
        {
            self.details_history.pop_front();
        }
    }

    /// Back from a chronobreak: counts the feed started again become offsets. After the game
    /// (frames kept) the game goes back to the last frame with no more gold than `w`: points
    /// after it go, markers after it are marked undone, and what was taken back is kept
    fn restart(&mut self, prev: &WindowFrame, w: &WindowFrame, chunk: &mut Chunk) -> Restart {
        let number = i16::try_from(self.state.pauses.len()).unwrap_or(i16::MAX);
        let snap = self.history.as_ref().and_then(|h| {
            h.iter()
                .rev()
                .find(|(_, f)| {
                    f.rfc460_timestamp <= prev.rfc460_timestamp
                        && gold_sum(f).zip(gold_sum(w)).is_some_and(|(f, w)| f <= w)
                })
                .cloned()
        });
        let snap_w = snap.as_ref().map_or(prev, |(_, f)| f).clone();
        let snap_d = self
            .details_history
            .iter()
            .rev()
            .find(|d| d.rfc460_timestamp <= snap_w.rfc460_timestamp)
            .cloned()
            .or_else(|| self.state.last_details.clone());
        // the details frame closest to `w`, as the feed sent it
        let raw_d = self
            .state
            .pending_details
            .iter()
            .filter(|d| !zero_details(d))
            .min_by_key(|d| (d.rfc460_timestamp - w.rfc460_timestamp).abs())
            .cloned();
        let reset = self
            .state
            .offsets
            .restart(&snap_w, snap_d.as_ref(), w, raw_d.as_ref());
        if reset {
            self.state.items_unknown = true;
        }
        let Some((snap_ms, snap_w)) = snap else {
            // live: no frames kept, the counts go on from the pause
            if reset {
                chunk.warnings.push(format!(
                    "back from pause {number} at {}: the feed started its counts again, counted on from the pause",
                    w.rfc460_timestamp
                ));
            }
            return Restart {
                values_from: None,
                back: None,
                reset,
            };
        };
        // `history` is only kept by `build`, which pushes the whole game at once
        debug_assert_eq!(
            i64::try_from(chunk.points()).ok(),
            Some(self.state.next_point)
        );
        let keep = (snap_ms / self.step_ms() + 1).min(self.state.next_point);
        chunk.rewind(usize::try_from(keep).unwrap_or(0), snap_ms, number);
        let back_s = (self.state.game_ms - snap_ms) / 1000;
        chunk.warnings.push(format!(
            "chronobreak after pause {number}: back {back_s} s to game time {}{}",
            fmt_game_time(snap_ms),
            if reset {
                ", the feed started its counts again"
            } else {
                ""
            }
        ));
        self.state.next_point = keep;
        self.state.game_ms = snap_ms;
        for (i, team) in [&snap_w.blue_team, &snap_w.red_team]
            .into_iter()
            .enumerate()
        {
            for (k, c) in counts(team).into_iter().enumerate() {
                self.state.max_seen[i][k] = c.unwrap_or(0);
            }
        }
        if let Some(d) = &snap_d {
            for p in &d.participants {
                if let Ok(i) = usize::try_from(p.participant_id - 1)
                    && i < 10
                {
                    self.state.skills_seen[i] = p.abilities.len();
                }
            }
        }
        self.state.last_details = snap_d;
        Restart {
            back: Some((back_s, undone(prev, &snap_w))),
            values_from: Some(snap_w),
            reset,
        }
    }

    /// A `paused` frame after one in play opens a pause, a frame in play after a `paused` one
    /// closes it
    fn pause_edges(
        &mut self,
        prev: Option<&WindowFrame>,
        w: &WindowFrame,
        game_ms: i64,
        chunk: &mut Chunk,
    ) {
        let paused = |f: &WindowFrame| f.game_state.as_deref() == Some("paused");
        let was = prev.is_some_and(paused);
        let pause = if paused(w) && !was {
            let number = i16::try_from(self.state.pauses.len() + 1).unwrap_or(i16::MAX);
            let p = Pause {
                number,
                game_time_s: game_ms / 1000,
                started_at: w.rfc460_timestamp,
                ended_at: None,
                ..Default::default()
            };
            self.state.pauses.push(p.clone());
            p
        } else if was && !paused(w) {
            let Some(p) = self.state.pauses.last_mut() else {
                return;
            };
            p.ended_at = Some(w.rfc460_timestamp);
            p.clone()
        } else {
            return;
        };
        chunk.push_pause(pause);
    }

    fn after_frame(&mut self, w: &WindowFrame) {
        if w.game_state.as_deref() == Some("finished") {
            self.state.finished = true;
        }
    }

    /// `details` frames up to `ts` become the current one, with their skills and runes
    fn take_details(&mut self, ts: DateTime<Utc>, game_ms: i64, chunk: &mut Chunk) -> Result<()> {
        let n = self
            .state
            .pending_details
            .iter()
            .take_while(|d| d.rfc460_timestamp <= ts)
            .count();
        let taken: Vec<DetailsFrame> = self.state.pending_details.drain(..n).collect();
        for mut d in taken {
            if game_ms > RESTART_AFTER_MS && zero_details(&d) {
                continue;
            }
            self.state.offsets.details(&mut d);
            if self.state.items_unknown {
                for p in &mut d.participants {
                    p.items_unknown = true;
                }
            }
            for p in &d.participants {
                let Some(i) = usize::try_from(p.participant_id - 1)
                    .ok()
                    .filter(|i| *i < 10)
                else {
                    return Err(eyre!(
                        "details frame at {}: participant id {}",
                        d.rfc460_timestamp,
                        p.participant_id
                    ));
                };
                let pid = i16::try_from(p.participant_id).unwrap_or_default();
                let seen = self.state.skills_seen[i];
                if p.abilities.len() > seen {
                    for s in &p.abilities[seen..] {
                        chunk.skills.push(Skill {
                            participant_id: pid,
                            skill: s.clone(),
                            game_time_s: game_ms / 1000,
                            game_ms,
                        });
                    }
                    self.state.skills_seen[i] = p.abilities.len();
                } else if p.abilities.len() < seen {
                    self.warn_once(
                        &format!("skills:{pid}"),
                        format!(
                            "participant {pid}: skill list went from {seen} to {}",
                            p.abilities.len()
                        ),
                        chunk,
                    );
                }
                if let Some(perks) = &p.perk_metadata {
                    let runes = Runes {
                        participant_id: pid,
                        style: perks.style_id,
                        sub_style: perks.sub_style_id,
                        perks: perks.perks.clone(),
                    };
                    if !self.state.runes_set[i] {
                        self.state.runes_set[i] = true;
                        chunk.runes.push(runes);
                    } else if self
                        .state
                        .last_details
                        .as_ref()
                        .and_then(|l| {
                            l.participants
                                .iter()
                                .find(|q| q.participant_id == p.participant_id)
                        })
                        .and_then(|q| q.perk_metadata.as_ref())
                        .is_some_and(|old| old != perks)
                    {
                        self.warn_once(
                            &format!("runes:{pid}"),
                            format!(
                                "participant {pid}: runes changed during the game, first ones kept"
                            ),
                            chunk,
                        );
                    }
                }
            }
            if self.history.is_some() {
                self.details_history.push_back(d.clone());
            }
            self.state.last_details = Some(d);
        }
        Ok(())
    }

    fn markers(&mut self, prev: &WindowFrame, w: &WindowFrame, game_ms: i64, chunk: &mut Chunk) {
        let t = game_ms / 1000;
        for (i, side, a, b) in [
            (0, Side::Blue, &prev.blue_team, &w.blue_team),
            (1, Side::Red, &prev.red_team, &w.red_team),
        ] {
            let (before, after) = (counts(a), counts(b));
            for (k, kind) in KINDS.iter().enumerate() {
                let Some(now) = after[k] else { continue };
                if let Some(was) = before[k]
                    && now < was
                {
                    chunk.warnings.push(format!(
                        "{side:?} {kind}s went from {was} to {now} at game time {}",
                        fmt_game_time(game_ms)
                    ));
                }
                let max = self.state.max_seen[i][k];
                if now <= max {
                    continue;
                }
                self.state.max_seen[i][k] = now;
                if now - max > MAX_JUMP {
                    chunk.warnings.push(format!(
                        "{side:?} {kind}s jumped from {max} to {now} at game time {}, no markers",
                        fmt_game_time(game_ms)
                    ));
                    continue;
                }
                let from = usize::try_from(max).unwrap_or(0);
                for n in from..usize::try_from(now).unwrap_or(from) {
                    let detail = if *kind == "dragon" {
                        b.dragons.as_ref().and_then(|d| d.get(n)).cloned()
                    } else {
                        None
                    };
                    chunk.markers.push(Marker {
                        game_time_s: t,
                        game_ms,
                        kind,
                        side,
                        detail,
                        undone_by_pause: None,
                    });
                }
            }
        }
    }

    /// The points up to the last frame. Only once the game is over: the last point is the last
    /// frame even when it isn't on a step
    pub fn finish(&mut self) -> Result<Chunk> {
        let mut chunk = Chunk::default();
        let Some(last) = self.state.last_window.clone() else {
            return Err(eyre!("no window frames"));
        };
        let g = self.state.game_ms;
        while self.state.next_point * self.step_ms() <= g {
            chunk.push_point(
                last.rfc460_timestamp,
                Some(&last),
                self.state.last_details.as_ref(),
            )?;
            self.state.next_point += 1;
        }
        if (self.state.next_point - 1) * self.step_ms() < g {
            chunk.push_point(
                last.rfc460_timestamp,
                Some(&last),
                self.state.last_details.as_ref(),
            )?;
            self.state.next_point += 1;
        }
        Ok(chunk)
    }
}

const KINDS: [&str; 4] = ["tower", "inhibitor", "baron", "dragon"];

/// A team's towers, inhibitors, barons and dragons (in `KINDS` order), `None` when unknown
fn counts(t: &TeamFrame) -> [Option<i64>; 4] {
    [
        t.towers,
        t.inhibitors,
        t.barons,
        t.dragons
            .as_ref()
            .map(|d| i64::try_from(d.len()).unwrap_or(i64::MAX)),
    ]
}

fn fmt_game_time(ms: i64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub first_frame: NaiveDateTime,
    pub duration_s: i64,
    pub points: i64,
    pub step_s: i32,
    pub chunk: Chunk,
}

/// A whole game
pub fn build(window: &[RawFrame], details: &[RawFrame], step_s: i32) -> Result<Timeline> {
    if step_s <= 0 {
        return Err(eyre!("step {step_s} s"));
    }
    let mut b = Builder::for_archive(step_s);
    let mut chunk = b.push(window, details)?;
    chunk.append(b.finish()?);
    let state = b.state();
    let first_frame = state
        .first_frame
        .ok_or_else(|| eyre!("no window frames"))?
        .naive_utc();
    Ok(Timeline {
        first_frame,
        duration_s: state.duration_s(),
        points: state.points(),
        step_s,
        chunk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{hand, parse_frames};
    use serde_json::{Value, json};

    fn raws(v: &[Value]) -> Vec<RawFrame> {
        v.iter().map(hand::raw).collect()
    }

    fn at(s: i64) -> DateTime<Utc> {
        hand::start() + TimeDelta::seconds(s)
    }

    /// A `window` frame at `s` seconds with `state`, gold `gold`, blue towers and dragons
    fn wf(s: i64, state: &str, gold: i64, towers: i32, dragons: &[&str]) -> Value {
        let mut f = hand::window_frame(at(s), state, gold);
        f["blueTeam"]["towers"] = json!(towers);
        f["blueTeam"]["dragons"] = json!(dragons);
        f
    }

    fn game(seconds: i64) -> (Vec<RawFrame>, Vec<RawFrame>) {
        let (w, d) = hand::game(seconds, true);
        (raws(&w), raws(&d))
    }

    #[test]
    fn normal_game() {
        let (w, d) = game(95);
        let t = build(&w, &d, 10).unwrap();
        // 0, 10, …, 90, then the last frame at 95
        assert_eq!(t.points, 11);
        assert_eq!(t.duration_s, 95);
        assert_eq!(t.chunk.points(), 11);
        assert_eq!(t.first_frame, hand::start().naive_utc());
        let blue = &t.chunk.teams[0];
        assert_eq!(blue.gold[0], Some(2500));
        assert_eq!(blue.gold[3], Some(2800));
        assert_eq!(blue.gold[10], Some(2500 + 950));
        assert_eq!(t.chunk.wall_time[10], at(95).naive_utc());
        for p in &t.chunk.players {
            assert_eq!(p.gold.len(), 11);
            assert_eq!(p.item_counts.len(), 11);
            assert_eq!(p.damage_share[0], Some(0.1));
        }
        // two items a point, nothing padded
        assert_eq!(
            &t.chunk.players[0].items[..3],
            &[Some(1055), Some(2003), Some(1055)]
        );
        assert_eq!(t.chunk.players[0].items.len(), 11 * 2);
        assert_eq!(t.chunk.players[0].item_counts[0], Some(2));
        assert!(t.chunk.warnings.is_empty(), "{:?}", t.chunk.warnings);
    }

    #[test]
    fn length_on_a_step_and_just_past_one() {
        let (w, d) = game(90);
        assert_eq!(build(&w, &d, 10).unwrap().points, 10);
        let (w, d) = game(91);
        assert_eq!(build(&w, &d, 10).unwrap().points, 11);
        let (w, d) = game(0);
        let t = build(&w, &d, 10).unwrap();
        assert_eq!((t.points, t.duration_s), (1, 0));
    }

    #[test]
    fn other_steps() {
        let (w, d) = game(95);
        assert_eq!(build(&w, &d, 5).unwrap().points, 20);
        assert_eq!(build(&w, &d, 60).unwrap().points, 3);
        assert!(build(&w, &d, 0).is_err());
    }

    #[test]
    fn pause_is_not_game_time() {
        // in game 0-20, paused 20-80, in game 80-100, finished at 100
        let mut w = Vec::new();
        for s in 0..=100 {
            let state = match s {
                100 => "finished",
                20..80 => "paused",
                _ => "in_game",
            };
            w.push(hand::window_frame(at(s), state, 2500 + s));
        }
        let t = build(&raws(&w), &[], 10).unwrap();
        // 20 s + the frame at 80 (paused → the gap 79-80 doesn't count) + 20 s
        assert_eq!(t.duration_s, 40);
        assert_eq!(t.points, 5);
        // point 2 (game time 20) is the last paused frame, point 3 (30) is wall 90
        assert_eq!(t.chunk.wall_time[2], at(80).naive_utc());
        assert_eq!(t.chunk.wall_time[3], at(90).naive_utc());
        assert_eq!(t.chunk.teams[0].gold[3], Some(2590));
        // the wall time jumps over the pause
        assert_eq!(t.chunk.wall_time[1], at(10).naive_utc());
    }

    #[test]
    fn pauses_at_the_start_and_before_the_end() {
        let mut w = Vec::new();
        for s in 0..=60 {
            let state = match s {
                60 => "finished",
                0..10 | 50..60 => "paused",
                _ => "in_game",
            };
            w.push(hand::window_frame(at(s), state, 0));
        }
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.duration_s, 40);
        assert_eq!(t.points, 5);
    }

    #[test]
    fn unknown_state_counts_as_in_game_and_warns_once() {
        let w: Vec<Value> = (0..=20)
            .map(|s| hand::window_frame(at(s), if s == 20 { "finished" } else { "loading" }, 0))
            .collect();
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.duration_s, 20);
        assert_eq!(
            t.chunk.warnings,
            vec![r#"game state "loading" counted as in game"#]
        );
    }

    #[test]
    fn gaps_short_carried_long_empty() {
        // frames at 0..=5, then 25..=30 (20 s gap): carried
        let w: Vec<Value> = (0..=5)
            .chain(25..=30)
            .map(|s| hand::window_frame(at(s), "in_game", s))
            .collect();
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.teams[0].gold[1], Some(5));
        assert_eq!(t.chunk.teams[0].gold[2], Some(5));
        assert!(t.chunk.warnings.is_empty());
        // 0..=5, then 65..=70 (60 s gap): points 1-6 empty, wall time still set
        let w: Vec<Value> = (0..=5)
            .chain(65..=70)
            .map(|s| hand::window_frame(at(s), "in_game", s))
            .collect();
        let t = build(&raws(&w), &[], 10).unwrap();
        let gold = &t.chunk.teams[0].gold;
        assert_eq!(gold[0], Some(0));
        assert!(gold[1..=6].iter().all(Option::is_none), "{gold:?}");
        assert_eq!(gold[7], Some(70));
        assert_eq!(t.chunk.wall_time[3], at(30).naive_utc());
        // no details at all: no items, every count empty
        assert!(t.chunk.players[0].items.is_empty());
        assert!(t.chunk.players[0].item_counts.iter().all(Option::is_none));
        assert_eq!(t.chunk.warnings, vec!["feed gap of 60 s at game time 0:05"]);
    }

    #[test]
    fn order_and_repeats_dont_matter() {
        let (w, d) = game(40);
        let straight = build(&w, &d, 10).unwrap();
        let mut shuffled: Vec<RawFrame> = w.iter().rev().cloned().collect();
        shuffled.extend(w.iter().cloned());
        let mut d2 = d.clone();
        d2.extend(d.iter().rev().cloned());
        assert_eq!(build(&shuffled, &d2, 10).unwrap(), straight);
    }

    #[test]
    fn frames_a_millisecond_apart() {
        let mut w = vec![
            hand::window_frame(at(0), "in_game", 1),
            hand::window_frame(at(0) + TimeDelta::milliseconds(1), "in_game", 2),
        ];
        w.push(hand::window_frame(at(10), "finished", 3));
        let t = build(&raws(&w), &[], 10).unwrap();
        // point 0 = the last frame at or before game time 0
        assert_eq!(t.chunk.teams[0].gold[0], Some(1));
        assert_eq!(t.points, 2);
    }

    #[test]
    fn nothing_to_build() {
        assert!(build(&[], &[], 10).is_err());
        let (_, d) = game(10);
        assert!(build(&[], &d, 10).is_err());
    }

    #[test]
    fn markers_for_every_count_that_goes_up() {
        let w = vec![
            wf(0, "in_game", 0, 0, &[]),
            wf(10, "in_game", 0, 1, &[]),
            wf(20, "in_game", 0, 3, &["infernal"]),
            wf(30, "in_game", 0, 3, &["infernal", "ocean", "elder"]),
            wf(40, "finished", 0, 3, &["infernal", "ocean", "elder"]),
        ];
        let t = build(&raws(&w), &[], 10).unwrap();
        let m: Vec<(i64, &str, Option<&str>)> = t
            .chunk
            .markers
            .iter()
            .map(|m| (m.game_time_s, m.kind, m.detail.as_deref()))
            .collect();
        assert_eq!(
            m,
            [
                (10, "tower", None),
                (20, "tower", None),
                (20, "tower", None),
                (20, "dragon", Some("infernal")),
                (30, "dragon", Some("ocean")),
                (30, "dragon", Some("elder")),
            ]
        );
        assert!(t.chunk.markers.iter().all(|m| m.side == Side::Blue));
        assert_eq!(t.chunk.teams[0].dragons[3], Some(3));
    }

    #[test]
    fn counts_going_down_give_no_marker() {
        let mut w = vec![
            wf(0, "in_game", 0, 2, &["ocean"]),
            wf(10, "in_game", 0, 1, &[]),
            wf(20, "finished", 0, 2, &["ocean"]),
        ];
        w[1]["blueTeam"]["barons"] = json!(0);
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.warnings.len(), 2, "{:?}", t.chunk.warnings);
        assert!(t.chunk.warnings[0].contains("towers went from 2 to 1"));
        // back up after the drop (a chronobreak, a feed fix): nothing new was taken
        assert!(t.chunk.markers.is_empty(), "{:?}", t.chunk.markers);
    }

    #[test]
    fn a_frame_without_dragons_is_unknown_not_zero() {
        let mut w = vec![
            wf(0, "in_game", 0, 0, &["ocean"]),
            wf(10, "in_game", 0, 0, &["ocean"]),
            wf(20, "in_game", 0, 0, &["ocean", "cloud"]),
            wf(30, "finished", 0, 0, &["ocean", "cloud"]),
        ];
        w[1]["blueTeam"]["dragons"] = Value::Null;
        w[2]["redTeam"].as_object_mut().unwrap().remove("dragons");
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.teams[0].dragons, [Some(1), None, Some(2), Some(2)]);
        assert_eq!(t.chunk.teams[1].dragons[2], None);
        // only the cloud is new: the ocean isn't counted again after the frame without a list
        let d: Vec<_> = t
            .chunk
            .markers
            .iter()
            .map(|m| m.detail.as_deref())
            .collect();
        assert_eq!(d, [Some("cloud")]);
    }

    #[test]
    fn a_huge_jump_is_a_feed_error() {
        let mut w = vec![
            hand::window_frame(at(0), "in_game", 0),
            hand::window_frame(at(10), "finished", 0),
        ];
        w[1]["blueTeam"]["towers"] = json!(4_294_967_295_i64);
        let t = build(&raws(&w), &[], 10).unwrap();
        assert!(t.chunk.markers.is_empty());
        assert!(
            t.chunk.warnings[0].contains("no markers"),
            "{:?}",
            t.chunk.warnings
        );
    }

    #[test]
    fn a_frame_hours_away_is_refused() {
        let w = vec![
            hand::window_frame(at(0), "in_game", 0),
            hand::window_frame(at(7 * 3600), "in_game", 0),
        ];
        let e = build(&raws(&w), &[], 10).unwrap_err();
        assert!(e.to_string().contains("h from the first one"), "{e}");
        // also live, one frame at a time
        let mut b = Builder::new(10);
        b.push(&raws(&w[..1]), &[]).unwrap();
        assert!(b.push(&raws(&w[1..]), &[]).is_err());
        // 5 h of pauses is still a game (the longest seen, DK-DRX 2024-02-25 game 2: 2:58)
        let w = paused_game(&[
            (0, 10, "in_game"),
            (11, 11, "paused"),
            (5 * 3600, 5 * 3600 + 10, "in_game"),
        ]);
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.duration_s, 21);
    }

    #[test]
    fn a_saved_state_of_another_shape_is_refused() {
        let mut b = Builder::new(10);
        b.push(&raws(&[hand::window_frame(at(0), "in_game", 0)]), &[])
            .unwrap();
        let mut v = serde_json::to_value(b.state()).unwrap();
        v["skills_seen"] = json!([0, 0]);
        let state: LiveState = serde_json::from_value(v.clone()).unwrap();
        assert!(Builder::resume(state).is_err());
        v["skills_seen"] = json!(vec![0; 10]);
        v["step_s"] = json!(0);
        let state: LiveState = serde_json::from_value(v).unwrap();
        assert!(Builder::resume(state).is_err());
    }

    #[test]
    fn red_side_and_inhibitors_barons() {
        let mut w = vec![
            hand::window_frame(at(0), "in_game", 0),
            hand::window_frame(at(10), "finished", 0),
        ];
        w[1]["redTeam"]["inhibitors"] = json!(1);
        w[1]["redTeam"]["barons"] = json!(1);
        let t = build(&raws(&w), &[], 10).unwrap();
        let kinds: Vec<_> = t.chunk.markers.iter().map(|m| (m.kind, m.side)).collect();
        assert_eq!(kinds, [("inhibitor", Side::Red), ("baron", Side::Red)]);
    }

    fn df(s: i64, abilities: &[&str], perks: Option<Value>, items: Value) -> Value {
        let mut f = hand::details_frame(at(s), 1);
        for p in f["participants"].as_array_mut().unwrap() {
            p["abilities"] = json!(abilities);
            p["items"] = items.clone();
            match &perks {
                Some(v) => p["perkMetadata"] = v.clone(),
                None => p["perkMetadata"] = Value::Null,
            }
        }
        f
    }

    #[test]
    fn skills_in_order_with_their_time() {
        let w: Vec<Value> = (0..=40)
            .map(|s| hand::window_frame(at(s), if s == 40 { "finished" } else { "in_game" }, 0))
            .collect();
        let d = vec![
            df(0, &["Q"], None, json!([])),
            df(10, &["Q", "W", "E"], None, json!([])),
            df(20, &["Q"], None, json!([])),
        ];
        let t = build(&raws(&w), &raws(&d), 10).unwrap();
        let p1: Vec<(&str, i64)> = t
            .chunk
            .skills
            .iter()
            .filter(|s| s.participant_id == 1)
            .map(|s| (s.skill.as_str(), s.game_time_s))
            .collect();
        assert_eq!(p1, [("Q", 0), ("W", 10), ("E", 10)]);
        // the list shrinking is logged once per player, nothing removed
        assert_eq!(t.chunk.warnings.len(), 10);
    }

    #[test]
    fn runes_from_the_first_frame_that_has_them() {
        let w: Vec<Value> = (0..=30)
            .map(|s| hand::window_frame(at(s), if s == 30 { "finished" } else { "in_game" }, 0))
            .collect();
        let first = json!({"styleId": 8000, "subStyleId": 8100, "perks": [1, 2]});
        let other = json!({"styleId": 8200, "subStyleId": 8100, "perks": [3]});
        let d = vec![
            df(0, &[], None, json!([])),
            df(10, &[], Some(first), json!([])),
            df(20, &[], Some(other), json!([])),
        ];
        let t = build(&raws(&w), &raws(&d), 10).unwrap();
        assert_eq!(t.chunk.runes.len(), 10);
        assert_eq!(t.chunk.runes[0].style, Some(8000));
        assert_eq!(t.chunk.runes[0].perks, vec![1, 2]);
        assert_eq!(t.chunk.warnings.len(), 10, "{:?}", t.chunk.warnings);
        assert!(t.chunk.warnings[0].contains("runes changed"));
    }

    fn paused_game(states: &[(i64, i64, &str)]) -> Vec<Value> {
        states
            .iter()
            .flat_map(|&(from, to, state)| {
                (from..=to).map(move |s| hand::window_frame(at(s), state, s))
            })
            .collect()
    }

    #[test]
    fn pauses_with_their_game_and_wall_times() {
        // in play 0-100, paused 101-200, in play 201-300, paused 301-310, finished at 311
        let mut w = paused_game(&[
            (0, 100, "in_game"),
            (101, 200, "paused"),
            (201, 300, "in_game"),
            (301, 310, "paused"),
        ]);
        w.push(hand::window_frame(at(311), "finished", 311));
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(
            t.chunk.pauses,
            vec![
                Pause {
                    number: 1,
                    game_time_s: 101,
                    started_at: at(101),
                    ended_at: Some(at(201)),
                    ..Default::default()
                },
                Pause {
                    number: 2,
                    game_time_s: 201,
                    started_at: at(301),
                    ended_at: Some(at(311)),
                    ..Default::default()
                },
            ]
        );
        assert_eq!(t.duration_s, 201);
    }

    #[test]
    fn a_pause_still_open_and_closed_in_a_later_push() {
        let w = paused_game(&[(0, 50, "in_game"), (51, 80, "paused")]);
        let mut b = Builder::new(10);
        let first = b.push(&raws(&w), &[]).unwrap();
        assert_eq!(first.pauses.len(), 1);
        assert_eq!(first.pauses[0].ended_at, None);
        // live: the state goes through JSON between ticks
        let state: LiveState =
            serde_json::from_str(&serde_json::to_string(b.state()).unwrap()).unwrap();
        let mut b = Builder::resume(state).unwrap();
        let w = paused_game(&[(81, 90, "paused"), (91, 120, "in_game")]);
        let second = b.push(&raws(&w), &[]).unwrap();
        assert_eq!(
            second.pauses,
            vec![Pause {
                number: 1,
                game_time_s: 51,
                started_at: at(51),
                ended_at: Some(at(91)),
                ..Default::default()
            }]
        );
        let mut whole = first;
        whole.append(second);
        assert_eq!(whole.pauses.len(), 1);
        assert_eq!(whole.pauses[0].ended_at, Some(at(91)));
    }

    #[test]
    fn a_game_that_starts_paused() {
        let w = paused_game(&[(0, 20, "paused"), (21, 60, "in_game")]);
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.pauses[0].game_time_s, 0);
        assert_eq!(t.chunk.pauses[0].ended_at, Some(at(21)));
    }

    #[test]
    fn items_as_many_as_the_feed_lists() {
        let w: Vec<Value> = (0..=10)
            .map(|s| hand::window_frame(at(s), "in_game", 0))
            .collect();
        // 11 items (seen once, 2024-2026), then 2 control wards as two entries
        let d = vec![
            df(0, &[], None, json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])),
            df(10, &[], None, json!([3364, 2055, 2055])),
        ];
        let t = build(&raws(&w), &raws(&d), 10).unwrap();
        let p = &t.chunk.players[0];
        assert_eq!(p.item_counts, vec![Some(11), Some(3)]);
        assert_eq!(p.items.len(), 14);
        assert_eq!(p.items[10], Some(11));
        assert_eq!(&p.items[11..], &[Some(3364), Some(2055), Some(2055)]);
        assert!(t.chunk.warnings.is_empty());
    }

    #[test]
    fn a_missing_player_is_empty_at_that_point_only() {
        let mut w: Vec<Value> = (0..=20)
            .map(|s| hand::window_frame(at(s), "in_game", 100))
            .collect();
        // participant 3 missing from 5 to 10 s
        for f in &mut w[5..=10] {
            f["blueTeam"]["participants"]
                .as_array_mut()
                .unwrap()
                .remove(2);
        }
        let t = build(&raws(&w), &[], 10).unwrap();
        let p3 = &t.chunk.players[2].gold;
        assert_eq!(p3[0], Some(20));
        assert_eq!(p3[1], None);
        assert_eq!(p3[2], Some(20));
        assert_eq!(t.chunk.players[0].gold[1], Some(20));
    }

    #[test]
    fn bad_participant_ids_are_errors() {
        let mut w = vec![hand::window_frame(at(0), "in_game", 0)];
        w[0]["blueTeam"]["participants"][0]["participantId"] = json!(11);
        assert!(build(&raws(&w), &[], 10).is_err());
        let w = vec![hand::window_frame(at(0), "in_game", 0)];
        let mut d = hand::details_frame(at(0), 1);
        d["participants"][0]["participantId"] = json!(0);
        assert!(build(&raws(&w), &raws(&[d]), 10).is_err());
    }

    #[test]
    fn floats_missing_or_odd() {
        let w = vec![hand::window_frame(at(0), "in_game", 0)];
        let mut d = hand::details_frame(at(0), 1);
        d["participants"][0]["killParticipation"] = json!(1.0);
        d["participants"][1]["killParticipation"] = Value::Null;
        d["participants"][2]
            .as_object_mut()
            .unwrap()
            .remove("killParticipation");
        let t = build(&raws(&w), &raws(&[d]), 10).unwrap();
        assert_eq!(t.chunk.players[0].kill_participation[0], Some(1.0));
        assert_eq!(t.chunk.players[1].kill_participation[0], None);
        assert_eq!(t.chunk.players[2].kill_participation[0], None);
        assert_eq!(t.chunk.players[3].kill_participation[0], Some(0.0));
    }

    #[test]
    fn frames_after_finished_are_skipped() {
        let mut w: Vec<Value> = (0..=20)
            .map(|s| hand::window_frame(at(s), if s == 10 { "finished" } else { "in_game" }, s))
            .collect();
        w.truncate(21);
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.duration_s, 10);
        assert_eq!(t.points, 2);
        assert_eq!(t.chunk.warnings.len(), 1);
    }

    #[test]
    fn same_frames_same_result() {
        let (w, d) = game(77);
        assert_eq!(build(&w, &d, 10).unwrap(), build(&w, &d, 10).unwrap());
    }

    /// Live: 10 s slices one after the other, the state saved as JSON between them, give the
    /// same points as the whole game at once
    #[test]
    fn live_slices_equal_the_whole_game() {
        let (w, d) = game(95);
        let whole = build(&w, &d, 10).unwrap();
        let mut chunk = Chunk::default();
        let mut state: Option<String> = None;
        let mut b;
        for slice in 0..10 {
            b = match &state {
                None => Builder::new(10),
                Some(s) => Builder::resume(serde_json::from_str(s).unwrap()).unwrap(),
            };
            let in_slice = |f: &&RawFrame| f.ts >= at(slice * 10) && f.ts < at(slice * 10 + 10);
            let ws: Vec<RawFrame> = w.iter().filter(in_slice).cloned().collect();
            // the details of a slice can come before or after its windows
            let ds: Vec<RawFrame> = d.iter().filter(in_slice).cloned().collect();
            chunk.append(b.push(&ws, &ds).unwrap());
            // a repeated tick changes nothing
            chunk.append(b.push(&ws, &ds).unwrap());
            state = Some(serde_json::to_string(b.state()).unwrap());
        }
        let mut b = Builder::resume(serde_json::from_str(&state.unwrap()).unwrap()).unwrap();
        chunk.append(b.finish().unwrap());
        assert_eq!(chunk, whole.chunk);
        assert_eq!(b.state().points(), whole.points);
    }

    /// The real game from the fixtures: its first minute, a minute with a dragon, a tower and
    /// a baron, its last minute
    #[test]
    fn real_game_fixture() {
        let (meta, w) = parse_frames(fixture!("feed/frames/window.json")).unwrap();
        let (_, d) = parse_frames(fixture!("feed/frames/details.json")).unwrap();
        assert!(meta.is_some());
        let t = build(&w, &d, 10).unwrap();
        assert!(t.points > 0);
        let kinds: Vec<&str> = t.chunk.markers.iter().map(|m| m.kind).collect();
        for k in ["dragon", "tower", "baron"] {
            assert!(kinds.contains(&k), "{k}: {kinds:?}");
        }
        // the fixture skips most of the game: those are long gaps, not made-up points
        assert!(
            t.chunk.warnings.iter().all(|w| w.starts_with("feed gap")),
            "{:?}",
            t.chunk.warnings
        );
        assert_eq!(
            t.chunk.players.iter().map(|p| p.gold.len()).max(),
            Some(t.points as usize)
        );
        assert!(t.chunk.runes.len() == 10);
    }

    fn chronobreak(name: &str) -> (Vec<RawFrame>, Vec<RawFrame>) {
        let w = match name {
            "g2" => fixture!("feed/frames/chronobreak_window.json"),
            _ => fixture!("feed/frames/chronobreak2_window.json"),
        };
        let d = match name {
            "g2" => fixture!("feed/frames/chronobreak_details.json"),
            _ => fixture!("feed/frames/chronobreak2_details.json"),
        };
        (parse_frames(w).unwrap().1, parse_frames(d).unwrap().1)
    }

    /// A real chronobreak (G2-MKOI, LEC 2025-09-14 game 3): paused at 26:27, 16 min, frames of
    /// a server starting with everything at 0, then the game 8 s back and the feed's counts
    /// started again at 0. Cut: the first frame, 30 s before the pause, 20 s after
    #[test]
    fn real_chronobreak() {
        let (w, d) = chronobreak("g2");
        let t = build(&w, &d, 10).unwrap();
        assert_eq!(t.chunk.pauses.len(), 1, "{:?}", t.chunk.pauses);
        let p = &t.chunk.pauses[0];
        assert_eq!(p.game_time_s, 1587);
        assert_eq!(p.rollback_s, Some(8));
        assert!(p.counters_reset);
        let u = p.undone.as_ref().unwrap();
        // blue lost a kill and 805 gold, red 231 gold
        assert_eq!((u.teams[0].kills, u.teams[0].gold), (1, 805));
        assert_eq!((u.teams[1].kills, u.teams[1].gold), (0, 231));
        assert_eq!(u.players.len(), 10);
        // the zero frames make no points, the counts go on from the snapshot (blue had 4
        // kills 8 s before the pause, 3 towers)
        let blue = &t.chunk.teams[0];
        let n = blue.kills.len();
        assert_eq!(blue.kills[n - 1], Some(4));
        assert_eq!(blue.towers[n - 1], Some(3));
        assert!(blue.gold.iter().skip(1).flatten().all(|g| *g > 0));
        // 26:19 (the snapshot) + 19.6 s of game after the restart
        assert_eq!(t.duration_s, 1598);
        // after the restart the feed's item lists aren't the inventory
        assert_eq!(t.chunk.players[0].item_counts[n - 1], None);
        assert!(
            t.chunk
                .warnings
                .iter()
                .any(|w| w.contains("back 8 s to game time 26:19")),
            "{:?}",
            t.chunk.warnings
        );
    }

    /// The second one found (FUR-GAM, 2025-06-28 game 5): 5 s back, counts started again
    #[test]
    fn real_chronobreak_2() {
        let (w, d) = chronobreak("fur");
        let t = build(&w, &d, 10).unwrap();
        let p = &t.chunk.pauses[0];
        assert_eq!(p.rollback_s, Some(5));
        assert!(p.counters_reset);
    }

    /// Live (frames 10 s at a time, no history): no rollback, but the counts keep adding up
    /// and the zero frames are skipped. The archive fixes the rest after the game
    #[test]
    fn real_chronobreak_live() {
        let (w, d) = chronobreak("g2");
        let mut b = Builder::new(10);
        let mut chunk = Chunk::default();
        let mut i = 0;
        while i < w.len() {
            let end = w[i].ts + TimeDelta::seconds(10);
            let j = w.iter().position(|f| f.ts >= end).unwrap_or(w.len());
            let dd: Vec<RawFrame> = d
                .iter()
                .filter(|f| f.ts >= w[i].ts && f.ts < end)
                .cloned()
                .collect();
            chunk.append(b.push(&w[i..j], &dd).unwrap());
            let state: LiveState =
                serde_json::from_str(&serde_json::to_string(b.state()).unwrap()).unwrap();
            b = Builder::resume(state).unwrap();
            i = j;
        }
        let p = &chunk.pauses[0];
        assert_eq!(p.rollback_s, None);
        assert!(p.counters_reset);
        assert!(
            chunk
                .warnings
                .iter()
                .any(|w| w.contains("counted on from the pause")),
            "{:?}",
            chunk.warnings
        );
        let kills: Vec<i64> = chunk.teams[0].kills.iter().flatten().copied().collect();
        assert!(kills.windows(2).all(|k| k[1] >= k[0]), "{kills:?}");
        assert!(chunk.teams[0].gold.iter().skip(1).flatten().all(|g| *g > 0));
    }

    /// Hand-made: a tower 5 s before the pause, the game goes back to before it (the count 0 is
    /// right then, not a restart). The tower's marker stays, marked undone; taken again after,
    /// it's a new marker
    #[test]
    fn undone_marker_kept_and_taken_again() {
        let frame = |s: i64, state: &str, gold: i64, towers: i64| {
            let mut f = hand::window_frame(at(s), state, gold);
            f["blueTeam"]["towers"] = json!(towers);
            f
        };
        let mut w: Vec<Value> = (0..=200)
            .map(|s| frame(s, "in_game", 1000 + s * 10, 0))
            .collect();
        w.extend((201..=205).map(|s| frame(s, "in_game", 1000 + s * 10, 1)));
        w.push(frame(206, "paused", 1000 + 206 * 10, 1));
        // back at the gold of 195 s, the feed's tower count at 0
        w.extend((300..=320).map(|s| frame(s, "in_game", 1000 + (s - 105) * 10, 0)));
        w.push(frame(321, "in_game", 1000 + 216 * 10, 1));
        let t = build(&raws(&w), &[], 10).unwrap();
        let p = &t.chunk.pauses[0];
        assert_eq!(p.rollback_s, Some(11));
        assert!(!p.counters_reset);
        assert_eq!(p.undone.as_ref().unwrap().teams[0].towers, 1);
        let towers: Vec<_> = t
            .chunk
            .markers
            .iter()
            .filter(|m| m.kind == "tower")
            .map(|m| (m.game_time_s, m.undone_by_pause))
            .collect();
        assert_eq!(towers, vec![(201, Some(1)), (216, None)]);
        // the series: 1 tower at the end (the undone one doesn't count)
        assert_eq!(t.chunk.teams[0].towers.last(), Some(&Some(1)));
    }

    /// A frame after a pause without one team's gold isn't a chronobreak (no gold to compare)
    #[test]
    fn a_frame_without_gold_after_a_pause_is_no_chronobreak() {
        let mut w = paused_game(&[(0, 100, "in_game"), (101, 110, "paused")]);
        let mut odd = hand::window_frame(at(111), "in_game", 111);
        odd["redTeam"].as_object_mut().unwrap().remove("totalGold");
        w.push(odd);
        w.extend(paused_game(&[(112, 130, "in_game")]));
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.pauses[0].rollback_s, None);
        assert!(!t.chunk.pauses[0].counters_reset);
        assert_eq!(t.duration_s, 120);
    }

    /// A chronobreak before any kill or objective: only the details show the restart (wards
    /// back to 0, no skills). Wards and skills go on from the snapshot, items are unknown
    #[test]
    fn a_chronobreak_before_first_blood() {
        let mut w: Vec<Value> = (0..=200)
            .map(|s| hand::window_frame(at(s), "in_game", 1000 + s * 10))
            .collect();
        w.push(hand::window_frame(at(201), "paused", 1000 + 201 * 10));
        // back at the gold of 190 s
        w.extend((300..=330).map(|s| hand::window_frame(at(s), "in_game", 1000 + (s - 110) * 10)));
        let details = |s: i64, abilities: &[&str], wards: i64| {
            let mut f = df(s, abilities, None, json!([1055, 2003]));
            for p in f["participants"].as_array_mut().unwrap() {
                p["wardsPlaced"] = json!(wards);
            }
            f
        };
        let mut d: Vec<Value> = (0..=200)
            .step_by(10)
            .map(|s| details(s, &["Q", "W"], 3))
            .collect();
        d.extend((300..=320).step_by(10).map(|s| details(s, &[], 0)));
        d.push(details(330, &["E"], 1));
        let t = build(&raws(&w), &raws(&d), 10).unwrap();
        let p = &t.chunk.pauses[0];
        assert!(p.counters_reset, "{:?}", t.chunk.warnings);
        assert_eq!(p.rollback_s, Some(11));
        let p1 = &t.chunk.players[0];
        let n = p1.wards_placed.len();
        assert_eq!(p1.wards_placed[n - 1], Some(4));
        assert_eq!(p1.item_counts[n - 1], None);
        let skills: Vec<&str> = t
            .chunk
            .skills
            .iter()
            .filter(|s| s.participant_id == 1)
            .map(|s| s.skill.as_str())
            .collect();
        assert_eq!(skills, vec!["Q", "W", "E"]);
    }

    /// A marker taken after the snapshot but in the same second is undone too
    #[test]
    fn undone_in_the_snapshots_second() {
        let frame = |ms: i64, state: &str, gold: i64, towers: i64| {
            let mut f = hand::window_frame(at(0) + TimeDelta::milliseconds(ms), state, gold);
            f["blueTeam"]["towers"] = json!(towers);
            f
        };
        // gold 1000 + 1 every 100 ms, a tower at 200.5 s, paused at 201.1 s
        let mut w: Vec<Value> = (0..=2010)
            .map(|i| frame(i * 100, "in_game", 1000 + i, i64::from(i >= 2005)))
            .collect();
        w.push(frame(201_100, "paused", 3011, 1));
        // back at gold 3000: the snapshot is 200.0 s, the tower's second
        w.extend((0..50).map(|i| frame(300_000 + i * 100, "in_game", 3000 + i / 10, 0)));
        let t = build(&raws(&w), &[], 10).unwrap();
        let towers: Vec<_> = t
            .chunk
            .markers
            .iter()
            .filter(|m| m.kind == "tower")
            .map(|m| (m.game_time_s, m.undone_by_pause))
            .collect();
        assert_eq!(towers, vec![(200, Some(1))]);
    }

    /// `Chunk::rewind` cuts every column: a new column left out of it would be caught here
    #[test]
    fn rewind_cuts_every_column() {
        let (w, d) = game(95);
        let mut c = build(&w, &d, 10).unwrap().chunk;
        c.rewind(4, 35_000, 1);
        assert_eq!(c.wall_time.len(), 4);
        let cols = c
            .teams
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .chain(c.players.iter().map(|p| serde_json::to_value(p).unwrap()));
        for v in cols {
            for (k, a) in v.as_object().unwrap() {
                if k != "items"
                    && let Some(a) = a.as_array()
                {
                    assert_eq!(a.len(), 4, "{k}");
                }
            }
        }
        for p in &c.players {
            let n: i64 = p.item_counts.iter().flatten().sum();
            assert_eq!(p.items.len() as i64, n);
        }
    }

    /// The feed sends two frames with the same time, in game then paused (FUR-GAM 2025-06-28
    /// game 5): the last one counts, the pause is seen
    #[test]
    fn same_time_in_game_then_paused() {
        let mut w = paused_game(&[(0, 100, "in_game")]);
        w.push(hand::window_frame(at(100), "paused", 100));
        w.extend(paused_game(&[(160, 200, "in_game")]));
        let t = build(&raws(&w), &[], 10).unwrap();
        assert_eq!(t.chunk.pauses.len(), 1);
        assert_eq!(t.chunk.pauses[0].started_at, at(100));
        assert_eq!(t.duration_s, 140);
    }

    /// A real pause (Worlds, 2026-10-29): 20 s in game, 7 min paused with frames minutes
    /// apart, 20 s in game. Only the 40 s count, the minutes without frames aren't a gap
    #[test]
    fn real_pause_fixture() {
        let (_, w) = parse_frames(fixture!("feed/frames/paused_window.json")).unwrap();
        let (_, d) = parse_frames(fixture!("feed/frames/paused_details.json")).unwrap();
        assert!(w.iter().any(|f| f.state.as_deref() == Some("paused")));
        let t = build(&w, &d, 10).unwrap();
        // 21:16:27.430 → 21:16:47.328 and 21:23:49.232 → 21:24:08.607
        assert_eq!(t.duration_s, 39);
        assert_eq!(t.points, 5);
        assert!(t.chunk.warnings.is_empty(), "{:?}", t.chunk.warnings);
        assert!(t.chunk.teams[0].gold.iter().all(Option::is_some));
        // point 2 (game time 20 s) is the frame play resumed on (game time 19.9 s)
        let resumed = "2026-10-29T21:23:49.232Z".parse::<DateTime<Utc>>().unwrap();
        assert!(t.chunk.wall_time[2] <= resumed.naive_utc());
        assert!(t.chunk.wall_time[3] > resumed.naive_utc());
    }
}
