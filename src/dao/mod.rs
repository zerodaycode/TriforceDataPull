//! The data access layer

use std::fmt::Error;

use canyon_sql::crud::CrudOperations;
use crate::data_pull::serde_models::Leagues;
use self::models::leagues::League;
use color_eyre::{eyre::Context, Result};
mod models;



pub async fn bulk_leagues_in_database(leagues: Leagues) -> Result<()> {
    let mut db_leagues = leagues.leagues.into_iter()
        .map(|serde_league| League::from(serde_league))
        .collect::<Vec<_>>();
    
    League::multi_insert(
        &mut db_leagues.iter_mut()
            .map(|e| e)
            .collect::<Vec<&mut League>>()
    ).await.map_err(|e| 
        color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap())
    )
}