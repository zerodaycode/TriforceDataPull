mod data_pull;
mod service;
mod utils;


use canyon_sql;


#[canyon_sql::main]
fn main() {
    let mut data_pull = service::DataPull::new().await;
    data_pull.fetch_leagues().await;
    // data_pull.fetch_tournaments().await;
    data_pull.fetch_teams_and_players().await;
    // println!("Datapull: {data_pull:?}");
    data_pull.teams.iter().for_each(|t|
        println!("{}", t)
    );
}
