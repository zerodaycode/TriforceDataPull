# Game frames: archive, series, markers

Every frame the live feed sends for a game, about 4 a second, and what's built from them: one point every 10 s of game time for each team and player, plus markers for objectives. Background: [ADR 0007](../decisions/0007-v2-game-frames.md), [storage investigation](../investigations/2026-11-01-game-frames-storage.md). The last frame alone, as plain columns, is in [game-details.md](game-details.md).

Two layers:

- **Archive** (`v2_game_frames`): the frames as the feed sent them, compressed. Kept so anything can be built again later, even if the feed goes away.
- **Series** (`v2_game_timeline`, `v2_game_team_series`, `v2_game_player_series`, `v2_game_marker`): typed values per point, built from the archive (or from the live feed while a game is played).

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
| `items` | int[] | 10 per point, flat: point `k` is `items[10k+1 .. 10k+10]`. The items held, in the feed's order (trinket included), empty slots NULL |
| `rune_style`, `rune_sub_style` | int, NULL | from the first frame that has them |
| `runes` | int[], NULL | |
| `skill_order` | text[] | skills in the order taken (`Q`, `W`, …) |
| `skill_time` | int[] | game time (s) each one was taken, same length as `skill_order` |

### `v2_game_marker`

Objectives, one row each, index on `(game_id, t)`.

| Column | Type | Meaning |
|---|---|---|
| `game_id` | → `v2_game` | |
| `t` | int | game time, s |
| `kind` | text | `dragon`, `baron`, `tower`, `inhibitor` |
| `team_id` | → `v2_team` | the team that took it |
| `detail` | text, NULL | dragon type (`infernal`, `ocean`, `elder`, …) |

## Game time and points

- The feed's timestamps are wall time. Game time only advances while the game is in play: time while `paused` doesn't count, the `finished` frame is the end.
- Point `k` is the last frame with game time ≤ `k × step_s`. The last point is the last frame, even when it isn't on a step. So `points` = game time / step + 1, plus one when the game doesn't end on a step.
- Between two frames more than 30 s apart in play, the points in between are NULL (feed hole). Up to 30 s, the last value is kept. During a pause there are no points at all.
- Markers come from counters going above the highest count seen so far. A counter going down (a chronobreak or a feed fix) is logged and makes no marker, also when it comes back up. A jump of more than 11 between two frames is a feed error: logged, no markers.
- A frame without a dragon list gives a NULL point, not 0.
- A frame more than 4 h from the first one is a feed error: the game fails that try.
- A NULL value in an array: no data for that point.

## How it's filled

| | Live (`v2 frames live`) | After the game (`v2 frames`) |
|---|---|---|
| When | every 10 s, for games `inProgress` | queue of `completed` games without frames |
| Reads | the feed's newest 10 s (it serves windows that ended 50 s ago), catching up 2 min per tick if it's behind | the whole game, 10 s at a time (~400 calls) |
| Writes | appends points and markers, `source = 'live'` | the archive, then rebuilds every series row, `source = 'archive'` |
| Delay | points 60-90 s behind the game | a few minutes after the end, older games only with the backfill |

- The after-game write is all or nothing: archive, timeline, 2 team rows, 10 player rows and the markers, or none of it.
- Teams are the match's (`v2_event_team`): a feed team the match doesn't list takes the match's other team, like the game details (logged). So the series teams are always the ones in `v2_game_team`.
- The walk goes on through pauses (the feed can go 12 min without a frame) and stops after 30 min without one. Without a `finished` frame and before the game details know the end, a walk counts as failed and is tried later (on the last try it's kept).
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
SELECT gp.summoner_name, gp.champion, s.items[90 * 10 + 1 : 90 * 10 + 10] AS items
FROM v2_game g
JOIN v2_game_player_series s ON s.game_id = g.id
JOIN v2_game_player gp ON gp.game_id = g.id AND gp.participant_id = s.participant_id
WHERE g.ext_id = 115660540726095095;
```

The markers of a match, with the team code:

```sql
SELECT gm.number, m.t, m.kind, m.detail, tm.code
FROM v2_event e
JOIN v2_game gm ON gm.event_id = e.id
JOIN v2_game_marker m ON m.game_id = gm.id
JOIN v2_team tm ON tm.id = m.team_id
WHERE e.ext_id = 115660540726095094
ORDER BY gm.number, m.t;
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
- Frames of leagues without feed (`asian_games`, `cacg`, `pcs` seen) and of games before 2023-10.
- Frames read while live aren't kept: the archive is fetched again after the game.
