mod dao;
mod data_pull;
mod service;
mod utils;

use std::time::Instant;

use color_eyre::Result;

#[canyon_sql::main]
fn main() -> Result<()> {
    let start = Instant::now();
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
    data_pull.fetch_teams_and_players().await?;
    database_ops
        .bulk_teams_in_database(&data_pull.teams)
        .await?;
    database_ops
        .bulk_players_in_database(&data_pull.players)
        .await?;
    database_ops
        .bulk_team_player_in_database(&data_pull.teams)
        .await?;

    // Processing the complete schedule
    // data_pull.process_full_schedule().await?;

    // For testing purposes right now
    // data_pull.fetch_live().await?;
    // println!("Datapull: {data_pull:?}");
    println!("Execution time: {:?}", start.elapsed());
    Ok(())

    // data_pull.teams.iter().for_each(|t|
    //     println!("{}", t)
    // );

    // dao::bulk_leagues_in_database(data_pull.leagues)
    //     .await
    //     .with_context(|| "Failed the insert Leagues operation")
}
