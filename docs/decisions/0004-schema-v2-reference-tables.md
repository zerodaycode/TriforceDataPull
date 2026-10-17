# 0004 - Schema v2 reference tables

- Status: Proposed
- Date: 2026-10-17

## Context
The v1 tables follow the old API: names in one language, teams found by name, no seasons, splits or stages, and no unique keys. The new API ([ADR 0003](0003-migrate-to-lolesports-graphql-api.md)) gives ids, seasons, splits, stages and localized names. WSC-Triforce and the app will move to the new tables later.

## Decision
- `v2_*` tables next to the v1 ones, filled only from the new API, in the same process, on their own loop: first run at start, then every 2 hours at :15. `GetOtherLeagues` per locale and `GetLoLTeamOverview` feed them.
- `id` is a serial, `ext_id` is the API id with a unique index.
- Localized texts only in `*_i18n` tables, one row per locale. `en-US` is always there, more come from `TRIFORCE_LOCALES`. Readers fall back to `en-US`.
- Canyon entities create the tables and the foreign keys. What Canyon can't create (unique indexes, checks) is in `src/dao/v2/schema.sql`, run by the v2 loop. If it fails it is retried every 5 minutes.
- Writes are batched `INSERT ... ON CONFLICT` in raw SQL, because Canyon's builder inserts one row and has no upsert. Rows that didn't change are not rewritten.
- Rosters are synced per team.
- `TRIFORCE_V2=off` stops the loop and the startup SQL. Canyon still creates the tables.

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

## Open points
- Stage names: `v2_stage_i18n` stays empty until we call `GetSeasonStage`.
- `light_image_url` and `alt_image_url` have no source in the new API.
- Split names.
- Renaming `v2_*` when v1 goes away (Canyon rename is untested).
- v2 event tables: schedule, games with blue and red side, streams.
- Roster history needs another source.

## Evidence
- [Canyon features for Triforce](../investigations/2026-10-17-canyon-features-for-triforce.md)
- [WSC-Triforce on schema v2](../investigations/2026-10-17-wsc-triforce-on-schema-v2.md)
- [ADR 0003](0003-migrate-to-lolesports-graphql-api.md)
- `src/dao/v2/`
