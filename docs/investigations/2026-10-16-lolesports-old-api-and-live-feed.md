# LoL Esports old API and live feed

Date: 2026-10-16

What we know about the API Triforce uses today (`esports-api.lolesports.com/persisted/gw`) and the live stats feed (`feed.lolesports.com/livestats/v1`), from real captures. The new site API is in [the GraphQL investigation](2026-10-16-lolesports-graphql-api.md), the mapping between both in [the migration map](2026-10-16-lolesports-api-migration-map.md).

## Sources

| Batch | Source | What |
|---|---|---|
| 1 | lolesports.com, 9 HARs, 165 requests | Tournament, stage, results and handbook pages of the new site |
| 2 | andydanger.github.io/live-lol-esports, 1 HAR, 237 requests | Old API live: end of game 5 of C9 vs KC (Worlds Play-ins, 2026-10-15) |
| 3 | `recorder.py`, 146 min, 1.6 MB | A full Bo5: MVK 3-0 FUR (Worlds Play-ins, 2026-10-15 23:07 → 10-16 01:33 UTC), no pauses |
| 4 | lolesports.com, 3 HARs (filter "All", with JS bundles) | The persisted query manifest of the new site |
| Tests 1-3 | `gql_tool.py` | New API called from a script |

The tools are in [`tools/lolesports-explorer/`](../../tools/lolesports-explorer/).

## Summary

- The old API still works (checked 2026-10-15 22:47 UTC, and Triforce used it the whole day on 2026-10-16). Same public key as before, CORS `*`, cached 60 s (+60 s `stale-while-revalidate`, seen 2026-10-16).
- The new lolesports.com doesn't use it anymore, so Riot can turn it off without notice. Ids are the same in both APIs, so the new one works as fallback and cross check.
- Best-of is there: `getSchedule` and `getLive` → `match.strategy = {type: "bestOf", count: 5}`, `getEventDetails` → `match.strategy = {count: 5}`.
- Game end seen live: the feed `gameState` went from `in_game` to `finished` at 22:47:24.013 UTC. The feed doesn't say who won, the winner only shows as `gameWins` in the API.
- The API marks a game `completed` 3-4 min after the feed says `finished`. In the same update the next game goes to `inProgress`, ~16 min before it starts (break, side selection, draft). That's normal: API `inProgress` means "current game of the series".
- Series end was the same in every source within ~16 s (`getEventDetails`, `getLive`, `getSchedule`). Unplayed games became `unneeded` and the stream list was emptied.
- Not seen yet: a pause, and the tier 2/3 problems from the old project (see [Known problems](#known-problems-not-reproduced-yet)).

## Old API: `esports-api.lolesports.com/persisted/gw`

All endpoints:

- Header `x-api-key: 0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z` (public key of the web client, same as `src/service/caller.rs`)
- Param `hl=en-US`
- Response: `cache-control: max-age=60, public, stale-while-revalidate=60`, `access-control-allow-origin: *`

### `getSchedule?hl=en-US[&pageToken=…]`

```
data.schedule
  .pages { older, newer }            base64 tokens, e.g. "older::117322953110545728", pass one as pageToken
  .events[]                          80 per page (this page: 2026-10-05 → 2026-10-31)
     .startTime  .state (unstarted|inProgress|completed)  .type ("match")  .blockName ("Play-Ins", "Swiss", "Finals")
     .league { name, slug }
     .match
        .id                          same id as the new site
        .flags []                    hasVod, isSpoiler
        .strategy { type: "bestOf", count: 1|3|5 }
        .teams[] { name, code, image, result: {outcome, gameWins} | null, record: {wins, losses} | null }
```

- Teams have no id here. Team ids come from `getLive`, `getEventDetails` or `getStandings`.
- Team order can change. When the series ended, the next bracket match went from `TBD vs KC` to `KC vs MVK`, KC moved from position 2 to 1. Never match teams by array index.
- The bracket moves in the same update as the series end, both next matches got their teams at once.
- The page is a sliding window: an event that drops off the page isn't deleted.
- `flags: ["hasVod"]` appears with the first VOD (~17 min after game 1), it has nothing to do with the match state.
- Without a league filter the page mixes leagues (this one had Worlds, DCGI, EMEA Masters, LCS/CBLOL Promotion, CBLOL and VCS).

### `getEventDetails?hl=en-US&id={matchId}`

```
data.event
  .id .type ("match")  .tournament { id }  .league { id, slug, name, image }
  .match
     .strategy { count }                              ← no "type" here
     .teams[] { id, name, code, image, result: { gameWins } }   ← no outcome here
     .games[]
        .number  .id  .state (unstarted|inProgress|completed|unneeded)
        .teams[] { id, side: blue|red }
        .vods[] { parameter (YouTube id), provider, locale, offset, ... }
  .streams[] { provider (twitch|youtube|kick), parameter, locale, offset: -30000, statsStatus }
```

- It has the game ids the feed needs. `getLive` has them too for live matches.
- Game ids are not consecutive. C9-KC (batch 2): games 1-3 `115660588583171787-89`, games 4-5 `116299649500204436-37` (created later). MVK-FUR (batch 3): `115660588583040711`, `115660588583171784`, `115660588583171785`, `116299649500204434`, `116299649500204435`. Always read them from `games[]`.
- `unneeded` confirmed in batch 3: at 3-0, games 4 and 5 went from `unstarted` to `unneeded` in the same update as game 3 → `completed`.
- Sides of unplayed games are a default that alternates. The real side is picked after the previous game and the API updates it then. Game 2 showed FUR blue until 00:02:22, then MVK blue, ~4 min before the game started (00:06:04): that's the draft. The feed metadata (`blueTeamMetadata.esportsTeamId`) confirms it when the game starts.
- `vods[]` fill 13-17 min after each game.
- `streams[]` (25 during the series) was emptied at the series end. Streams have `offset: -30000`, the broadcast is ~30 s behind the game.

### `getStandings?hl=en-US&tournamentId={id}`

```
data.standings[].stages[] { name, slug } .sections[] { name, matches[], rankings[] }
  matches[] { id, state, previousMatchIds[], flags[],
              teams[] { id, slug, name, code, image, result: { outcome, gameWins } } }
```

For Worlds: Play-Ins (6 matches), Swiss (33) and Knockouts (7), the same matches as the new site.

### `getLive?hl=en-US`

```
data.schedule.events[]
   .id .startTime .state .type ("match" | "show") .blockName .league .tournament { id }
   .streams[]
   .match
      .id  .strategy { type, count }
      .teams[] { id, name, slug, code, image, result { outcome, gameWins }, record { wins, losses } }
      .games[] { number, id, state, teams[] { id, side }, vods[] }        ← same as getEventDetails
```

- One call has every live match with everything `getEventDetails` has. It changed at the same moments (same backend, 60 s cache).
- After the series the match left `getLive` and a `type: "show"` event (the broadcast, no teams) stayed. Filter on `type == "match"`.
- The CDN can serve an older copy after a newer one: on 2026-10-16 game 3 of FUR-C9 went `completed` → `inProgress` → `completed` in three polls ([live check](2026-10-17-old-vs-new-api-comparison.md#the-old-api-went-back)). `Date` is when the answer was made, use it to drop older answers.

## Live feed: `feed.lolesports.com/livestats/v1`

No API key. `cache-control: no-transform, max-age=60`, CORS `*`.

### `window/{gameId}[?startingTime=…]`: team totals

```
esportsGameId  esportsMatchId
gameMetadata
   .patchVersion ("16.20.826.3891")
   .blueTeamMetadata / .redTeamMetadata
       .esportsTeamId
       .participantMetadata[] { participantId 1-10, esportsPlayerId, summonerName, championId, role }
frames[]
   .rfc460Timestamp   .gameState (in_game | finished | paused)
   .blueTeam / .redTeam
       { totalGold, totalKills, towers, inhibitors, barons, dragons: ["hextech","mountain",…],
         participants[] { participantId, totalGold, level, kills, deaths, assists, creepScore, currentHealth, maxHealth } }
```

- Without `startingTime` you get the first frames of the game plus metadata.
- With `startingTime=T` you get the frames in `[T, T+10 s)`, about 4 per second (23-74 per window), not at regular intervals.
- Before the game starts the feed answers `HTTP 204`. In batch 3 that lasted 4-16 min while the API already said `inProgress` (draft and loading). The first `200` brings the first frame, gold starts going up ~4 s later.
- `gameState` never flickered: `in_game` all the game, then 8-9 `finished` frames in ~4 s, then the feed repeats old frames.
- `paused` is expected but not seen yet.

### `details/{gameId}?startingTime=…`: per player

`frames[] { rfc460Timestamp, participants[] { participantId, level, kills, deaths, assists, totalGoldEarned, creepScore, killParticipation, championDamageShare, wardsPlaced, wardsDestroyed, attackDamage, abilityPower, criticalChance, attackSpeed, lifeSteal, armor, magicResistance, tenacity, items[], perkMetadata { styleId, subStyleId, perks[] }, abilities[] } }`

- No `gameState` here, use `window` for state changes.
- ~3.5x bigger than `window` (~400 KB per 10 s).
- Item, rune and champion ids map through Data Dragon (`ddragon.leagueoflegends.com/cdn/16.20.1/data/en_US/item.json`, `runesReforged.json`).

### `startingTime` rules

Real error:

```json
{"httpStatus":400,"errorCode":"BAD_QUERY_PARAMETER",
 "message":"disallowed window with end time less than 20 sec old (was 9.51 sec old). requested window: [2026-10-15T22:47:10Z, 2026-10-15T22:47:20Z]) ahead of broadcast. current time: 2026-10-15T22:47:29.508Z"}
```

- A window is 10 s and must have ended at least 20 s ago, so `startingTime ≤ now - 30 s`. Use `floor_to_10s(now - 40 s)` to have margin. Format: `2026-10-15T22:47:10.000Z`.
- CloudFront caches the 400: the same URL kept returning it for 6-16 s (`x-cache: Error from cloudfront`). Asking too early costs a whole window.
- Use the server clock (`Date` + `Age` headers), not the local one. A few seconds of difference is enough to get 400s.

### Polling

- Data comes in 10 s windows and the CDN caches each URL 60 s, so poll every 10 s.
- andydanger polls `window` + `details` every 0.5 s, 20x more than useful: 228 feed calls in that HAR gave only 29 different responses, errors included.

### End of a game, seen live

Game 5, C9 vs KC, game id `116299649500204437`, KC on blue.

| UTC | Source | What happened |
|---|---|---|
| 22:16:01 | feed (first frame) | Game 5 starts |
| 22:47:22 | `getEventDetails` | KC 2 - C9 2. Games 1-4 `completed`, game 5 `inProgress` |
| 22:47:24.012 | feed frame | Last `in_game` frame. Blue 17 kills / 8 towers / 1 inhibitor, red 8 / 3 / 0 |
| 22:47:24.013 | feed frame | `gameState: "finished"` |
| 22:47:27.757 | feed frame | Last frame of the game (`finished`) |
| 22:47:50 | feed | The window with the end can be downloaded (~26 s after the end, the 30 s broadcast delay) |
| 22:48:00-22:48:19 | feed | Windows after the end (22:47:30, 22:47:40) return old frames from 22:47:22-26, a different subset each time |
| after 22:48:19 | not recorded | When the API caught up, batch 3 measured it (next section) |

For the parser:

1. `finished` is the end of the game, not of the series.
2. After the end the feed keeps sending old frames. Dedupe by `rfc460Timestamp` and ignore frames older than the newest you have.
3. The game winner is the team whose `gameWins` goes up in the API. Don't guess it from kills or towers.

## A full Bo5, measured (batch 3: MVK 3-0 FUR)

"API origin" is when Riot's server changed the answer, estimated from the `Date` header of the last old and first new response. "We saw it" includes our 30 s polling and the 60 s CDN cache.

| Game | API `inProgress` | Feed first frame | Feed `finished` | We saw `finished` | API origin `completed` | We saw `completed` |
|---|---|---|---|---|---|---|
| 1 | ≤ 23:07:25 (before recording) | 23:11:17 | 23:46:41 | +42 s | 23:49:11-23:50:08 (+2.5-3.5 min) | +3.5 min |
| 2 | 23:50:09 (same update as game 1 `completed`) | 00:06:04 (16 min later) | 00:41:00 | +47 s | 00:43:33-00:44:33 (+2.5-3.5 min) | +4.1 min |
| 3 | 00:45:06 (same update as game 2 `completed`) | 01:01:25 (16 min later) | 01:28:28 | +33 s | 01:30:51-01:31:55 (+2.4-3.5 min) | +4.0 min |

Series end (after game 3), in order:

| Source | We saw it | What changed |
|---|---|---|
| `getEventDetails` | 01:32:25 | 3-0, game 3 `completed`, games 4-5 `unneeded`, `streams` 25 → 0 |
| `getLive` | 01:32:41 (origin 01:32:22) | Match gone, only a `show` event left |
| `getSchedule` | 01:32:41 | `state: completed`, `outcome` win/loss, 3-0, next bracket matches filled |

Scores and states only moved forward in every source, no stale or contradictory answer during the whole series.

## Known problems (not reproduced yet)

From the old TriforceDataPull, rare and mostly on tier 2/3 matches (LCK CL, second divisions):

| # | Problem | How long |
|---|---|---|
| P1 | Match shown as live when it wasn't | unknown |
| P2 | Score missing or late | hours, sometimes a day |
| P3 | Completed match with a score (e.g. 2-0) rewritten to 0-0 | days later |

None of them appeared in batches 1-3 (all Worlds). Ideas, not checked:

- Tier 2/3 results may be entered by hand, without live stats.
- A 0-0 rewrite can be a voided or replayed match, a forfeit or a correction.

What to look for:

- matches still `inProgress` long after their start
- `completed` matches whose score doesn't reach `ceil(count/2)` (0-0, 1-0 in a Bo3)
- `completed` matches with games still `unstarted`
- tier 2/3 games without feed data

A possible link with how Triforce detects the end of a match is in the [migration map](2026-10-16-lolesports-api-migration-map.md#how-triforce-decides-a-match-ended). The design that follows from all this is in [live match state design](2026-10-16-live-match-state-design.md).

## Still to capture

With `recorder.py` (it fetches windows one after the other now, the first version skipped one 10 s window every ~3.5 min):

- a pause (`paused` state, gold stops)
- a Bo3 and a Bo1
- a series that goes the distance
- a technical problem if possible (remake, crash, chronobreak)
- a tier 2/3 match day, for P1-P3

Without live data:

- a historical check of tier 2/3 leagues with `getSchedule` paging + `getEventDetails`, looking for the patterns above
- the remaining old endpoints (`getGames`, `getCompletedEvents`…), one call each

HARs have cookies and an approximate location (`x-rg-esports-geolocation`), don't commit them. `recorder.py` output has no personal data.
