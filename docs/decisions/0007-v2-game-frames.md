# 0007 - v2 game frames: archive and series

- Status: Proposed
- Date: 2026-11-01

## Context
The game details ([ADR 0005](0005-v2-matches-and-game-details.md)) keep only the feed's last frame. The feed has about 4 frames a second for every game since 2023-10, it isn't used by the new site and can go away. We want to keep all of it before that happens, and later draw graphs (gold over time, objectives) and run stats over many games. A game is 8,000 frames and 56 MB of JSON ([storage investigation](../investigations/2026-11-01-game-frames-storage.md)).

## Decision
- Two layers in the Postgres we have, no extension, tables in `src/dao/v2/schema.sql` (Canyon has no `bytea` or arrays, and never touches a table it doesn't know):
  - Archive `v2_game_frames`: one row per game and endpoint (`window`, `details`), every frame as the feed sent it, zstd level 19 (format 1). About 233 KB a game.
  - Series `v2_game_timeline`, `v2_game_team_series`, `v2_game_player_series`: one row per game, team and player, one array per stat, a point every 10 s of game time (paused time doesn't count). Plus `v2_game_marker` for dragons, barons, towers and inhibitors. Built from the archive, so they can be built again (`rebuild-timeline`) without the feed.
- `v2_game.frames` (`live`, `done`, `no_feed`) with tries and waits like the details, as `V2Game` fields so Canyon keeps the columns.
- After the game a queue walks the feed 10 s at a time (~400 calls), writes the archive and the series in one statement (all or nothing). It walks through pauses (the feed can go 12 min without a frame) and up to the end the details job saw.
- While a game is played, a loop every 10 s appends points (`source = 'live'`), starting at the game's first frame and catching up after missed ticks. The archive replaces them after the game. The frames read live aren't kept.
- Old games only with `TRIFORCE_BACKFILL_FRAMES=on` (off by default). Every history job got the same kind of switch: `TRIFORCE_BACKFILL_MATCHES`, `TRIFORCE_BACKFILL_DETAILS` (both on, like before).
- `docs/data/` describes all v2 data for any consumer, and is updated with every change to it.

## Consequences
- Good:
  - Everything the feed sends is kept, even fields we don't read yet.
  - Typed series answer SQL over many games (gold difference at 15:00 for a league) without decoding anything.
  - Live graphs are possible, and the after-game write fixes their holes.
  - The archive costs less than the box score's sources would suggest: zstd keeps the repeated frames almost free.
- Bad:
  - The DB grows ~7-8 GB with the full backfill (~23k games: ~5-6 GB archive, ~2 GB series), ~3 GB a year after.
  - A backfill of the whole history is ~400 calls a game, days of run time.
  - The archive needs zstd to read. SQL can't look inside it.
  - Writes go through one big statement with the data as text (Canyon binds no binary or arrays).
  - Live points are 60-90 s behind the game (the feed refuses newer windows).

## Evidence
- [Game frames storage](../investigations/2026-11-01-game-frames-storage.md)
- [Live recording and feed history](../investigations/2026-10-18-live-recording-and-feed-history.md)
- [docs/data/game-frames.md](../data/game-frames.md)
- `src/client/feed/walk.rs`, `src/frames/`, `src/dao/v2/frames.rs`, `src/service/v2_frames.rs`, `src/service/v2_frames_live.rs`
