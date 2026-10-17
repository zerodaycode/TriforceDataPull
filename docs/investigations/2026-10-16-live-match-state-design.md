# Live match state: what we learned and a design

Date: 2026-10-16

Design notes, nothing of this is in the code yet. Based on the measurements in [the old API investigation](2026-10-16-lolesports-old-api-and-live-feed.md) (batch 3: a full Bo5, batch 2: the end of a game 5). Today Triforce decides a match ended when it leaves `getLive`, see [the migration map](2026-10-16-lolesports-api-migration-map.md#how-triforce-decides-a-match-ended).

## What we know

| Question | Answer |
|---|---|
| Where is best-of? | `getSchedule` / `getLive` / `getEventDetails` → `strategy.count`. Worlds Swiss mixes Bo1 and Bo3 in the same stage, read it per match |
| When is a game really live? | When the feed returns its first frame (`200` after the `204`s). API `inProgress` means "current game of the series", at Worlds it was set 4-16 min before play (break, side selection, draft) |
| How do I know a game ended? | Feed `gameState: "finished"`, visible 33-47 s after the end. The API confirms (game `completed` + the winner's `gameWins` +1) ~3-4 min after the end. The feed never says who won, wait for the API |
| How do I know a series ended? | In the same API update that confirms the last game: a team reaches `ceil(count/2)` wins, the other games become `unneeded`, `streams` empties. `getLive` drops it and `getSchedule` sets `outcome` + `completed` within ~16 s |
| Was C9-KC "stuck live"? | No, it was really in game 5 (batch 2) |
| Do the recordings explain the old bugs? | No. The 3-4 min delay is normal. The bugs were rare, mostly tier 2/3, and lasted hours to days (P1-P3) |
| Are completed results consistent? | Yes: 172/172 completed matches on the new site, and every state change in batch 3 |
| Is `startTime` reliable? | Missing for TBD matches, and in the new site Swiss payload even for completed matches. Merge from more than one source |

## Polling plan

| What | Endpoint | How often |
|---|---|---|
| Upcoming and recent matches | `getSchedule` (+ `pageToken` for older pages) | every 5-10 min, every 1 min when a match starts in less than 30 min |
| Everything live (scores, games, sides, best-of) | `getLive` | every 60 s, that's its cache, faster gives nothing |
| Live frames of the current game | feed `window`, consecutive 10 s windows from where we left off, up to `floor10(server_now - 40 s)` | every 10 s when frames come, every 30 s while it answers `204` |
| Final check | `getEventDetails` once when a match leaves `getLive`, plus a "settle" pass some hours later | once |
| Fallback | new site GraphQL / stage page (`RSC: 1`) | only if the old API fails |

About 1 request per minute for the API plus 6 per minute per live game. Every 10 s window has new data, so this is the minimum that doesn't miss anything. Today Triforce polls `getLive` every 2 min and doesn't use the feed.

## State machines (forward only)

```
GAME    unstarted ─► current ─► live ─► [paused ⇄ live] ─► ended ─► confirmed
                     (API inProgress)  (first feed     (feed        (API completed
                      feed 204)         frame)          "finished")  + gameWins +1)
        unstarted ─► unneeded           (API, after the series is decided)

SERIES  unstarted ─► live ─────────────► decided ─────────────────► confirmed
                     (API inProgress)    (API gameWins reaches       (getSchedule outcome set,
                                          ceil(count/2))              left getLive)
```

Rules:

1. Forward only. Save the state and score with the time the answer was made (CloudFront: `Date`, Netlify: `Date - Age`). Ignore any answer older than what we have, and a score never goes down. Seen on 2026-10-16: the old `getLive` said game 3 `completed`, then `inProgress` again from an older cached copy, then `completed` ([live check](2026-10-17-old-vs-new-api-comparison.md#the-old-api-went-back)).
2. "Live" comes from the feed, not from the API. Show "current game: waiting" while the feed answers `204`.
3. A game that `ended` (feed) has no winner yet. Show "finished, waiting for result" for up to ~5 min until the API confirms. Don't guess the winner from kills or towers.
4. The score decides the series, with `strategy.count`. `unneeded` games, an empty `streams` and leaving `getLive` confirm it.
5. Sides for the next game are a default until they're picked after the previous game. Trust them when they change (or when the feed metadata confirms them at game start).
6. Teams by id (`getLive`/`getEventDetails`) or by code (`getSchedule` has no ids), never by position in the array.
7. Last resort: a series `live` with no feed data and no API change for 6 h becomes "unconfirmed", the settle pass fixes it.
8. "Completed" can change later (P3). Save each change as a new row with its source time, so the forward-only check doesn't reject a real correction. Check completed matches again at +3 h, +24 h and +72 h. Rule 1 is for a live series, corrections after the end are a different, logged event.

With the new GraphQL API the same rules apply, with the answer time kept per operation (`watchLiveQuery` and `scheduleEvents` disagreed with the same answer time). Its cache is longer (120 s + 60 s stale vs 60 s + 60 s), but in the live check both APIs confirmed the end of game 3 3.5-5 min late. Keep the window at ~5 min for both. See [the migration map](2026-10-16-lolesports-api-migration-map.md#getlive--watchlivequery-tested-live).
