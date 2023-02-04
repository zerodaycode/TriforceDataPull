//! The data access layer

use canyon_sql::crud::CrudOperations;

use crate::data_pull::serde_models::Leagues;

use self::models::leagues::League;

mod models;

pub async fn bulk_leagues_in_database(leagues: Leagues) -> Result<(), &'static dyn std::error::Error> {
    // for league in leagues.leagues {
    //     let mut v = League::from(league);
    //     v.insert().await;
    // }
    let mut db_leagues = leagues.leagues.into_iter()
        .map(|serde_league| League::from(serde_league))
        .collect::<Vec<_>>();
    
    match League::multi_insert(
        &mut db_leagues.iter_mut()
            .map(|e| e)
            .collect::<Vec<&mut League>>()
    ).await {
        Ok(_) => println!("Leagues inserted succesfully"),
        Err(e) => println!("Error inserting the leagues in the database: {e:?}"),
    }
    Ok(())
}