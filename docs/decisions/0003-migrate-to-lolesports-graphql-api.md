# 0003 - Migrate to the lolesports.com GraphQL API

- Status: Proposed
- Date: 2026-10-16

## Context
Triforce reads everything from the old API (`esports-api.lolesports.com/persisted/gw`). It still works, but the new lolesports.com doesn't use it anymore, so Riot can turn it off without notice. The new site gets its data from a GraphQL API (`lolesports.com/api/gql`).

What we know about the new API ([investigation](../investigations/2026-10-16-lolesports-graphql-api.md), [migration map](../investigations/2026-10-16-lolesports-api-migration-map.md)):

- Every call we make has an equivalent, all called from a script with real data. `watchLiveQuery` was checked during a live match (FUR vs C9, 2026-10-16): same data as the old `getLive` ([live check](../investigations/2026-10-17-old-vs-new-api-comparison.md)).
- The ids are the same, so the rows we have keep their ids: `ext_id` for leagues, tournaments, teams and players, `match_id` for the schedule. Shows have no stored id (matched by `start_time` + league).
- Some calls get better: one call for all leagues with their tournaments (today 51 calls), team and game ids in the schedule, `homeLeague` with its id (no more lookups by name or slug).
- It only runs persisted queries (sha256 hashes of the site queries). The hashes change when Riot rebuilds the site.
- From a script it needs the Apollo client headers and a lolesports.com `Referer`.
- Live data is cached 120 s + 60 s stale (old `getLive`: 60 s + 60 s stale). Measured at the end of one game: new API +4 min 56 s, old +3 min 48 s. In the game before, `watchLiveQuery` and `scheduleEvents` disagreed with the same answer time. Only the feed is real time.
- Our DB (filled from the old API) and the new API have the same leagues, tournaments, teams, players and team-player links, row by row ([comparison](../investigations/2026-10-17-old-vs-new-api-comparison.md#data-comparison-our-db-vs-the-new-api)).
- The new schedule has team ids. Today we find schedule teams by name and 24 matches are linked to the wrong team (49 names are shared by 104 teams).
- No live in-game stats. The old feed `feed.lolesports.com/livestats/v1` is the only source and the new site doesn't use it either.

## Decision (proposed)
Move the reads to the GraphQL API, one data set at a time, keeping the old API as fallback until each part is checked against it:

1. Leagues and tournaments: `GetOtherLeagues`.
2. Teams and players: `GetLoLTeamOverview` without `teamId`.
3. Schedule: `scheduleEvents`.
4. Live and event details: `watchLiveQuery` + `watchVodQuery` (checked live on 2026-10-16). The adapter sets the event `state` itself.

Steps 1 and 2 are the default since 2026-11-01 (`TRIFORCE_SOURCE_LEAGUES` / `TRIFORCE_SOURCE_TEAMS` = `new`). `old` stays as fallback until this ADR is accepted.

The query hashes are kept in a manifest file next to Triforce (not in the repo). Triforce downloads it again from the site when a hash is unknown, and logs `GQL_MANIFEST:` when that fails or when a query we use changed. If we need live in-game stats later, they come from the old feed.

The adapters keep the time each answer was made (Netlify: `Date - Age`) per operation and match, in memory in the live loop, and ignore answers older than the newest one. The old API served an older copy after a newer one during the live check, so this applies to both APIs. Two operations can disagree with the same answer time, so state and score never go back whatever the operation. Live wins are copied by team id, not by position in the array.

Triforce sends the site headers only (no custom `User-Agent`, the API doesn't need it), and never polls an operation faster than its `cache-control: max-age` (60-120 s).

## Consequences
- Good: we stop depending on an API the site doesn't use, fewer calls, better ids (no lookups by name or slug). `teamId` fixes the 24 schedule matches linked to the wrong team.
- Bad: hashes can break with any site build, we need a refresh process and monitoring.
- Bad: we depend on headers that make the request look like the site (`Referer`). It's what the site does, but Riot can change it.
- Bad: the new API caches live data longer (120 s + 60 s stale vs 60 s + 60 s). In the one game we measured both confirmed the end 3.5-5 min late (old +3:48, new +4:56).
- Bad: saving the answer time (history of P3 corrections, rule 8 of the live design) needs a new column or table: a Canyon migration and a rebuild.
- Neutral: the new API sends tournament datetimes. Our dates are their UTC date in all 513, so the adapter takes the UTC date and the `date` columns stay.
- Bad: no `alternativeImage`, 1,325 teams have `alt_image_url` today.

## Open before accepting
- [x] `watchLiveQuery` during a live match: same data as `getLive`, no `state`, cache 120 s + 60 s stale, game 3 end seen +3 min 48 s (old) / +4 min 56 s (new) after the feed, one game (2026-10-16, [live check](../investigations/2026-10-17-old-vs-new-api-comparison.md)). Not seen: the variables of the site live page (no HAR).
- [x] How to refresh the hashes: Triforce keeps them in a file (`TRIFORCE_GQL_MANIFEST`, default `gql-manifest.json`) and downloads the manifest again from the site in Rust when a hash is unknown (once an hour at most) or with `refresh-manifest`. The Python tool stays to check things by hand (2026-10-17).
- [x] Rust spike: `GetOtherLeagues` from Triforce, behind `TRIFORCE_SOURCE_LEAGUES=new`, gives the same leagues and tournaments we store: 51 leagues and 514 tournaments, same md5 on every column. `refresh-manifest` found build `a93ba50` with 164 operations, and a made-up hash triggered one refresh and the run went on (2026-10-17).
- [x] Rust spike: `GetLoLTeamOverview` from Triforce, behind `TRIFORCE_SOURCE_TEAMS=new`, wrote the same 1,586 teams and 4,526 players as the old API (same md5 on every column). It kept all 1,325 stored alternative images, and `team_player` stayed at 5,431 rows with no duplicates (2026-10-17).
- [ ] Tier 2/3 behavior (P1-P3) in the new API.
- [x] Tournament dates: our `start_date`/`end_date` are the UTC date of `startTime`/`endTime` in all 513 (2026-10-16). Take the UTC date, no column change.
- [ ] `team.alt_image_url`: there is no `alternativeImage` in the new API and `update()` writes the whole row. 1,325 teams have one. Keep the stored value or drop the column.
  Until we find another source, Triforce keeps the stored value when it reads teams from the new API (2026-10-17). This is the default since 2026-11-01.
- [ ] TBD slots: the new schedule sends `teamId: "0"` there, today we link a "TBD" team. Decide: NULL (and remove the TBD rows) or keep a placeholder. Removing them means: the teams sync skips them (`GetLoLTeamOverview` returns 3 TBD teams), both schedule team columns set to NULL first, and `team_right_id` gets the foreign key `team_left_id` already has.
- [ ] Shows: check that `EventShow.startTime` gives the same values we match on today, or add an `ext_id` to `schedule`.

This ADR stays "Proposed" and we update it while we close these points. When it's accepted it doesn't change anymore, a later change of plan is a new ADR.

## Evidence
- [Old vs new API: live check and data comparison](../investigations/2026-10-17-old-vs-new-api-comparison.md)
- [lolesports.com GraphQL API](../investigations/2026-10-16-lolesports-graphql-api.md)
- [Migration map](../investigations/2026-10-16-lolesports-api-migration-map.md)
- [Old API and live feed](../investigations/2026-10-16-lolesports-old-api-and-live-feed.md)
- Checked from Triforce's environment on 2026-10-16: `homeLeagues` and `GetLoLTeamOverview` answer HTTP 200, and `GetLoLTeamOverview` returns the same 1,586 teams / 4,526 players / 5,431 team-player links that the old API gave us that day.
