pub mod details;
pub mod events;
pub mod reference;

use color_eyre::Result;

use crate::{
    client::{adapters::ext_id, gql::ops::watch_vod::GameTeam},
    dao::v2::rows::GameSides,
};

/// The two teams of a game, one blue and one red (two different teams), or `None`
pub fn game_sides(game_id: &str, teams: &[GameTeam]) -> Option<GameSides> {
    let sides: Vec<(i64, &'static str)> = teams
        .iter()
        .filter_map(|t| {
            let side = match t.side.as_deref() {
                Some("blue") => "blue",
                Some("red") => "red",
                _ => return None,
            };
            Some((ext_id(&t.id).ok()?, side))
        })
        .collect();
    if sides.len() != 2 || sides[0].1 == sides[1].1 || sides[0].0 == sides[1].0 {
        return None;
    }
    Some(GameSides {
        game_ext_id: ext_id(game_id).ok()?,
        sides,
    })
}

/// The sides with the match's teams, same rule as the feed (`details::match_teams`): a team the
/// match doesn't list takes the match's other team. (team in the answer, match team) when it did
pub fn remap_sides(s: &mut GameSides, event_teams: &[i64]) -> Result<Option<(i64, i64)>> {
    let (ids, remapped) =
        details::match_teams(s.game_ext_id, [s.sides[0].0, s.sides[1].0], event_teams)?;
    s.sides[0].0 = ids[0];
    s.sides[1].0 = ids[1];
    Ok(remapped)
}
