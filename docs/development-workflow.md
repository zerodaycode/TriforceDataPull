# Development workflow

## Branches

`<type>/<short-description>`, with the same types as the commits:
`feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `chore`, `build`, `ci`.

Example: `fix/team-player-insert-panic`

## Commits

[Conventional Commits](https://www.conventionalcommits.org/). Short subject, `* ` bullets for the details:

```
refactor(dao): migrate to Canyon-SQL develop branch API

* Rename `CanyonCrud` derive to `Crud` in all entities
* Port query builders to new format
```

The why goes to `docs/decisions` or `docs/investigations`, not to the commit.

## Before merging

1. Review the changes.
2. QA:
   - `cargo fmt --check`
   - `cargo clippy --all-targets`
   - `cargo test`
   - Run the app against the local DB: no panics, data looks right, a second run doesn't duplicate rows.

## Docs to update

| Change | Update |
|---|---|
| Something users notice (feature, fix, upgrade) | `CHANGELOG.md` |
| A decision with trade-offs | new ADR in `docs/decisions/` from `0000-template.md`. A proposed ADR can be updated until it's accepted, after that don't edit it, write a new one that supersedes it |
| Research behind a decision | `docs/investigations/YYYY-MM-DD-topic.md`, linked from the ADR |
| Plans change | `ROADMAP.md`, with `[High]`/`[Medium]`/`[Low]` |
