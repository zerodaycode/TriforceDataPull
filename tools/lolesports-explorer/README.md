# lolesports-explorer

Small tools we used to study the LoL Esports APIs (old API, live feed and the new lolesports.com GraphQL API). They don't touch Triforce. What we found is in [`docs/investigations/`](../../docs/investigations/).

Python 3.8+, standard library only.

| Tool | Use |
|---|---|
| `gql_tool.py` | `extract` / `list` / `show` / `call` the new site GraphQL operations (read only), `opencollection` exports a Bruno collection |
| `recorder.py` | `python3 recorder.py --league worlds`: record the old API + live feed during matches, without abusing them |
| `summarize.py` | `python3 summarize.py recordings/<folder>`: turn a recording into a short timeline (`timeline.md`) |
| `har_explorer.py` | `python3 har_explorer.py <hars>` (writes to `har_report/`, git-ignored): list endpoints, decode page payloads and infer types from browser HARs |

| File | What |
|---|---|
| `manifest_acab248.json` | The site persisted query manifest (164 operations with hash and query text), build `acab248` |
| `lolesports_acab248.opencollection.yml` | The 124 queries as a Bruno collection (OpenCollection), import it and pick the environment `lolesports.com` |

## Examples

```bash
python3 gql_tool.py list manifest_acab248.json 'event|team|league'
python3 gql_tool.py show manifest_acab248.json scheduleEvents
python3 gql_tool.py call manifest_acab248.json scheduleEvents '{"hl":"en-US","sport":["lol"],"pageSize":5}'
python3 gql_tool.py call manifest_acab248.json scheduleEvents '{"hl":"en-US"}' --dry-run   # only print the URL
```

## When the hashes stop working

Riot rebuilds the site and the hashes change. Record a HAR of lolesports.com with the Network filter on "All", then:

```bash
python3 gql_tool.py extract new.har -o manifest.json
python3 gql_tool.py opencollection manifest.json -o lolesports.opencollection.yml
```

Other ways to find the manifest are in [the GraphQL investigation](../../docs/investigations/2026-10-16-lolesports-graphql-api.md#where-the-operations-come-from).

## Bruno

- Each operation is a plain HTTP GET with `operationName` / `variables` / `extensions` params. Bruno's GraphQL request type can't be used, it sends the query text and the server refuses it.
- The headers are set once at collection level.
- The Docs tab of each request has its hash and full query text. Change the `variables` param to change the arguments.
- Operations we didn't test have template variables with `<id>` placeholders.
- No mutations (`--include-mutations` to add them).
- Settings → Encode URL is on for every request. Without it Bruno sends the JSON unencoded and the server answers `400 INVALID_GRAPHQL_REQUEST` "There was no GraphQL operation to execute".

## Don't commit

- HARs: they have your cookies and approximate location.
- `recordings/` and `har_report/` (tool output). Recordings have no personal data, but they are big. `har_report/` comes from your HARs: the `authorization` header is redacted, but POST bodies (300 chars) are copied, so don't share it if you recorded logged in.

Both are in `.gitignore`.

`recorder.py` uses the public web client key, set `LOLESPORTS_API_KEY` if Riot changes it.
