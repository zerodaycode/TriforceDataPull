# 0001 - Use Canyon-SQL `develop` branch, pinned to a commit

- Status: Accepted
- Date: 2026-10-16

## Context
We had to move from Canyon-SQL 0.4.2 and there are three options:

- **0.5.1** (last release): every insert and count panics with `Reached the panic match arm of insert for the DatabaseConnection type`. The derive macros generate `#[cfg(feature = "postgres")]` code that is checked against our crate features, not Canyon ones, so only the `_ => panic!()` arm is left. It can be avoided declaring the features in our crate, but it only hides the bug. Details in [the investigation](../investigations/2026-10-16-canyon-0.5.1-cfg-bug.md).
- **`main`** (`71c7951`, 2026-08-07): the cfg bug is fixed, but the API doesn't match the [Canyon book](https://zerodaycode.github.io/canyon-book/). No `Crud` derive, `*Operations` traits, and errors with lifetimes that don't work with `?` in our code.
- **`develop`** (`1c81b3b`, 2026-09-23): it's what the book documents. `Crud` derive, `Read`/`Insert`/`Update`/`Delete` traits and `CanyonResult`.

## Decision
Use the `develop` code, pinned with `rev` to commit `1c81b3b`:

```toml
canyon_sql = { git = "https://github.com/zerodaycode/Canyon-SQL.git", rev = "1c81b3b1c1982c33aeadd1d214bc2aeef95253b4", features = ["postgres", "migrations"] }
```

With `branch = "develop"` only `Cargo.lock` keeps the commit, and `cargo update` moves it to the new head of the branch. With `rev` the commit is in `Cargo.toml`, updating Canyon is a change we do on purpose.

## Consequences
- Good: the code looks like the book examples, `?` works with `CanyonResult`, no cfg bug.
- Bad: it's not released. To update: `git ls-remote https://github.com/zerodaycode/Canyon-SQL.git develop`, read the new commits, change `rev`, build and run the app once against the dev DB.
- Bad: `1c81b3b` is not tagged. If upstream rewrites `develop`, fresh clones can't fetch it. Fallback: a fork with a tag, or `cargo vendor`. Move to a release once one has this API.
- Bad: `multi_insert` doesn't exist anymore, `team_player` rows are inserted one by one.
- Bad: we still need `extern crate canyon_sql;`, see [0002](0002-extern-crate-canyon-sql.md).

## Evidence
- [Comparison between 0.5.1, `main` and `develop`](../investigations/2026-10-16-canyon-main-vs-develop.md)
- [0.5.1 cfg bug](../investigations/2026-10-16-canyon-0.5.1-cfg-bug.md)
- Tested on 2026-10-16 against LoL Esports and the local DB: 51 leagues, 513 tournaments, 1586 teams, 4526 players, 5431 team_player rows, 2086 schedule rows. A second run updates the same rows, no duplicates.
