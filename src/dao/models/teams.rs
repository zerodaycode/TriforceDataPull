use canyon_sql::macros::*;
use serde::Serialize;

use super::leagues::League;
use crate::client::legacy::models as api;

#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity]
pub struct Team {
    #[primary_key]
    id: i32,
    ext_id: i64,
    name: String,
    slug: String,
    code: String,
    image_url: String,
    alt_image_url: Option<String>,
    bg_image_url: Option<String>,
    #[foreign_key(table = "league", column = "id")]
    home_league: Option<i64>,
}

impl From<&api::Team> for Team {
    fn from(value: &api::Team) -> Self {
        Self {
            id: Default::default(),
            ext_id: value.id.into(),
            name: value.name.clone(),
            slug: value.slug.clone(),
            code: value.code.clone(),
            image_url: value.image.clone(),
            alt_image_url: value.alternative_image.clone(),
            bg_image_url: value.background_image.clone(),
            home_league: None,
        }
    }
}

impl Team {
    /// The new API has no `alternativeImage`: keep the one we have instead of writing NULL (only used with the new API)
    pub fn keep_alt_image_from(&mut self, stored: &Team) {
        if self.alt_image_url.is_none() {
            self.alt_image_url.clone_from(&stored.alt_image_url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Team;

    fn team(alt: Option<&str>) -> Team {
        Team {
            id: 1,
            ext_id: 2,
            name: "T1".into(),
            slug: "t1".into(),
            code: "T1".into(),
            image_url: "img".into(),
            alt_image_url: alt.map(String::from),
            bg_image_url: None,
            home_league: None,
        }
    }

    #[test]
    fn keeps_the_stored_alt_image() {
        let mut fetched = team(None);
        fetched.keep_alt_image_from(&team(Some("alt")));
        assert_eq!(fetched.alt_image_url.as_deref(), Some("alt"));
        // a new one from the old API wins
        let mut fetched = team(Some("new"));
        fetched.keep_alt_image_from(&team(Some("alt")));
        assert_eq!(fetched.alt_image_url.as_deref(), Some("new"));
    }
}
