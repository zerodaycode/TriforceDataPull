# Game frames storage

Date: 2026-11-01

How to keep every frame of the live feed for each game: how big it is, where to put it and in what shape. Sources: one finished game walked through the feed (`115660540726095095`, Worlds, 2026-10-31, 33 min), a scratch schema in the dev DB (dropped after), the Canyon-SQL code we pin (`cc563d7`). Background: [live recording and feed history](2026-10-18-live-recording-and-feed-history.md), [ADR 0005](../decisions/0005-v2-matches-and-game-details.md).

## What the feed gives

- `window/{gameId}?startingTime=<t>` and `details/{gameId}?startingTime=<t>` return ~10 s of frames from `t`. Walking a game 10 s at a time: 200 calls per endpoint, 15.5 MB (`window`) and 49.6 MB (`details`) of JSON.
- ~4 frames a second (median gap 0.166 s, max 2.6 s): 8,053 frames for 33 min. `window` and `details` have the same timestamps.
- ~75 % of the frames repeat the previous one with a new timestamp (`window` 2,109 changed, `details` 3,077).
- `gameState`: `in_game` until the last frame, `finished` on it. Games from 2026-04 (LEC) end without `finished`. No pause in this game.
- `window` frame: per team gold, kills, towers, inhibitors, barons, dragon list, and 5 players with gold, level, KDA, CS, health.
- `details` frame: 10 players with ~20 numbers (KDA, gold, CS, damage share, kill participation, wards, AD, AP, armor, ...), 7 item ids, runes, skill order.

## Size per game (`window` + `details`)

| Stored as | All frames | Changed frames |
|---|---|---|
| JSON | 56 MB | 20 MB |
| `jsonb` (Postgres lz4) | ~4-5 MB | ~1.4 MB |
| gzip -9 | 610 KB | 350 KB |
| xz | 280 KB | 207 KB |
| Rows, one per player per frame | ~80k | ~30k |

- `jsonb` measured on the `window` changed frames (3.4 MB JSON → 245 KB), the rest is scaled from it. JSON text in a `bytea` with lz4: 342 KB.
- No zstd in the container: xz stands for it. zstd at high levels is usually 10-20 % over xz.
- With a strong codec the repeated frames cost little (280 KB vs 207 KB), so we can keep all of them.

For ~23k completed games since 2023-10 (~8k more per year): ~5-7 GB compressed by us, 30-100 GB as `jsonb`, 1-2 billion rows.

## Where to store it

| Option | Good | Bad |
|---|---|---|
| Postgres, the one we have | backups, FKs and the job queue in one place; consumers already read it | can't query inside a blob we compress |
| Files or object storage (disk, S3, MinIO) | small DB | another volume to back up, a row can point at a missing file |
| Time-series DB (InfluxDB, QuestDB) | made for numbers over time | the frames are nested JSON (items, runes); another service; Canyon doesn't know it |
| ClickHouse, DuckDB + Parquet | SQL over billions of frame rows | that's analysis, not an archive; Parquet can be made from the archive later |
| MongoDB | | nothing `jsonb` doesn't do, another service |

The archive is written once per game and read whole, ~7 GB in total. Not enough to run a second database.

## Plain Postgres or an extension

- TimescaleDB is the one that changes things: its compressed tables would make rows small enough. But compression isn't in the Apache build (other image, its own upgrades), it's made for flat numbers, and neither Canyon nor `schema.sql` knows hypertables. Worth a look again only for SQL over every frame.
- pg_partman, Citus columnar, pg_duckdb: not needed at this size, or too new.
- Plain Postgres already compresses big values (TOAST, lz4). A value we compress ourselves goes in a `bytea` with `STORAGE EXTERNAL` so Postgres doesn't try again.

## Shape

- Raw archive: one row per game and endpoint, the frames as the feed sent them, compressed. Not one row per frame (millions of rows read always together), not `jsonb` (5-6x bigger, and nobody queries inside it), not typed columns (the feed is unofficial and changes: a field we don't have a column for would be lost).
- For graphs and analysis: typed series built from the archive, one row per game per team and per player, one array per stat, a point every 10 s of game time. ~50 KB per game estimated, ~90 KB measured (30 min backfill, 12 games), ~2 GB for the full history. Typed columns make sense here: the fields are known and the volume is small. Rebuilt from the archive when a new field or step is needed, no feed calls.
- The feed timestamps are wall time. Pro games get paused, so the series use game time (paused time removed).

## Canyon

- The `cc563d7` migrations know `i16/i32/i64/f32/f64/String/bool` and date-times. No `bytea`, no arrays, and floats have their own issue.
- It never drops a table it doesn't know (`TableOperation` in `canyon_migrations/src/migrations/processor.rs` has no drop), but it drops the columns of an entity table that the entity doesn't have (`ColumnOperation::DeleteColumn`).
- So the new tables go in `src/dao/v2/schema.sql`, and new columns on `v2_game` go in the `V2Game` entity. Nothing waits for the migrations refactor.

## Measured while building it

- zstd level 19 on the same game: 96 KB (`window`) + 137 KB (`details`) = 233 KB, every frame kept. Better than xz (281 KB). About 5-6 GB for the full history.
- Pauses: a search over 100 Worlds games at 60 s steps (842 calls) found 2 with `paused` frames. Game `115660540726095075` (2026-10-29) paused 4 times, 73 min of wall time for 37 min of game. During a pause the feed sends very few frames: the longest gap without any frame was 12 min (21:43:24 → 21:55:41), all of them while `paused`. A walk or a live loop that stops after a few empty calls would cut these games short.
- Item lists: the feed sends the items a player holds, not fixed slots. In one game the lists had 0 to 8 items (the trinket and role quest items count).

## Serving it (notes for later)

Not decided here, the consumer decides. What came up:

- Charts drawn by the client from one array per series are small (5-30 KB gzipped per game at 10-60 s) and interactive. Images drawn by the server are only useful for previews and embeds.
- A finished game never changes, it can be cached for good. A live one changes every ~10 s.
- Reading the archive needs zstd. Reading the series is plain SQL.
