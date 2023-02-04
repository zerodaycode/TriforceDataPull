mod data_pull;
mod service;
mod utils;
mod dao;

use canyon_sql;
use color_eyre::{eyre::Context, Result};

#[canyon_sql::main]
fn main() -> Result<()> {
    color_eyre::install()?;
    let mut data_pull = service::DataPull::new().await;
    data_pull.fetch_leagues().await;
    // data_pull.fetch_tournaments().await;
    data_pull.fetch_teams_and_players().await;
    // println!("Datapull: {data_pull:?}");
    data_pull.teams.iter().for_each(|t|
        println!("{}", t)
    );

    dao::bulk_leagues_in_database(data_pull.leagues)
        .await
        .with_context(|| "Failed the insert Leagues operation")
}
