//! The data access layer

use std::fmt::Error;

use self::models::{leagues::League, tournaments::Tournament};
use crate::{data_pull::serde_models::Leagues, service::OurTournaments};
use canyon_sql::crud::CrudOperations;
use color_eyre::Result;
mod models;

#[derive(Debug, Default)]
pub struct DatabaseOps {
    pub leagues: Vec<League>,
    pub tournaments: Vec<Tournament>,
}

impl DatabaseOps {
    pub async fn bulk_leagues_in_database(&mut self, leagues: &Leagues) -> Result<()> {
        let mut db_leagues = leagues.leagues.iter().map(League::from).collect::<Vec<_>>();

        League::multi_insert(&mut db_leagues.iter_mut().collect::<Vec<&mut League>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;

        self.leagues = db_leagues;

        Ok(())
    }

    pub async fn bulk_tournaments_in_database(
        &mut self,
        tournaments: &OurTournaments,
    ) -> Result<()> {
        let mut db_tournaments = tournaments
            .iter()
            .map(|serde_tournament| {
                let mut t = Tournament::from(serde_tournament);
                // t.league
                t.league = self
                    .leagues
                    .iter()
                    .find(|league| serde_tournament.league_id.0 == league.ext_id)
                    .map(|league| league.id)
                    .unwrap_or_default();
                t
            })
            .collect::<Vec<_>>();

        Tournament::multi_insert(&mut db_tournaments.iter_mut().collect::<Vec<&mut Tournament>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;

        self.tournaments = db_tournaments;

        Ok(())
    }
}
