mod dao;
mod data_pull;
mod service;
mod utils;

use color_eyre::Result;

#[canyon_sql::main]
fn main() -> Result<()> {
    color_eyre::install()?;
    let mut data_pull = service::DataPull::default();
    let mut database_ops = dao::DatabaseOps::default();

    // Processing the leagues
    data_pull.fetch_leagues().await?;
    database_ops
        .bulk_leagues_in_database(&data_pull.leagues)
        .await?;

    // Processing the tournaments
    data_pull.fetch_tournaments().await?;
    database_ops
        .bulk_tournaments_in_database(&data_pull.tournaments)
        .await?;

    // Processing the teams and players
    // data_pull.fetch_teams_and_players().await?;

    // Processing the complete schedule
    // data_pull.process_full_schedule().await?;

    // For testing purposes right now
    // data_pull.fetch_live().await?;
    println!("Datapull: {data_pull:?}");
    Ok(())

    // data_pull.teams.iter().for_each(|t|
    //     println!("{}", t)
    // );

    // dao::bulk_leagues_in_database(data_pull.leagues)
    //     .await
    //     .with_context(|| "Failed the insert Leagues operation")
}
