# Game frames: archive, series, markers

Every frame the live feed sends for a game, about 4 a second, and what's built from them: one point every 10 s of game time for each team and player, plus markers for objectives. Background: [ADR 0007](../decisions/0007-v2-game-frames.md), [storage investigation](../investigations/2026-11-01-game-frames-storage.md). The last frame alone, as plain columns, is in [game-details.md](game-details.md).

Two layers:

- **Archive** (`v2_game_frames`): the frames as the feed sent them, compressed. Kept so anything can be built again later, even if the feed goes away.
- **Series** (`v2_game_timeline`, `v2_game_team_series`, `v2_game_player_series`, `v2_game_marker`, `v2_game_pause`, `v2_game_rollback_team`, `v2_game_rollback_player`): typed values per point, objectives, pauses and chronobreaks, built from the archive (or from the live feed while a game is played).

## Tables

### `v2_game_frames` (archive)

Two rows per game, `window` and `details`, primary key `(game_id, endpoint)`.

| Column | Type | Meaning |
|---|---|---|
| `game_id` | → `v2_game` | |
| `endpoint` | text | `window` (team totals, per player gold/level/KDA/CS/health) or `details` (per player stats, items, runes, skills) |
| `format` | smallint | how `data` is packed, `1` today |
| `frame_count` | int | frames in `data` |
| `first_frame`, `last_frame` | timestamp | wall time of the first and last frame |
| `raw_bytes` | int | JSON size before compression |
| `fetched_at` | timestamp | when the walk ran |
| `data` | bytea | format 1: zstd of `{"gameMetadata": …, "frames": [ … ]}`. Frames are the feed's JSON byte for byte, oldest first, one per timestamp. `gameMetadata` (teams, players, champions, patch) only in the `window` row, `null` in `details` |

`data` is stored as is (`STORAGE EXTERNAL`): it's already compressed. A query that doesn't name `data` doesn't read it.

### `v2_game_timeline`

One row per game with series.

| Column | Type | Meaning |
|---|---|---|
| `game_id` | → `v2_game`, primary key | |
| `source` | text | `live` (built while the game was played, may have holes) or `archive` (built from `v2_game_frames`, final) |
| `step_s` | smallint | seconds of game time between two points (10) |
| `first_frame` | timestamp | wall time of game time 0 |
| `duration_s` | int | game time of the last frame |
| `points` | int | length of every series array of this game |
| `wall_time` | timestamp[] | wall time of each point: jumps over pauses |
| `live_state` | jsonb, NULL | internal, where the live loop is; NULL once built from the archive |
| `built_at` | timestamp | |

### `v2_game_team_series`

Two rows per game, primary key `(game_id, team_id)`. Every array has `points` values, point `k` (0-based) at index `k + 1`.

| Column | Type | Meaning |
|---|---|---|
| `team_id` | → `v2_team` | |
| `side` | text | `blue`, `red` |
| `gold`, `kills`, `towers`, `inhibitors`, `barons` | int[] | team totals |
| `dragons` | int[] | dragons taken so far (count); their types are in the markers |

### `v2_game_player_series`

Ten rows per game, primary key `(game_id, participant_id)` (1-5 blue, 6-10 red). Join `v2_game_player` on `(game_id, participant_id)` for the player, team, champion and role.

| Column | Type | Meaning |
|---|---|---|
| `gold`, `level`, `kills`, `deaths`, `assists`, `cs`, `hp`, `max_hp` | int[] | from `window` (`gold` = gold in hand, `hp` = current health) |
| `gold_earned` | int[] | from `details`: total gold earned |
| `kill_participation`, `damage_share` | real[] | 0 to 1 |
| `wards_placed`, `wards_destroyed` | int[] | |
| `attack_damage`, `ability_power`, `attack_speed`, `life_steal`, `armor`, `magic_resist` | int[] | champion stats at that point |
| `crit_chance`, `tenacity` | real[] | |
| `items` | int[] | the items of every point one after the other, in the feed's order (trinket included, one entry per unit of a stack: 2 control wards are `2055, 2055`). Up to 11 seen in one point |
| `item_counts` | int[] | how many of `items` belong to each point, NULL when the point has no `details` frame. `v2_items_at(items, item_counts, k)` gives point `k` |
| `rune_style`, `rune_sub_style` | int, NULL | from the first frame that has them |
| `runes` | int[], NULL | |
| `skill_order` | text[] | skills in the order taken (`Q`, `W`, …) |
| `skill_time` | int[] | game time (s) each one was taken, same length as `skill_order` |

### `v2_game_marker`

Objectives, one row each, index on `(game_id, game_time_s)`.

| Column | Type | Meaning |
|---|---|---|
| `game_id` | → `v2_game` | |
| `game_time_s` | int | game time, s |
| `kind` | text | `dragon`, `baron`, `tower`, `inhibitor` |
| `team_id` | → `v2_team` | the team that took it |
| `detail` | text, NULL | dragon type (`infernal`, `ocean`, `elder`, …) |
| `undone_by_pause` | smallint, NULL | the objective was taken back by the chronobreak at the end of this pause (`v2_game_pause.number`). It stays here, but the series don't count it. Taken again after, it's a new row |

### `v2_game_pause`

Pauses of the game, one row each, primary key `(game_id, number)`. No rows: the game had no pause the feed marked.

| Column | Type | Meaning |
|---|---|---|
| `game_id` | → `v2_game` | |
| `number` | smallint | 1, 2, … in the order of the game |
| `game_time_s` | int | game time when it paused, s |
| `started_at` | timestamp | wall time of the first `paused` frame |
| `ended_at` | timestamp, NULL | wall time of the first frame in play after it (or the `finished` one). NULL while the game is still paused, or when the feed ends paused |
| `rollback_s` | int, NULL | chronobreak: game seconds the game went back when it came back from this pause. NULL: a normal pause (or live, see below) |
| `counters_reset` | boolean | the feed started its counts again at 0 after this pause (seen with every chronobreak so far) |

### `v2_game_rollback_team`, `v2_game_rollback_player`

What a chronobreak took back, one row per team (`team_id`) or player (`participant_id`), primary key `(game_id, number, …)` with `number` → the pause in `v2_game_pause`. Values are what was there at the pause minus what the game went back to: `kills`, `gold`, `towers`, `inhibitors`, `barons`, `dragons` for teams; `kills`, `deaths`, `assists`, `gold`, `cs` for players. Only for pauses with `rollback_s`.

The feed sends very few frames during a pause, often one and then nothing for minutes (80 min seen, DK-DRX 2024-02-25 game 2). Seen: 3 pauses of 11, 16 and 38 min in game 1 of that series, 2 of 59 and 80 min in game 2 (2:58 of wall time for 39 min of game).

## Game time and points

- The feed's timestamps are wall time. Game time only advances while the game is in play: time while `paused` doesn't count, the `finished` frame is the end.
- Point `k` is the last frame with game time ≤ `k × step_s`. The last point is the last frame, even when it isn't on a step. So `points` = game time / step + 1, plus one when the game doesn't end on a step.
- Between two frames more than 30 s apart in play, the points in between are NULL (feed hole). Up to 30 s, the last value is kept. During a pause there are no points at all.
- Markers come from counters going above the highest count seen so far. A counter going down (a chronobreak or a feed fix) is logged and makes no marker, also when it comes back up. A jump of more than 11 between two frames is a feed error: logged, no markers.
- A frame without a dragon list gives a NULL point, not 0.
- A frame more than 6 h from the first one is a feed error: the game fails that try (the longest game seen took 2:58 with its pauses).
- A NULL value in an array: no data for that point.

## Chronobreaks

Riot can take a game back to an earlier moment (a chronobreak). Seen twice (G2-MKOI 2025-09-14 game 3, FUR-GAM 2025-06-28 game 5), both the same way in the feed:

1. The game pauses. Minutes without frames.
2. Frames with every player at level 1 and no gold (a server starting again): skipped, no points, no game time.
3. The game comes back from a moment a few seconds before the pause (5 and 8 s seen): gold, level and CS restored.
4. The feed's counts start again at 0: team kills, towers, inhibitors, barons, dragons, player kills/deaths/assists, wards, skills. The item lists only have what was bought after.

What's stored:

- The series show the game as it went on: game time goes back to the snapshot (the last frame before the pause with no more gold than after), the points after it are the ones played after the restart. Counts the feed started again get what they had at the snapshot added, so they keep adding up.
- Items after a restart are NULL (`item_counts` NULL): the feed's list isn't the inventory any more.
- The pause gets `rollback_s` and `counters_reset`, what was taken back goes to `v2_game_rollback_team` / `v2_game_rollback_player`, objectives taken back stay in `v2_game_marker` with `undone_by_pause`.
- The archive keeps every frame, the seconds taken back included.
- Live can't find the snapshot (it doesn't keep the frames): it skips the zero frames and keeps the counts adding up from the pause, with `rollback_s` NULL. The after-game write puts the rollback in.
- The final stats in `v2_game_team` / `v2_game_player` aren't fixed for a chronobreak yet (ROADMAP).

## How it's filled

| | Live (`v2 frames live`) | After the game (`v2 frames`) |
|---|---|---|
| When | every 10 s, for games `inProgress` | queue of `completed` games without frames |
| Reads | the feed's newest 10 s (it serves windows that ended 50 s ago), catching up 2 min per tick if it's behind | the whole game, 10 s at a time (~400 calls) |
| Writes | appends points and markers, adds or closes pauses, `source = 'live'` | the archive, then rebuilds every series row, markers and pauses, `source = 'archive'` |
| Delay | points 60-90 s behind the game | a few minutes after the end, older games only with the backfill |

- The after-game write is all or nothing: archive, timeline, 2 team rows, 10 player rows, the markers and the pauses, or none of it.
- Teams are the match's (`v2_event_team`): a feed team the match doesn't list takes the match's other team, like the game details (logged). So the series teams are always the ones in `v2_game_team`.
- The walk goes on through pauses (the feed can go 80 min without a frame). It stops after 30 min without one only when the game's end isn't known or the walk is past it, and 6 h after the first frame at most. Without a `finished` frame and before the game details know the end, a walk counts as failed and is tried later (on the last try it's kept).
- A feed or network outage (no answer, 5xx, 429 after 3 retries) pauses the job 5 min and counts no try.
- Live only writes while the game is `inProgress`. A game whose live write fails waits a minute before the next try.
- The live points are replaced by the archive's when the game is done. Live holes (missed ticks, restarts) are gone then.
- `TRIFORCE_BACKFILL_FRAMES=off` (default): only games of matches from the last 3 days, and games that had live points. `on`: every completed game, newest first (~23k games since 2023-10, days of calls).
- `rebuild-timeline --all` or `rebuild-timeline <game ext id>…` builds the series again from the archive, without the feed (a new step, a new column, a fix).

## `v2_game.frames`

| Value | Archive | Series | Can still change |
|---|---|---|---|
| NULL | no | no | yes |
| `live` | no | `source = 'live'`, may have NULL holes | yes, every 10 s while played, then replaced |
| `done` | yes | `source = 'archive'` | only by `rebuild-timeline` |
| `no_feed` | no | whatever live got, if anything | no |

`frames_tries` and `frames_next_try`: a failed walk waits 10 min, 40 min, 2 h 40, 10 h 40; the 5th failure makes the game `no_feed`. A 204 from the feed makes it `no_feed` at once.

## Reading it

One game's team gold and the difference, point by point:

```sql
SELECT i - 1 AS point, (i - 1) * t.step_s AS game_s, b.gold[i] AS blue, r.gold[i] AS red, b.gold[i] - r.gold[i] AS diff
FROM v2_game g
JOIN v2_game_timeline t ON t.game_id = g.id
JOIN v2_game_team_series b ON b.game_id = g.id AND b.side = 'blue'
JOIN v2_game_team_series r ON r.game_id = g.id AND r.side = 'red'
CROSS JOIN generate_series(1, t.points) AS i
WHERE g.ext_id = 115660540726095095
ORDER BY i;
```

Gold difference at 15:00 for every finished game of a league:

```sql
SELECT g.ext_id, b.gold[900 / t.step_s + 1] - r.gold[900 / t.step_s + 1] AS blue_lead
FROM v2_game g
JOIN v2_event e ON e.id = g.event_id
JOIN v2_league l ON l.id = e.league_id
JOIN v2_game_timeline t ON t.game_id = g.id AND t.source = 'archive'
JOIN v2_game_team_series b ON b.game_id = g.id AND b.side = 'blue'
JOIN v2_game_team_series r ON r.game_id = g.id AND r.side = 'red'
WHERE l.slug = 'worlds' AND t.duration_s >= 900;
```

A player's items at one point (point 90, 15:00):

```sql
SELECT gp.summoner_name, gp.champion, v2_items_at(s.items, s.item_counts, 90) AS items
FROM v2_game g
JOIN v2_game_player_series s ON s.game_id = g.id
JOIN v2_game_player gp ON gp.game_id = g.id AND gp.participant_id = s.participant_id
WHERE g.ext_id = 115660540726095095;
```

The markers of a match, with the team code:

```sql
SELECT gm.number, m.game_time_s, m.kind, m.detail, tm.code
FROM v2_event e
JOIN v2_game gm ON gm.event_id = e.id
JOIN v2_game_marker m ON m.game_id = gm.id
JOIN v2_team tm ON tm.id = m.team_id
WHERE e.ext_id = 115660540726095094
ORDER BY gm.number, m.game_time_s;
```

Pauses of a game and how long they took:

```sql
SELECT p.number, p.game_time_s / 60 AS game_min, p.started_at, p.ended_at, round(extract(epoch FROM p.ended_at - p.started_at) / 60) AS minutes
FROM v2_game g
JOIN v2_game_pause p ON p.game_id = g.id
WHERE g.ext_id = 111561337007305538
ORDER BY p.number;
```

Pauses and chronobreaks of a game, with what each chronobreak took back:

```sql
SELECT p.number, p.game_time_s, round(extract(epoch FROM p.ended_at - p.started_at) / 60) AS minutes, p.rollback_s, p.counters_reset,
  (SELECT string_agg(t.code || ' ' || r.kills || ' kills, ' || r.towers || ' towers, ' || r.dragons || ' dragons', '; ') FROM v2_game_rollback_team r JOIN v2_team t ON t.id = r.team_id WHERE r.game_id = p.game_id AND r.number = p.number) AS taken_back,
  (SELECT count(*) FROM v2_game_marker m WHERE m.game_id = p.game_id AND m.undone_by_pause = p.number) AS objectives_undone
FROM v2_game g
JOIN v2_game_pause p ON p.game_id = g.id
WHERE g.ext_id = 113487569888189690
ORDER BY p.number;
```

Sizes, without reading the blobs:

```sql
SELECT count(DISTINCT game_id) AS games, sum(octet_length(data)) / 1024 / 1024 AS archive_mb, sum(raw_bytes) / 1024 / 1024 AS json_mb
FROM v2_game_frames;
```

The archive needs zstd to read: `SELECT encode(data, 'hex') …`, unhex, zstd-decompress, parse the JSON. In Triforce: `frames::pack::unpack`.

Sizes measured (a 33 min Worlds game): 233 KB of archive (96 KB `window`, 137 KB `details`) for 56 MB of JSON, about 5-6 GB for the whole history; series about 90 KB a game (about 2 GB).

## What isn't there

- Positions on the map (the feed has none): no kill or ward locations.
- The champion in the series rows: they only have the participant id, the champion is in `v2_game_player.champion`.
- Rift Herald, Voidgrubs, Atakhan, plates: the feed doesn't count them.
- Frames of leagues without feed (`asian_games`, `cacg`, `pcs` seen) and of games before 2023-10 (the feed has some, 2022 checked, but `scheduleEvents` doesn't list those games).
- Frames read while live aren't kept: the archive is fetched again after the game.
