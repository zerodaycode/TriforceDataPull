mod data_pull;
mod service;
mod utils;

use canyon_sql;

#[canyon_sql::main]
fn main() {
    let mut data_pull = service::DataPull::new().await;
    data_pull.fetch_leagues().await;
    data_pull.fetch_tournaments().await;
    println!("Datapull: {data_pull:?}");
}
