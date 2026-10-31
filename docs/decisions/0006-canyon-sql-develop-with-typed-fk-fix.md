# 0006 - Canyon-SQL `develop` with the typed foreign key fix

- Status: Proposed
- Date: 2026-10-31

## Context
`develop` moved from `1c81b3b` to `9892c84` (2026-09-24). Changes that touch us:

- Foreign keys point at the entity, not at a table name: `#[foreign_key(references = Team::id)]` instead of `#[foreign_key(table = "team", column = "id")]`. A typo is a compile error now.
- The query builder trait is `QueryBuilderExt` (was `QueryBuilderOps`).
- The macros don't generate `crate::canyon_sql::...` paths anymore, the reason for [0002](0002-extern-crate-canyon-sql.md).

With migrations on, `9892c84` doesn't build on any entity with a foreign key: `custom attribute panicked ... Error extracting column ref from FK annotation`. The migrations still read the old `Table: x, Column: y` text from the annotation. The fix is the branch `fix/typed-fk-old-migrations` (`cc563d7`, on top of `9892c84`): it takes the table of the referenced entity from the entity register, so a custom `table_name` works too.

## Decision
Use the fix commit, pinned with `rev` like in [0001](0001-use-canyon-sql-develop-branch.md):

```toml
canyon_sql = { git = "https://github.com/zerodaycode/Canyon-SQL.git", rev = "cc563d7b085919369273461dd9bd1ae78c4a6bba", features = ["postgres", "migrations"] }
```

Remove `extern crate canyon_sql;` from `src/main.rs`.

When the fix is in `develop`, or the migrations refactor replaces the old migrations, move `rev` to that commit.

## Consequences
- Good: everything builds from what is in git, `Cargo.lock` keeps the commit.
- Good: foreign keys are checked by the compiler, and `extern crate` is gone.
- Bad: `cc563d7` is on a branch, not on `develop`. If the branch is deleted after the merge, fresh clones can't fetch it: move `rev` before that.
- Bad: the foreign key fields of the v1 tables are `bigint` but point at `int4` ids, so the lookup methods Canyon generates for them (`find_league`, `find_all_by_team_left`, ...) fail at runtime. We don't use them. Changing the types would change the columns, so they stay, with a comment.
- Same as 0001: not released, update `rev` on purpose.

## Evidence
- `git -C ../Canyon-SQL diff 1c81b3b 9892c84`, and `cc563d7` for the fix.
- 2026-10-31 against the dev DB: with migrations on the build bakes no migration query (the schema already matches), the 30 foreign keys are the same before and after a run, 98 tests and the 6 DB tests pass, clippy has the same 47 warnings as before.
- `9892c84` without the fix: `Error extracting column ref from FK annotation` (`canyon_migrations/src/migrations/processor.rs`, `extract_foreign_key_annotation`).
