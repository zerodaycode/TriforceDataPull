use canyon_sql::{date_time::NaiveDateTime, macros::*};
use serde::Serialize;

/// When a long job (like the full schedule pass) last ended without error
#[derive(Debug, Clone, Crud, CanyonMapper, Serialize)]
#[canyon_entity(table_name = "v2_job_run")]
pub struct V2JobRun {
    #[primary_key]
    pub id: i32,
    pub job: String,
    pub last_ok_time: NaiveDateTime,
}
