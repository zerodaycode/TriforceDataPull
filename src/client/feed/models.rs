//! `feed.lolesports.com/livestats/v1` answers, only what we store

use chrono::{DateTime, Utc};
use color_eyre::{Result, eyre::WrapErr};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub esports_game_id: String,
    pub game_metadata: Option<GameMetadata>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub frames: Vec<WindowFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMetadata {
    pub patch_version: Option<String>,
    pub blue_team_metadata: TeamMeta,
    pub red_team_metadata: TeamMeta,
}

impl GameMetadata {
    /// (blue, red) team ext ids as the feed has them (not mapped to the match's yet)
    pub fn team_ext_ids(&self) -> Result<(i64, i64)> {
        let id = |s: &str| {
            s.parse::<i64>()
                .wrap_err_with(|| format!("team id {s:?} in gameMetadata"))
        };
        Ok((
            id(&self.blue_team_metadata.esports_team_id)?,
            id(&self.red_team_metadata.esports_team_id)?,
        ))
    }

    pub fn parse(raw: &serde_json::value::RawValue) -> Result<Self> {
        serde_json::from_str(raw.get()).wrap_err("gameMetadata")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMeta {
    pub esports_team_id: String,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub participant_metadata: Vec<ParticipantMeta>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantMeta {
    pub participant_id: i32,
    /// Missing or `"-"` for players without an esports id (national teams, some subs)
    pub esports_player_id: Option<String>,
    #[serde(default)]
    pub summoner_name: String,
    #[serde(default)]
    pub champion_id: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowFrame {
    pub rfc460_timestamp: DateTime<Utc>,
    /// `in_game`, `paused` or `finished`
    pub game_state: Option<String>,
    pub blue_team: TeamFrame,
    pub red_team: TeamFrame,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamFrame {
    pub total_gold: Option<i64>,
    pub inhibitors: Option<i32>,
    pub towers: Option<i32>,
    pub barons: Option<i32>,
    pub total_kills: Option<i32>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub dragons: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Details {
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub frames: Vec<DetailsFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailsFrame {
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub participants: Vec<PlayerFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerFrame {
    pub participant_id: i32,
    pub level: Option<i32>,
    pub kills: Option<i32>,
    pub deaths: Option<i32>,
    pub assists: Option<i32>,
    pub total_gold_earned: Option<i64>,
    pub creep_score: Option<i32>,
    pub champion_damage_share: Option<f64>,
    pub wards_placed: Option<i32>,
    pub wards_destroyed: Option<i32>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub items: Vec<i64>,
    pub perk_metadata: Option<Perks>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub abilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Perks {
    pub style_id: Option<i32>,
    pub sub_style_id: Option<i32>,
    #[serde(default, deserialize_with = "crate::client::null_as_empty")]
    pub perks: Vec<i64>,
}
