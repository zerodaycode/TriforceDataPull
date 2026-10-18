//! Batched INSERT … ON CONFLICT for the v2 tables. FKs are resolved by ext_id in the SQL

use std::collections::BTreeMap;

use canyon_sql::{connection::DbConnection, date_time::NaiveDateTime, query::QueryParameter};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Val {
    I64(Option<i64>),
    I32(Option<i32>),
    Text(Option<String>),
    Time(Option<NaiveDateTime>),
}

/// Params per statement, under Postgres' limit of 65535
pub const MAX_PARAMS: usize = 60_000;

pub fn t(s: &str) -> Val {
    Val::Text(Some(s.to_string()))
}

impl Val {
    pub fn param(&self) -> &dyn QueryParameter {
        match self {
            Val::I64(v) => v,
            Val::I32(v) => v,
            Val::Text(v) => v,
            Val::Time(v) => v,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Col {
    Plain(&'static str),
    /// The value is an ext_id, the column stores the id of that row in `table`
    ByExt {
        name: &'static str,
        table: &'static str,
    },
}

impl Col {
    fn name(&self) -> &'static str {
        match self {
            Col::Plain(n) | Col::ByExt { name: n, .. } => n,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Upsert {
    pub table: &'static str,
    pub cols: &'static [Col],
    pub conflict: &'static [usize],
    pub update: bool,
    /// Extra condition for `DO UPDATE` (`v2_x` = stored row, `EXCLUDED` = new one)
    pub guard: Option<&'static str>,
}

pub fn build_sql(u: &Upsert, rows: usize) -> String {
    let names: Vec<&str> = u.cols.iter().map(Col::name).collect();
    let mut n = 0;
    let values: Vec<String> = (0..rows)
        .map(|_| {
            let vals: Vec<String> = u
                .cols
                .iter()
                .map(|c| {
                    n += 1;
                    match c {
                        Col::Plain(_) => format!("${n}"),
                        Col::ByExt { table, .. } => {
                            format!("(SELECT id FROM {table} WHERE ext_id = ${n})")
                        }
                    }
                })
                .collect();
            format!("({})", vals.join(", "))
        })
        .collect();
    let conflict: Vec<&str> = u.conflict.iter().map(|i| names[*i]).collect();
    let set: Vec<&str> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| !u.conflict.contains(i))
        .map(|(_, c)| *c)
        .collect();
    // nothing to update (all columns are in the key): DO NOTHING
    let action = if u.update && !set.is_empty() {
        let assign: Vec<String> = set.iter().map(|c| format!("{c} = EXCLUDED.{c}")).collect();
        // skip rows that didn't change: no new tuple, no WAL, no dead rows
        let old: Vec<String> = set.iter().map(|c| format!("{}.{c}", u.table)).collect();
        let new: Vec<String> = set.iter().map(|c| format!("EXCLUDED.{c}")).collect();
        let guard = u.guard.map(|g| format!(" AND ({g})")).unwrap_or_default();
        format!(
            "DO UPDATE SET {} WHERE ({}) IS DISTINCT FROM ({}){guard}",
            assign.join(", "),
            old.join(", "),
            new.join(", ")
        )
    } else {
        "DO NOTHING".to_string()
    };
    format!(
        "INSERT INTO {} ({}) VALUES {} ON CONFLICT ({}) {}",
        u.table,
        names.join(", "),
        values.join(", "),
        conflict.join(", "),
        action
    )
}

/// Postgres refuses two rows with the same conflict key in one statement: last one wins.
/// Sorted by key, so two loops upserting the same table lock rows in the same order (no deadlock)
pub fn dedupe(u: &Upsert, rows: Vec<Vec<Val>>) -> Vec<Vec<Val>> {
    let mut by_key: BTreeMap<Vec<Val>, Vec<Val>> = BTreeMap::new();
    for row in rows {
        let key: Vec<Val> = u.conflict.iter().map(|i| row[*i].clone()).collect();
        by_key.insert(key, row);
    }
    by_key.into_values().collect()
}

pub async fn run(u: &Upsert, rows: Vec<Vec<Val>>) -> Result<u64> {
    let rows = dedupe(u, rows);
    // Postgres takes at most 65535 parameters per statement
    let per_batch = (MAX_PARAMS / u.cols.len()).max(1);
    let mut total = 0;
    for batch in rows.chunks(per_batch) {
        let sql = build_sql(u, batch.len());
        let params: Vec<&dyn QueryParameter> = batch.iter().flatten().map(Val::param).collect();
        total += ""
            .execute(&sql, &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err_with(|| format!("upsert {}", u.table))?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Upsert = Upsert {
        table: "v2_tournament",
        cols: &[
            Col::Plain("ext_id"),
            Col::ByExt {
                name: "league_id",
                table: "v2_league",
            },
            Col::Plain("slug"),
        ],
        conflict: &[0],
        update: true,
        guard: None,
    };

    #[test]
    fn sql_for_two_rows() {
        assert_eq!(
            build_sql(&T, 2),
            "INSERT INTO v2_tournament (ext_id, league_id, slug) VALUES \
             ($1, (SELECT id FROM v2_league WHERE ext_id = $2), $3), \
             ($4, (SELECT id FROM v2_league WHERE ext_id = $5), $6) \
             ON CONFLICT (ext_id) DO UPDATE SET league_id = EXCLUDED.league_id, slug = EXCLUDED.slug \
             WHERE (v2_tournament.league_id, v2_tournament.slug) IS DISTINCT FROM (EXCLUDED.league_id, EXCLUDED.slug)"
        );
    }

    #[test]
    fn guard_is_added_to_the_update() {
        let u = Upsert {
            guard: Some("v2_tournament.slug <> 'x'"),
            ..T
        };
        assert!(build_sql(&u, 1).ends_with(
            "IS DISTINCT FROM (EXCLUDED.league_id, EXCLUDED.slug) AND (v2_tournament.slug <> 'x')"
        ));
        let u = Upsert { update: false, ..u };
        assert!(build_sql(&u, 1).ends_with("DO NOTHING"));
    }

    #[test]
    fn do_nothing_when_no_update() {
        let u = Upsert { update: false, ..T };
        assert!(build_sql(&u, 1).ends_with("ON CONFLICT (ext_id) DO NOTHING"));
    }

    #[test]
    fn do_nothing_when_all_columns_are_the_key() {
        let u = Upsert {
            table: "v2_league_i18n",
            cols: &[Col::Plain("a"), Col::Plain("b")],
            conflict: &[0, 1],
            update: true,
            guard: None,
        };
        assert!(build_sql(&u, 1).ends_with("ON CONFLICT (a, b) DO NOTHING"));
    }

    #[test]
    fn dedupe_keeps_the_last() {
        let rows = vec![
            vec![
                Val::I64(Some(1)),
                Val::I64(Some(9)),
                Val::Text(Some("a".into())),
            ],
            vec![
                Val::I64(Some(2)),
                Val::I64(Some(9)),
                Val::Text(Some("b".into())),
            ],
            vec![
                Val::I64(Some(1)),
                Val::I64(Some(9)),
                Val::Text(Some("c".into())),
            ],
        ];
        let out = dedupe(&T, rows);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][2], Val::Text(Some("c".into())));
        assert_eq!(out[1][2], Val::Text(Some("b".into())));
    }
}
