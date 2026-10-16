# Canyon-SQL: 0.5.1 vs `main` vs `develop`

Date: 2026-10-16

## Why we looked
After moving to `main` the code didn't look like the [Canyon book](https://zerodaycode.github.io/canyon-book/) (no `Crud` derive, other trait names). The book is built from [zerodaycode/canyon-book](https://github.com/zerodaycode/canyon-book), so we compared dates and branches:

| Source | Last commit |
|---|---|
| canyon-book | 2026-09-28 |
| Canyon-SQL `develop` | `1c81b3b`, 2026-09-23 |
| Canyon-SQL `main` | `71c7951`, 2026-08-07 |

`git ls-remote` shows the branches. Searching for the book APIs (`proc_macro_derive(Crud`, `CanyonResult`, `CanyonError`) only `develop` has them. The book documents `develop`.

## Differences

| | 0.5.1 | `main` 71c7951 | `develop` 1c81b3b | Book |
|---|---|---|---|---|
| CRUD derive | `CanyonCrud` | `CanyonCrud` | `Crud` (and `Read`/`Insert`/`Update`/`Delete`) | `Crud` |
| Traits | `CrudOperations` | `ReadOperations`, `InsertOperations`, ... | `Read`, `Insert`, `Update`, `Delete` | same as `develop` |
| Errors | `Box<dyn Error>` | `Box<dyn Error + 'a>` | `CanyonResult` / `CanyonError` | `CanyonResult` |
| Builder trait | `ops::QueryBuilder` | `QueryBuilderOps` | `QueryBuilderOps` | `QueryBuilderExt` (doesn't exist) |
| `multi_insert` | yes | no | no | "insert them individually" |
| `extern crate canyon_sql` needed | no | yes | yes | not mentioned |
| cfg bug | yes | fixed | fixed | - |

Book mismatches still open on `develop`:
- `querybuilder.md` imports `canyon_sql::query::querybuilder::QueryBuilderExt`, the trait in the code is `QueryBuilderOps` (`canyon_core/src/query/querybuilder/contracts/mod.rs:154`).
- `extern crate canyon_sql;` is required and not documented, see [ADR 0002](../decisions/0002-extern-crate-canyon-sql.md).

## Query builder, how we use it
`where_value` borrows the value (`&'a Z`), so it has to live as long as the query. A temporary in the same statement is enough, that's what `src/dao/mod.rs` does:

```rust
let db_events = Schedule::select_query()?
    .where_value(
        &ScheduleFieldValue::start_time(Utc::now().naive_utc().checked_sub_days(Days::new(1))),
        Operator::Gt,
    )
    .build()?
    .launch_default::<Schedule>()
    .await;
```

If the query is kept in a variable, bind the value first. For writes the book uses the connection, it returns the affected rows:

```rust
let id_gt_zero = TeamPlayerFieldValue::id(0);
let query = TeamPlayer::delete_query()?
    .where_value(&id_gt_zero, Operator::Gt)
    .build()?;
let connection = Canyon::instance()?.get_default_connection()?;
let deleted = connection.execute(query.sql(), query.params()).await?;
```

Our code uses `launch_default::<TeamPlayer>()` for the delete, it also works (tested on `develop`).

## What the builder can't do (yet)
- **Multi-row insert:** `InsertQueryBuilder` always writes one row, `VALUES ($1, $2)` (`canyon_core/src/query/querybuilder/syntax/emitter/types/insert.rs:73-81`). More values only add parameters, the database rejects it. The book says the same: insert rows one by one or write the SQL yourself.
- **Transactions:** there is no `BEGIN`/`COMMIT` API. The `Transaction` trait only runs single queries (`canyon_core/src/transaction.rs:43-97`).

For `team_player` this means one SQL statement through `DbConnection::execute`. Postgres runs a single statement atomically, so delete and insert can go together:

```sql
WITH deleted AS (DELETE FROM team_player)
INSERT INTO team_player (team_id, player_id) VALUES ($1, $2), ($3, $4), ...
```
