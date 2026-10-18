//! New API answers → the types the DAO takes today

pub mod leagues;
pub mod teams;
pub mod v2;

use crate::client::legacy::models::LolesportsId;

pub(crate) fn parse_id(id: &str) -> color_eyre::Result<LolesportsId> {
    id.parse::<i64>()
        .map(LolesportsId)
        .map_err(|e| color_eyre::eyre::eyre!("bad lolesports id {id:?}: {e}"))
}

/// A lolesports id as `i64`
pub(crate) fn ext_id(id: &str) -> color_eyre::Result<i64> {
    parse_id(id).map(|x| x.0)
}
