# 0004 - Schema v2 reference tables

- Status: Proposed
- Date: 2026-10-17

## Context
The v1 tables follow the old API: names in one language, teams found by name, no seasons, splits or stages, and no unique keys. The new API ([ADR 0003](0003-migrate-to-lolesports-graphql-api.md)) gives ids, seasons, splits, stages and localized names. WSC-Triforce and the app will move to the new tables later.

## Decision
- `v2_*` tables next to the v1 ones, filled from the new API (only `alt_image_url` comes from the old one, see below), in the same process, on their own loop: first run at start, then every 2 hours at :15. `GetOtherLeagues` per locale and `GetLoLTeamOverview` feed them.
- `id` is a serial, `ext_id` is the API id with a unique index.
- Localized texts only in `*_i18n` tables, one row per locale. `en-US` is always there, more come from `TRIFORCE_LOCALES`. Readers fall back to `en-US`.
- Canyon entities create the tables and the foreign keys. What Canyon can't create (unique indexes, checks) is in `src/dao/v2/schema.sql`, run at start. If it fails the app doesn't start.
- Writes are batched `INSERT ... ON CONFLICT` in raw SQL, because Canyon's builder inserts one row and has no upsert. Rows that didn't change are not rewritten.
- Rosters are synced per team.
- Player photos: the API sends a placeholder when a player has none (`default-headshot.png`, `silhouette.png`, ...). We store `NULL` instead, so readers pick their own fallback. A photo is a placeholder when its file name is a known one, when 3 or more players share it, or when it is empty.
- `alt_image_url` comes from the old API (`getTeams`) while it lives. The new API and the site don't use it.
- `TRIFORCE_V2=off` stops the loop and the startup SQL. Canyon still creates the tables, its migrations run before anything else.

## Consequences
- Good:
  - Real keys, so rows can't be duplicated.
  - Seasons, splits and stages are stored.
  - Names in several languages without touching v1.
  - Unchanged rows are not rewritten.
- Bad:
  - While a v1 source is `new` the API is called twice for the same data (accepted while v1 lives).
  - No transaction across statements (Canyon), a failed run can leave part of the data written until the next run.
  - The tables are created even with `TRIFORCE_V2=off`.
  - Every v2 run also calls the old `getTeams` for `alt_image_url`. When the old API goes away the stored values stay, they just stop updating.

## Open points
- Stage names: `v2_stage_i18n` stays empty until we call `GetSeasonStage`.
- `light_image_url` has no source yet. The team pages only use `image`.
- Split names.
- Renaming `v2_*` when v1 goes away. Canyon rename is untested: if it doesn't work we run a SQL script and report it to Canyon-SQL.
- v2 tables for matches: today v1 keeps them in `schedule`, with one left and one right team for the whole match. v2 will have the match, its games with the blue and red team of each game, and the streams.
- Roster history needs another source (ROADMAP).

## Evidence
- [Canyon features for Triforce](../investigations/2026-10-17-canyon-features-for-triforce.md)
- [WSC-Triforce on schema v2](../investigations/2026-10-17-wsc-triforce-on-schema-v2.md)
- [Team pages, images and locales](../investigations/2026-10-18-team-pages-images-and-locales.md)
- [ADR 0003](0003-migrate-to-lolesports-graphql-api.md)
- `src/dao/v2/`
