# Old API vs new API: live check and data comparison

Date: 2026-10-17 (tests from 2026-10-16 22:25 to 23:33 UTC)

Two checks for [ADR 0003](../decisions/0003-migrate-to-lolesports-graphql-api.md): the new API during a real live match, and our database (filled from the old API) against the new API, row by row. Background: [old API](2026-10-16-lolesports-old-api-and-live-feed.md), [GraphQL API](2026-10-16-lolesports-graphql-api.md), [migration map](2026-10-16-lolesports-api-migration-map.md).

## Live check: FUR vs C9

Worlds, Bo5, match `115660588583171794`. Sources polled with a small read-only script (stdlib Python, identifying User-Agent):

- old `getLive`
- new `watchLiveQuery` (`eventState: ["inProgress"], eventType: "all"`)
- new `scheduleEvents` (`eventState: ["inProgress"]`)
- the live feed `window` for game 3 (`115660588583171797`)

### Same content

| | old `getLive` | new `watchLiveQuery` | new `scheduleEvents` |
|---|---|---|---|
| The live match | yes | yes | yes |
| Score, game ids and states | same | same | same |
| Best-of | `{type: bestOf, count: 5}` | same | same |
| Event `state` | `inProgress` | **not there** | `inProgress` |
| Team id | `id` | composite `"<matchId>:<teamId>"` | `teamId` |

`watchLiveQuery` event fields: `blockName, id, league, match, startTime, streams, tournament, type`. So the adapter has to set the state itself, or read it from `scheduleEvents`.

`getEventDetails` vs `watchVodQuery` for this match and the one before (KC 3-0 MVK) are the same too: game ids, game states (also `unneeded`), sides, VODs per game (1 and 4), streams (25 and 0) and scores. Checked again with both answers saved at 23:46 (game 4 live), still equal. The new one adds `strategy.type` and the tournament record (W/L) per team.

### Caching

66 answers of each source:

- `watchLiveQuery` and `scheduleEvents`: `max-age=120, stale-while-revalidate=60` during the live match too. `age` went up to 155 s (`watchLiveQuery`) and 159 s (`scheduleEvents`), so copies older than `max-age` are really served. One `watchLiveQuery` answer had `max-age=17, stale-while-revalidate=0`.
- old `getLive`: `max-age=60, stale-while-revalidate=60`, so up to ~2 min by header. `age` up to 63 s, over 60 in 9 answers.
- The two CDNs use `Date` in a different way. CloudFront (old API, feed): `Date` is when the answer was made, `Date + Age` = now. Netlify (new API): `Date` is now, the answer was made at `Date - Age`.

### End of game 3

| Source | Says finished/completed, answer made at (UTC) | After the end |
|---|---|---|
| feed `window` | frame `23:21:41.033` with `gameState: finished` (we got it 43 s later) | 0 |
| old `getLive` | 23:25:29 | +3 min 48 s |
| new `watchLiveQuery` | 23:26:37 | +4 min 56 s |
| new `scheduleEvents` | 23:26:37 (same answer time) | +4 min 56 s |

Game 3 is the only game measured against the feed. Game 2 was polled every 60 s, without the feed and with one poll lost (22:30:49):

| Source | Answer made at | Game 2 |
|---|---|---|
| new `watchLiveQuery` | 22:31:49 | `completed` |
| new `scheduleEvents` | 22:31:49 (same copy still served at 22:33:50) | `inProgress` |
| old `getLive` | 22:32:13 | `completed` |

Same answer time, different data in the two new operations. So the answer time only orders answers of the same operation, and neither API is faster every time. Only the feed is close to real time.

### The old API went back

The old `getLive` answers for game 3, in the order we got them:

| We got it | Answer made at | Game 3 |
|---|---|---|
| 23:25:35 | 23:25:29 | `completed` |
| 23:26:06 | 23:25:13 | `inProgress` |
| 23:26:37 | 23:26:13 | `completed` |

The second answer is an older cached copy, served after a newer one. It happened earlier too, without a state change: at 22:27:27 we got a copy made at 22:27:13, at 22:27:58 one made at 22:27:03. A client that takes every answer as it comes sees a finished game going live again. Triforce does that today. The fix is the forward only rule in [the live state design](2026-10-16-live-match-state-design.md#state-machines-forward-only): keep the time each answer was made (CloudFront `Date`, Netlify `Date - Age`) and ignore answers older than the newest we have.

### Live feed

`feed.lolesports.com/livestats/v1/window/{gameId}` worked during the match: HTTP 200, `x-cache: Hit from cloudfront`, `max-age=60`. First frame of game 3 at 22:47:32, ~4 frames per second. The `esportsTeamId`s in the feed are the same team ids of both APIs.

## Data comparison: our DB vs the new API

DB filled from the old API on 2026-10-16 (Triforce runs), compared with `GetOtherLeagues`, `GetLoLTeamOverview` and `scheduleEvents` (`pageSize: 80`, paged over the same time window, 28 pages) the same night.

| Data | Result |
|---|---|
| Leagues | 51 / 51, `slug`, `name`, `region`, `image` equal |
| Tournaments | 513 / 513 equal, plus 1 new one in the API (`lcp_promotion_2026`, created after our last run) |
| Tournament dates | Our `start_date`/`end_date` = the **UTC date** of `startTime`/`endTime` in all 513. Times are like `22:00`/`23:00` UTC (midnight in Europe) and the old API kept the UTC date |
| Teams | 1,586 / 1,586 equal, `home_league` too: today's lookup by name gives the same league as `homeLeague.id` |
| `alt_image_url` | 1,325 teams have it. The new API has no such field |
| Players | 4,526 / 4,526 equal. `handle` = `summonerName` for all of them |
| Team-player links | 5,431 / 5,431 equal |
| Schedule | 2,086 matches in both, `start_time`, `blockname`, league and best-of equal. 7 matches only in the API. 2 with other state/wins and 2 TBD slots filled with a real team: they changed after our last run. The live match FUR-C9 (`115660588583171794`) has the teams the other way: our DB has C9 left and FUR right, every API sends `[FUR, C9]` |
| Shows | 0 in our DB, 0 in `scheduleEvents` (`eventType: all`) for this window |

### Schedule teams: wrong team by name

Our schedule finds the teams by name (`src/dao/mod.rs`, `db_team.name.eq(&team_1.name)`), the old schedule has no team ids. 49 team names are used by 104 teams, so the lookup can take the wrong one:

| Name | Matches with the wrong team | Our link | Real team (`teamId`) |
|---|---|---|---|
| Malvinas Gaming | 10 | `107598840547235130` (MVG, other league) | `107598878262940546` |
| Polar Squad | 8 | `108121051709036068` (PLSQ) | `108121057213928999` (PLS) |
| DFM Academy | 6 | `106827765448944398` (DFM) | `113871708167519028` (DFMA) |

24 matches in total. The new API's `matchTeams[].teamId` fixes it. With the old API we can match by name + code + image: the image is different in all 49 shared names, and on the first `getSchedule` page (80 matches) all 86 real team slots had the same image as our `image_url`. Name + code + league isn't enough, it leaves 15 of the 49 names with more than one team.

Team order: the live update copies the wins by position (`src/dao/models/event.rs`, `teams.first()` goes to `team_left_wins`). For FUR-C9 that writes FUR's wins in C9's column. Wins have to go by team, not by position, with both APIs.

TBD slots: we link a placeholder team called "TBD" (82 slots, all TBDA). The new API sends `teamId: "0"` with name and code "TBD". When we migrate, `"0"` becomes "no team" (NULL). `GetLoLTeamOverview` still returns the 3 TBD teams (TBDA, TBDC, TBDD), so if we remove them the teams sync has to skip them. The old `getSchedule` sends code `TBD`, no stored team has it, so name + code + image gives NULL too.

## What it means for Triforce

- Leagues, tournaments, teams, players and links are the same data: moving them to the new API is safe. Keep `alt_image_url` (only the old API has it) or drop the column.
- Tournament dates: take the UTC date of `startTime`/`endTime`. No column change needed.
- Schedule: `teamId` fixes 24 wrong team links, and TBD (`teamId: "0"`) becomes NULL instead of a fake team. Live wins go by team, not by position.
- Live: same data in both APIs. In game 3 (one game measured) both were 3.5-5 min after the real end. The feed is the only fast source.
- The old API served an older answer after a newer one. The new one also serves stale copies (`stale-while-revalidate`), we didn't see it go back but nothing stops it. Keep the time each answer was made per operation and never go back, with both APIs. Two operations with the same answer time can still disagree, so state and score never go back whatever the source.
