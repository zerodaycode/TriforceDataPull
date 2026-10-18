# Live recording and feed history

Date: 2026-10-18

What a full recording of two Worlds Bo5 tells about sides, team order and answers that go back, and what the live feed and `scheduleEvents` give for past matches. Sources: a `recorder.py` run (old API + live feed, 2026-10-16 18:03 to 2026-10-17 04:50 UTC, not in git), calls to the feed and to `scheduleEvents` on 2026-10-18. Background: [old API and live feed](2026-10-16-lolesports-old-api-and-live-feed.md), [live match state](2026-10-16-live-match-state-design.md), [old vs new API](2026-10-17-old-vs-new-api-comparison.md).

## The recording

| Match | Id | Result | Games with feed |
|---|---|---|---|
| KC vs MVK | `115660588583171790` | KC 3-0 | 3 |
| FUR vs C9 | `115660588583171794` | FUR 3-1 | 4 |

`getLive` and `getSchedule` every 60 s, `getEventDetails` every 30 s, feed `window` and `details` every 10 s while a game was live. Old API only. The [comparison](2026-10-17-old-vs-new-api-comparison.md) showed the new API sends the same content, the team order flip below still has to be seen on GraphQL.

## Sides

| Game | First feed frame | Feed blue team, all answers | API's last side change | API final = feed |
|---|---|---|---|---|
| KC-MVK 1 | 18:11:38 | always the same | default kept | yes |
| KC-MVK 2 | 19:01:17 | same | 18:58:44 | yes |
| KC-MVK 3 | 19:55:30 | same | 19:50:46 | yes |
| FUR-C9 1 | 20:53:43 | same | 20:50:53 | yes |
| FUR-C9 2 | 22:17:10 | same | 21:51:32 | yes |
| FUR-C9 3 | 22:47:31 | same | 22:45:02 (changed 3 times in 2 min) | yes |
| FUR-C9 4 | 23:43:21 | same | default kept | yes |

- Before a game the API sends default sides (they alternate per game). The real ones come during champ select: the team that lost the previous game picks side or first pick.
- No side changed after the first frame, in 7 games.
- The feed's `gameMetadata` has the sides from the game itself: we take those as the real ones, the API ones before the game are provisional.

## Team order

Every answer sends the teams with game 1's blue team first. When game 1's sides are set the order changes: FUR-C9 went from `[C9, FUR]` to `[FUR, C9]` at 20:50:53, in `getSchedule`, `getLive` and `getEventDetails`.

So the position of a team in the answer is not stable. A team, its wins and its outcome must always be read and stored together. This is why v1 has FUR's wins in C9's column ([comparison](2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name)).

## Going back with a newer answer

| Source | Answer made at | Says |
|---|---|---|
| `getSchedule` | 20:38:35 | KC-MVK `completed` 3-0 |
| `getSchedule` | 20:40:07 | KC-MVK `inProgress` 3-0 |
| `getSchedule` | 20:40:38 | KC-MVK `completed` 3-0 |
| `getEventDetails` | 21:37:40 | FUR-C9 game 1 `completed`, 0-1 |
| `getEventDetails` | 21:38:40 | game 1 `inProgress`, 0-0 |
| `getEventDetails` | 21:39:40 | game 1 `completed`, 0-1 |

The answer time goes forward and the data goes back. Dropping older answers isn't enough: state and wins have to be compared too, and never go down.

## Completed before the final score

`getSchedule` said FUR-C9 `completed` with FUR 2-1 at 00:15:29, and 3-1 with the outcome at 00:17:30. A `completed` state doesn't mean the score is final, the winner comes from the outcome.

## Game end

Every time a game became `completed`, the same answer gave the winner +1 in `gameWins` (7 of 7). No API sends a winner per game, this is the only way to know it, so it needs someone watching while it happens (or a sweep: 2-0, 3-0).

The API confirmed the end of a game 269 to 461 s after the feed's last frame. Measured only at Worlds, smaller events may be different: nothing should wait a fixed time.

## Feed for past games

`feed.lolesports.com/livestats/v1` also answers for finished games, without a key.

| Call | Gives | Size |
|---|---|---|
| `window/{gameId}` | `gameMetadata`: patch, blue/red `esportsTeamId`, 10 players (`esportsPlayerId`, `summonerName`, `championId`, `role`), and the first frames | ~17 KB |
| `window/{gameId}?startingTime=<now>` | the same metadata and the **last** frames: team gold, kills, towers, inhibitors, barons, dragons | ~18 KB |
| `details/{gameId}?startingTime=<now>` | last frames per player: level, KDA, gold, CS, items, runes, skill order, damage share, wards | ~58 KB |

- Any `startingTime` after the end gives the last frames, so the end time isn't needed.
- Checked on games from 2023-10 (Worlds), 2026-04 (LEC) and 2026-10 (Worlds). The 2026-04 game has no `finished` frame, its last frames say `in_game`.
- One completed game per league from 2026-04 to 2026-10: 36 of 39 leagues answered with 10 of 10 players. `asian_games`, `cacg` and `pcs` answered `204`.
- The new site doesn't use the feed, it can go away without notice.

## `scheduleEvents` history

- Nothing before Worlds 2023 (2023-10-15): earlier date ranges answer 0 events.
- `eventDateStart` needs `eventDateEnd`, without it the answer is an error (`Invalid eventDateStart/eventDateEnd`) with HTTP 200.
- With both dates the pages come oldest first, `newer` gives the next one, the edges of two pages can repeat an event.
- From 2023-10-01, `pageSize: 100`: 115 pages, 11,484 matches, 0 shows, 28,210 games, 23,138 completed (2026-10-18).
- Outcomes: `win`, `loss` and `tie` (116 slots, `playAll 2` matches ending 1-1).
- Streams only while a match is live.
