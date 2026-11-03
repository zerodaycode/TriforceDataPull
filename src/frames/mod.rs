//! Game frames from the live feed: every frame as the feed sent it (the archive), and the
//! series built from them (one point every 10 s of game time)

pub mod box_score;
pub mod pack;
pub mod timeline;

use chrono::{DateTime, Utc};
use color_eyre::{Result, eyre::WrapErr};
use serde::Deserialize;
use serde_json::value::RawValue;

/// One frame, kept byte for byte, with what we need to sort and walk it
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub ts: DateTime<Utc>,
    /// `in_game`, `paused`, `finished` (`window` only)
    pub state: Option<String>,
    pub raw: Box<RawValue>,
}

impl PartialEq for RawFrame {
    fn eq(&self, other: &Self) -> bool {
        self.ts == other.ts && self.state == other.state && self.raw.get() == other.raw.get()
    }
}

#[derive(Deserialize)]
struct Body {
    #[serde(rename = "gameMetadata")]
    game_metadata: Option<Box<RawValue>>,
    frames: Option<Vec<Box<RawValue>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Head {
    rfc460_timestamp: DateTime<Utc>,
    game_state: Option<String>,
}

impl RawFrame {
    pub fn new(raw: Box<RawValue>) -> Result<Self> {
        let head: Head = serde_json::from_str(raw.get()).wrap_err("feed frame")?;
        Ok(Self {
            ts: head.rfc460_timestamp,
            state: head.game_state,
            raw,
        })
    }
}

/// A `window` or `details` answer: its `gameMetadata` (only `window` has one) and frames
pub fn parse_frames(body: &str) -> Result<(Option<Box<RawValue>>, Vec<RawFrame>)> {
    let parsed: Body = serde_json::from_str(body).wrap_err("feed answer")?;
    let frames = parsed
        .frames
        .unwrap_or_default()
        .into_iter()
        .map(RawFrame::new)
        .collect::<Result<Vec<_>>>()?;
    Ok((parsed.game_metadata, frames))
}

#[cfg(test)]
pub(crate) mod hand {
    //! Hand-made feed answers for tests: a game of `seconds` with a frame every second
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use serde_json::{Value, json};

    pub fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 31, 23, 5, 7).unwrap() + TimeDelta::milliseconds(91)
    }

    pub fn ts(t: DateTime<Utc>) -> String {
        t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    pub fn metadata() -> Value {
        let players = |from: i32| -> Vec<Value> {
            (from..from + 5)
                .map(|p| json!({"participantId": p, "esportsPlayerId": format!("{}", 1000 + p), "summonerName": format!("P{p}"), "championId": "Ahri", "role": "mid"}))
                .collect()
        };
        json!({
            "patchVersion": "16.20.1",
            "blueTeamMetadata": {"esportsTeamId": "-402", "participantMetadata": players(1)},
            "redTeamMetadata": {"esportsTeamId": "-403", "participantMetadata": players(6)},
        })
    }

    pub fn team(gold: i64, towers: i32, dragons: &[&str]) -> Value {
        json!({"totalGold": gold, "inhibitors": 0, "towers": towers, "barons": 0, "totalKills": 0, "dragons": dragons,
            "participants": (0..5).map(|i| json!({"participantId": i + 1, "totalGold": gold / 5, "level": 1, "kills": 0, "deaths": 0, "assists": 0, "creepScore": 0, "currentHealth": 500, "maxHealth": 600})).collect::<Vec<_>>()})
    }

    pub fn window_frame(t: DateTime<Utc>, state: &str, gold: i64) -> Value {
        let mut red = team(gold, 0, &[]);
        // red players are 6-10
        for (i, p) in red["participants"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            p["participantId"] = json!(i + 6);
        }
        json!({"rfc460Timestamp": ts(t), "gameState": state, "blueTeam": team(gold, 0, &[]), "redTeam": red})
    }

    pub fn details_frame(t: DateTime<Utc>, level: i32) -> Value {
        json!({"rfc460Timestamp": ts(t), "participants": (1..=10).map(|p| json!({
            "participantId": p, "level": level, "kills": 0, "deaths": 0, "assists": 0, "totalGoldEarned": 500,
            "creepScore": 0, "killParticipation": 0.0, "championDamageShare": 0.1, "wardsPlaced": 0,
            "wardsDestroyed": 0, "attackDamage": 60, "abilityPower": 0, "criticalChance": 0.0,
            "attackSpeed": 100, "lifeSteal": 0, "armor": 30, "magicResistance": 30, "tenacity": 0.0,
            "items": [1055, 2003], "perkMetadata": {"styleId": 8000, "subStyleId": 8100, "perks": [8005, 9111]},
            "abilities": ["Q"]})).collect::<Vec<_>>()})
    }

    /// `(window, details)` frames, one a second, the last one `finished` when asked
    pub fn game(seconds: i64, finished: bool) -> (Vec<Value>, Vec<Value>) {
        let mut w = Vec::new();
        let mut d = Vec::new();
        for s in 0..=seconds {
            let t = start() + TimeDelta::seconds(s);
            let state = if finished && s == seconds {
                "finished"
            } else {
                "in_game"
            };
            w.push(window_frame(t, state, 2500 + s * 10));
            d.push(details_frame(t, 1 + (s / 60) as i32));
        }
        (w, d)
    }

    pub fn raw(v: &Value) -> super::RawFrame {
        super::RawFrame::new(serde_json::value::RawValue::from_string(v.to_string()).unwrap())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_keep_their_bytes_and_unknown_fields() {
        let body = r#"{"esportsGameId":"1","gameMetadata":{"x":1},"frames":[{"rfc460Timestamp":"2026-10-31T23:05:07.091Z","gameState":"in_game","new":  [1,2]}]}"#;
        let (meta, frames) = parse_frames(body).unwrap();
        assert_eq!(meta.unwrap().get(), r#"{"x":1}"#);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].state.as_deref(), Some("in_game"));
        assert!(frames[0].raw.get().contains(r#""new":  [1,2]"#));
    }

    #[test]
    fn null_frames_and_no_metadata() {
        let (meta, frames) = parse_frames(r#"{"frames":null}"#).unwrap();
        assert!(meta.is_none() && frames.is_empty());
        let (meta, _) = parse_frames(r#"{"gameMetadata":null,"frames":[]}"#).unwrap();
        assert!(meta.is_none());
        assert!(parse_frames(r#"{"frames":[{"gameState":"in_game"}]}"#).is_err());
        assert!(parse_frames("not json").is_err());
    }
}
