//! What the v2 DAO takes. Adapters build these from the API, the DAO never sees API types

use canyon_sql::date_time::NaiveDateTime;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReferenceRows {
    pub leagues: Vec<LeagueRow>,
    pub seasons: Vec<SeasonRow>,
    pub splits: Vec<i64>,
    pub tournaments: Vec<TournamentRow>,
    pub stages: Vec<StageRow>,
    pub teams: Vec<TeamRow>,
    pub players: Vec<PlayerRow>,
    /// (team ext_id, player ext_id), the whole current roster
    pub team_players: Vec<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueRow {
    pub ext_id: i64,
    pub slug: String,
    pub image_url: String,
    pub dark_image_url: Option<String>,
    pub texts: Vec<LeagueText>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueText {
    pub locale: String,
    pub name: String,
    pub region: String,
    pub position: Option<i32>,
}

/// A translated name
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub locale: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonRow {
    pub ext_id: i64,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TournamentRow {
    pub ext_id: i64,
    pub league_ext_id: i64,
    pub season_ext_id: Option<i64>,
    pub split_ext_id: Option<i64>,
    pub slug: String,
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageRow {
    pub ext_id: i64,
    pub tournament_ext_id: i64,
    pub texts: Vec<Text>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamRow {
    pub ext_id: i64,
    pub slug: String,
    pub name: String,
    pub code: String,
    pub image_url: String,
    pub bg_image_url: Option<String>,
    pub home_league_ext_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRow {
    pub ext_id: i64,
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub image_url: Option<String>,
    pub role: String,
}

/// One `scheduleEvents` page in one locale
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventRows {
    pub events: Vec<EventRow>,
    /// Leagues and teams seen in the events, only added when we don't have them
    pub league_stubs: Vec<LeagueStub>,
    pub team_stubs: Vec<TeamStub>,
    /// Events left out by the adapter (bad id, unknown type)
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub ext_id: i64,
    /// `match` or `show`
    pub kind: &'static str,
    pub league_ext_id: i64,
    pub tournament_ext_id: Option<i64>,
    pub start_time: Option<NaiveDateTime>,
    pub state: String,
    pub strategy_type: Option<String>,
    pub strategy_count: Option<i32>,
    pub match_type: Option<String>,
    /// In the locale of the page
    pub block_name: Option<String>,
    pub teams: Vec<EventTeamRow>,
    pub games: Vec<GameRow>,
    pub streams: Vec<StreamRow>,
}

/// A slot of a match: its team, wins and outcome always come from the same answer
#[derive(Debug, Clone, PartialEq)]
pub struct EventTeamRow {
    /// 1 or 2, order of the answer (game 1's blue team first once sides are picked)
    pub slot: i32,
    /// `None` = TBD
    pub team_ext_id: Option<i64>,
    pub game_wins: Option<i32>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameRow {
    pub ext_id: i64,
    pub number: i32,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamRow {
    pub provider: String,
    pub parameter: String,
    pub locale: String,
    pub english_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeagueStub {
    pub ext_id: i64,
    pub slug: String,
    pub name: String,
    pub image_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamStub {
    pub ext_id: i64,
    pub name: String,
    pub code: String,
    pub image_url: String,
}

/// Values the API sends that the CHECKs in `schema.sql` accept (a test keeps them equal).
/// An event with any other value is skipped by the adapter, so it can't fail a whole page
pub const EVENT_STATES: &[&str] = &["unstarted", "inProgress", "completed"];
pub const GAME_STATES: &[&str] = &["unstarted", "inProgress", "completed", "unneeded"];
/// `v2_game.frames`, checked against schema.sql
#[cfg(test)]
pub const FRAMES_STATES: &[&str] = &["live", "done", "no_feed"];
pub const STRATEGIES: &[&str] = &["bestOf", "playAll"];
pub const MATCH_TYPES: &[&str] = &["normal", "tiebreaker"];
/// `tie`: a `playAll 2` ending 1-1
pub const OUTCOMES: &[&str] = &["win", "loss", "tie"];

/// How far an event got
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Progress {
    pub state_rank: i32,
    pub wins: i64,
    pub outcomes: i64,
}

impl Progress {
    /// Any part lower than what we have: an old copy, skipped (ADR 0005). State, wins and
    /// outcomes never go down in a real answer
    pub fn went_back_from(&self, stored: &Progress) -> bool {
        self.state_rank < stored.state_rank
            || self.wins < stored.wins
            || self.outcomes < stored.outcomes
    }
}

/// `unstarted` 0, `inProgress` 1, `completed` (and a game's `unneeded`) 2. Same as
/// `v2_state_rank()` in `schema.sql`
pub fn state_rank(state: &str) -> i32 {
    match state {
        "inProgress" => 1,
        "completed" | "unneeded" => 2,
        _ => 0,
    }
}

impl EventRow {
    pub fn progress(&self) -> Progress {
        Progress {
            state_rank: state_rank(&self.state),
            wins: self
                .teams
                .iter()
                .map(|t| i64::from(t.game_wins.unwrap_or(0)))
                .sum(),
            outcomes: self.teams.iter().filter(|t| t.outcome.is_some()).count() as i64,
        }
    }
}

/// One finished game from the feed: sides, lineup and the last frame
#[derive(Debug, Clone, PartialEq)]
pub struct GameDetailsRow {
    pub game_ext_id: i64,
    pub patch: Option<String>,
    /// First and last feed frame
    pub start_time: Option<NaiveDateTime>,
    pub end_time: Option<NaiveDateTime>,
    pub teams: Vec<GameTeamRow>,
    pub players: Vec<GamePlayerRow>,
    /// Players the feed has, added only when we don't have them
    pub player_stubs: Vec<PlayerStub>,
    /// (feed team, match team) when the feed had a team the match doesn't list and it took the
    /// match's other team (a spot that changed hands during the season)
    pub remapped: Option<(i64, i64)>,
}

impl GameDetailsRow {
    /// Sides and lineups only: the numbers of a feed that ended paused aren't the box score
    /// (the frames write it)
    pub fn without_numbers(mut self) -> Self {
        self.end_time = None;
        for t in &mut self.teams {
            (t.gold, t.kills, t.towers, t.inhibitors, t.barons, t.dragons) =
                (None, None, None, None, None, None);
        }
        for p in &mut self.players {
            (p.level, p.kills, p.deaths, p.assists, p.cs) = (None, None, None, None, None);
            (p.wards_placed, p.wards_destroyed, p.damage_share_bp) = (None, None, None);
            p.gold = None;
            (p.items, p.runes, p.skill_order) = (None, None, None);
            (p.rune_style, p.rune_sub_style) = (None, None);
        }
        self
    }
}

/// A game being played, from the live feed
#[derive(Debug, Clone, PartialEq)]
pub struct LiveGameRow {
    pub game_ext_id: i64,
    /// First frame
    pub start_time: NaiveDateTime,
    /// Last frame, once the feed says `finished`
    pub end_time: Option<NaiveDateTime>,
    /// Blue then red, totals right now
    pub teams: Vec<GameTeamRow>,
}

/// A team in a game: its side and its totals in the last frame
#[derive(Debug, Clone, PartialEq)]
pub struct GameTeamRow {
    pub team_ext_id: i64,
    /// `blue` or `red`
    pub side: &'static str,
    pub gold: Option<i64>,
    pub kills: Option<i32>,
    pub towers: Option<i32>,
    pub inhibitors: Option<i32>,
    pub barons: Option<i32>,
    /// `infernal,ocean`
    pub dragons: Option<String>,
}

/// A player in a game: champion, role (of that game) and the last frame
#[derive(Debug, Clone, PartialEq)]
pub struct GamePlayerRow {
    pub team_ext_id: i64,
    /// `None`: the feed has no esports id for this player (missing or `"-"`)
    pub player_ext_id: Option<i64>,
    /// As the feed sends it, with the team code (`"FUR Guigo"`)
    pub summoner_name: String,
    /// 1-5 blue, 6-10 red
    pub participant_id: i32,
    pub role: String,
    pub champion: String,
    pub level: Option<i32>,
    pub kills: Option<i32>,
    pub deaths: Option<i32>,
    pub assists: Option<i32>,
    pub gold: Option<i64>,
    pub cs: Option<i32>,
    pub wards_placed: Option<i32>,
    pub wards_destroyed: Option<i32>,
    /// Share of the team's damage to champions × 10000 (`0.2144` → `2144`)
    pub damage_share_bp: Option<i32>,
    /// Item ids, `1086,3158`
    pub items: Option<String>,
    pub rune_style: Option<i32>,
    pub rune_sub_style: Option<i32>,
    /// Rune ids, `8437,8401`
    pub runes: Option<String>,
    /// `W,E,Q,…`
    pub skill_order: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerStub {
    pub ext_id: i64,
    pub handle: String,
    pub role: String,
}

/// Blue and red team of one game, from `watchVodQuery` (finished games without feed) or
/// `watchLiveQuery`
#[derive(Debug, Clone, PartialEq)]
pub struct GameSides {
    pub game_ext_id: i64,
    /// (team ext_id, `blue`/`red`)
    pub sides: Vec<(i64, &'static str)>,
}
