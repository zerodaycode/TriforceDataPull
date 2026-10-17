# Canyon-SQL: what Triforce needs and works around

Date: 2026-10-17

Found while building the v2 reference tables on Canyon-SQL `develop`, `rev 1c81b3b`. Canyon paths are relative to the checkout in `~/.cargo/git/checkouts/canyon-sql-*/1c81b3b/`. The API sketches are what we would like, not something that exists.

## Schema and migrations

| # | Missing | Triforce today | Where |
|---|---|---|---|
| 1 | UNIQUE and composite unique | `CREATE UNIQUE INDEX IF NOT EXISTS` at startup | `src/dao/v2/schema.sql`, `src/dao/v2/schema.rs` |
| 2 | Index attribute | `CREATE INDEX IF NOT EXISTS`, same file | same |
| 3 | CHECK / enum mapping | `DROP CONSTRAINT IF EXISTS` + `ADD CONSTRAINT ... CHECK` at startup | `src/dao/v2/schema.sql` |
| 4 | `timestamptz` (`DateTime<Utc>`) | `NaiveDateTime`, always UTC by convention (`naive_utc()`) | `src/dao/v2/models/*` |
| 10 | Hook to run SQL after migrations | `dao::v2::schema::apply()` first thing in the v2 loop, retried every 5 minutes | `src/main.rs` |
| 11 | Schemas other than `public` | `v2_` prefix on the table names | `src/dao/v2/models/*` |
| 12 | Datasource per entity | same DB, prefixed tables, no separate v2 DB | same |
| 13 | More than one entity per file | one file per entity, 12 files | `src/dao/v2/models/` |
| 14 | FK / unique handling that does not collide | every FK is `#[foreign_key]`, unique only as index | `src/dao/v2/schema.sql` (header comment) |
| 15 | Table rename untested | open point, `v2_*` will be renamed later | none yet |

### 1-2. Unique and index

Canyon migrations only create/rename tables, add/drop columns, change type or nullability, and set PK and FK (`canyon_migrations/src/migrations/processor.rs`).

```rust
#[canyon_entity(table_name = "v2_league_i18n")]
#[unique_index(name = "v2_league_i18n_key", columns = ["league_id", "locale"])]
pub struct V2LeagueI18n {
    #[index]
    pub league_id: i32,
    pub locale: String,
}
```

### 3. CHECK

The type map (`canyon_migrations/src/migrations/transforms.rs:12-55`) only knows ints, `String`, `bool`, `NaiveDate`, `NaiveTime` and `NaiveDateTime`.

```rust
#[check(regex = "^[a-z]{2}-[A-Z]{2}$")]
pub locale: String,
```

### 4. timestamptz

`NaiveDateTime` maps to `timestamp without time zone` (`transforms.rs`, `constants.rs`).

```rust
pub start_time: Option<DateTime<Utc>>, // timestamp with time zone
```

### 10. Hook after migrations

`#[canyon_sql::main]` runs `Canyon::init` and the migrations before the body of `main` (`canyon_macros/src/lib.rs:73-111`), there is no place in between.

```rust
#[canyon_sql::main(after_migrations = dao::v2::schema::apply)]
async fn main() { ... }
```

### 11-12. Schema and datasource

Migrations read `table_schema = 'public'` (`canyon_migrations/src/constants.rs:34`) and apply every entity to every datasource with migrations on (`canyon_migrations/src/migrations/handler.rs:30-37`).

```rust
#[canyon_entity(schema = "v2", datasource = "reference")]
pub struct V2League { ... }
```

### 13. One entity per file

With migrations on, two `#[canyon_entity]` in one file is a compile error ("does not support having multiple structs annotated with `#[canyon::entity]` on the same file"). Wanted: several entities per module.

### 14. FKs and unique constraints

Canyon drops FKs that are not in an entity (`processor.rs:516-545`). The per-column metadata query joins `pg_constraint` by the column inside `(...)` (`constants.rs:30-32`), so a single-column `UNIQUE` constraint would show up in it. We avoid both: FKs only as `#[foreign_key]`, unique only as indexes, never `ADD CONSTRAINT ... UNIQUE`. Fixing 1 makes this go away.

### 15. Rename

`processor.rs` has `ALTER TABLE ... RENAME` but we did not try it. We will when the `v2_*` tables get their final names.

## Queries

| # | Missing | Triforce today | Where |
|---|---|---|---|
| 5 | Upsert (`ON CONFLICT`) | small generic builder with raw SQL, FKs resolved with `(SELECT id FROM t WHERE ext_id = $n)` | `src/dao/v2/upsert/mod.rs` |
| 6 | Multi-row insert | `VALUES` lists, batches under 65535 params | `src/dao/v2/upsert/mod.rs` (`run`) |
| 7 | DB transactions | one statement with CTEs where it must be atomic | `src/dao/v2/reference.rs` (`sync_team_players`) |
| 8 | Multi-statement execute | split the SQL file and run one by one | `src/dao/v2/schema.rs` (`statements`) |
| 9 | Partial update | v1 keeps the stored value before `update()`, v2 leaves columns out of the upsert | `src/dao/models/teams.rs` (`keep_alt_image_from`), `src/dao/v2/reference.rs` (`TEAM`) |
| 19 | `Crud` derive without the FK target in scope | `use super::<file>::<Struct>;` in FK entities | `src/dao/v2/models/*` |

### 5. Upsert

```rust
V2League::insert_query()?
    .values(&rows)
    .on_conflict(V2LeagueField::ext_id)
    .do_update(&[V2LeagueField::slug, V2LeagueField::image_url])
    .build()?
```

### 6. Multi-row insert

`InsertQueryBuilder` writes one row, `VALUES ($1, $2)` (`canyon_core/src/query/querybuilder/syntax/emitter/types/insert.rs:73-81`). The test is commented out (`tests/crud/insert_operations.rs:136`, `// fn test_crud_multi_insert_operation() {`). See also [main vs develop](2026-10-16-canyon-main-vs-develop.md).

```rust
V2Player::insert_many(&players).await?;
```

### 7. Transactions

`DbConnection::execute` takes a pooled connection per call (`canyon_core/src/connection/clients/postgresql.rs:99-105`), so BEGIN / COMMIT can't span calls. The `Transaction` trait (`canyon_core/src/transaction.rs:43`) is a CRUD proxy, not a DB transaction.

```rust
let mut tx = connection.begin().await?;
tx.execute(delete_sql, &params).await?;
tx.execute(insert_sql, &params).await?;
tx.commit().await?;
```

### 8. Multi-statement execute

`execute` is one prepared statement.

```rust
connection.batch_execute(include_str!("schema.sql")).await?;
```

### 9. Partial update

`update()` writes the whole row.

```rust
Team::update_query()?
    .set(&[TeamFieldValue::name(name)])
    .where_value(&TeamFieldValue::id(id), Operator::Eq)
    .build()?
```

### 19. FK target in scope

Without the target struct imported, the `Crud` derive fails to compile. The attribute already has `table` and `column`, so it could resolve the type itself.

## Config, build and tests

| # | Issue | Triforce today | Where |
|---|---|---|---|
| 16 | First `canyon*.toml` found (depth <= 2) wins | template renamed to `canyon.toml.example` | `canyon.toml.example`, README (Setup) |
| 17 | Real error swallowed when creating `canyon_memory` | check which config Canyon read and the host | none |
| 18 | `canyon_tokio_test` runs `Canyon::init` but not the migrations | start the app once before DB tests | [README (Tests)](../../README.md#tests) |
| 20 | Migrations computed at compile time, the build connects to the DB | `touch src/main.rs` after DB changes | README |

- **16:** the lookup is in `canyon_core/src/canyon.rs:205`. Wanted: error when there is more than one, or a `CANYON_CONFIG` env var.
- **17:** "Error creating the 'canyon_memory' table" hides the real DB error (`memory.rs:283`). Usually it means it can't connect. Wanted: include the source error and the config path in the message.
- **18:** `canyon_macros/src/canyon_tokio_test.rs`. A test that needs the new tables fails until the app ran once.
- **20:** `canyon_macros/src/canyon_macro.rs:10-17`. A binary only fits the DB it was built against. Wanted: compute the migrations at startup, or a flag to skip them in the build.
