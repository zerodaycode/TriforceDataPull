#!/usr/bin/env python3
"""
gql_tool.py - use the lolesports.com GraphQL persisted queries.

The server (https://lolesports.com/api/gql) doesn't run free queries
("PERSISTED_QUERY_ID_REQUIRED"). It only runs the operations it knows, by the
sha256 of the query text. The site has the full list in a JS bundle
("apollo-persisted-query-manifest"). This tool extracts it, and you can look at
the operations or call them by name.

    python3 gql_tool.py extract  <har files / js files / folders> -o manifest.json
    python3 gql_tool.py list     manifest.json [filter-regex]
    python3 gql_tool.py show     manifest.json <operationName>
    python3 gql_tool.py call     manifest.json <operationName> '<variables json>' [--dry-run] [-o out.json]

Examples:
    python3 gql_tool.py call manifest.json scheduleEvents '{"hl":"en-US","sport":["lol"],"pageSize":5}'
    python3 gql_tool.py call manifest.json watchLiveQuery '{"hl":"en-US","sport":["lol"],"eventState":["inProgress"],"eventType":"all"}'

When Riot changes a query (new site build) the hashes change. If a call starts failing
with PersistedQueryNotFound, run "extract" again on a new HAR (Network filter "All").
Standard library only, Python 3.8+.
"""
import argparse
import glob
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

ENDPOINT = "https://lolesports.com/api/gql"
MARKER = '"format":"apollo-persisted-query-manifest"'


def js_unescape(lit):
    def rep(m):
        s = m.group(0)
        c = s[1]
        if c == "u":
            return chr(int(s[2:6], 16))
        if c == "x":
            return chr(int(s[2:4], 16))
        return {"n": "\n", "t": "\t", "r": "\r", "b": "\b", "f": "\f", "0": "\0"}.get(c, c)
    return re.sub(r"\\(u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)", rep, lit)


def manifest_from_js(src):
    """Find JSON.parse('{"format":"apollo-persisted-query-manifest",...}') in a bundle."""
    i = src.find(MARKER)
    if i < 0:
        return None
    start = src.rfind("JSON.parse('", 0, i)
    if start < 0:                       # manifest inlined as plain JSON
        start = src.rfind("{", 0, i)
        depth, j = 0, start
        while j < len(src):
            depth += {"{": 1, "}": -1}.get(src[j], 0)
            if depth == 0:
                return json.loads(src[start:j + 1])
            j += 1
        return None
    start += len("JSON.parse('")
    end = start
    while True:                         # end of the '...' string, escaped quotes don't count
        end = src.find("'", end)
        if end < 0:
            return None
        if src[end - 1] != "\\":
            break
        end += 1
    return json.loads(js_unescape(src[start:end]))


def iter_sources(paths):
    files = []
    for p in paths:
        files += sorted(glob.glob(os.path.join(p, "*"))) if os.path.isdir(p) else [p]
    for f in files:
        if f.endswith(".har"):
            with open(f, encoding="utf-8") as fh:
                har = json.load(fh)
            for e in har["log"]["entries"]:
                c = e["response"].get("content", {})
                if "javascript" in (c.get("mimeType") or "") and c.get("text"):
                    t = c["text"]
                    if c.get("encoding") == "base64":
                        import base64
                        t = base64.b64decode(t).decode("utf-8", "replace")
                    yield e["request"]["url"], t, har
        elif os.path.isfile(f):
            with open(f, encoding="utf-8", errors="replace") as fh:
                yield f, fh.read(), None


def client_version(har):
    for e in (har or {}).get("log", {}).get("entries", []):
        for h in e["request"]["headers"]:
            if h["name"].lower() == "apollographql-client-version":
                return h["value"]
    return None


def cmd_extract(args):
    found, version = None, None
    for src_name, text, har in iter_sources(args.paths):
        version = version or client_version(har)
        if MARKER in text:
            found = manifest_from_js(text)
            if found:
                print("manifest found in %s" % src_name)
                break
    if not found:
        sys.exit("No persisted-query manifest found. Record a HAR with the Network filter on 'All' (JS included).")
    bad = [o["name"] for o in found["operations"] if hashlib.sha256(o["body"].encode()).hexdigest() != o["id"]]
    found["_meta"] = {"client_version": version, "operations": len(found["operations"]),
                      "id_is_sha256_of_body": not bad, "mismatches": bad}
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(found, fh, indent=1, ensure_ascii=False)
    kinds = {}
    for o in found["operations"]:
        kinds[o["type"]] = kinds.get(o["type"], 0) + 1
    print("wrote %s: %s operations %s, client version %s, id == sha256(body): %s" % (
        args.out, len(found["operations"]), kinds, version, not bad))


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def find_op(m, name):
    for o in m["operations"]:
        if o["name"] == name:
            return o
    sys.exit("operation %r not in manifest (try: list)" % name)


def cmd_list(args):
    rx = re.compile(args.filter or ".", re.I)
    for o in sorted(load(args.manifest)["operations"], key=lambda o: (o["type"], o["name"].lower())):
        if rx.search(o["name"]):
            head = o["body"].split("{", 1)[0].strip()
            print("%-9s %-48s %s" % (o["type"], o["name"], head[len(o["type"]) + len(o["name"]) + 1:][:120]))


def cmd_show(args):
    o = find_op(load(args.manifest), args.name)
    print("# %s %s\n# id %s\n\n%s" % (o["type"], o["name"], o["id"], o["body"]))


def cmd_call(args):
    o = find_op(load(args.manifest), args.name)
    if o["type"] != "query":
        sys.exit("refusing to send a %s: this tool only reads data" % o["type"])
    variables = json.loads(args.variables or "{}")
    qs = urllib.parse.urlencode({
        "operationName": o["name"],
        "variables": json.dumps(variables, separators=(",", ":")),
        "extensions": json.dumps({"persistedQuery": {"version": 1, "sha256Hash": o["id"]}}, separators=(",", ":")),
    })
    url = "%s?%s" % (ENDPOINT, qs)
    # Without the Apollo client headers the server answers 401 "No client headers set".
    # The version is the site build of the manifest (saved by "extract").
    version = args.client_version or (load(args.manifest).get("_meta") or {}).get("client_version")
    headers = {"content-type": "application/json",
               "accept": "application/graphql-response+json,application/json;q=0.9",
               "apollographql-client-name": "Esports Web",
               "user-agent": "lol-hobby-research/1.0 (personal, non-commercial)",
               # Test 2 (2026-10-16): it also needs a lolesports.com Referer or Origin, one of them
               # is enough (any lolesports.com page works). Without them: 401 "No client headers set".
               "referer": "https://lolesports.com/"}
    if version:
        headers["apollographql-client-version"] = version
    for h in args.header or []:
        k, _, v = h.partition(":")
        headers[k.strip()] = v.strip()
    if args.dry_run:
        print(url)
        print(json.dumps(headers, indent=1))
        return
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            status, body, hdrs = r.status, r.read().decode("utf-8", "replace"), dict(r.headers)
    except urllib.error.HTTPError as e:
        status, body, hdrs = e.code, e.read().decode("utf-8", "replace"), dict(e.headers or {})
    except (urllib.error.URLError, OSError) as e:
        sys.exit("network error: %s" % e)
    print("HTTP %s  cache-control: %s  age: %s" % (status, hdrs.get("Cache-Control"), hdrs.get("Age")), file=sys.stderr)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(body)
        print("saved %s (%d bytes)" % (args.out, len(body)), file=sys.stderr)
    else:
        try:
            print(json.dumps(json.loads(body), indent=1, ensure_ascii=False)[:args.max_chars])
        except ValueError:
            print(body[:args.max_chars])


# --- opencollection ---
# Bruno / OpenCollection export. The server refuses the query text (PERSISTED_QUERY_ID_REQUIRED),
# so each operation is a plain HTTP GET with operationName / variables / extensions as
# query params, same as the website (Bruno's GraphQL request type sends the text).

OC_FOLDERS = [  # (folder name, regex for the operation name), first match wins
    ("LoL - Leagues and seasons", r"^(getLeagues|homeLeagues|SettingsLeagues|GetOtherLeagues|GetSeasonForNavigation|Season|GetGprSeasons|"
                                   r"GetSeasonTournaments|GetTournamentsForSchedule|GetSeasonLeagues|GetSplitLeague|GetSeason|GetSeasonTournament)$"),
    ("LoL - Schedule and live", r"^(scheduleEvents|homeEvents|watchLiveQuery|watchLiveLeaguesQuery|watchSingleLiveEventByLeagueQuery|"
                                 r"GetSeasonTournamentEvents|GetTeamEvents|GetLoLTeamEvents)$"),
    ("LoL - Match details and VODs", r"^(watchVodQuery|GetWatchStats)$"),
    ("LoL - Brackets and standings", r"^(GetSeasonStage|GetSeasonTournamentDisplay)$"),
    ("LoL - Teams and players", r"^(GetLoLTeamOverview|GetTeams|SettingsTeams|GetSuperlativeTeams)$"),
    ("LoL - Rankings and points", r"^(GetTeamGpr|GetLeagueElo|GetSeasonPoints|GetSeasonForChampionshipPoints)$"),
    ("Other - Pickem, Crystal Ball, factions", r"(?i)pickem|crystalball|faction|^GetPlayer|^PlayerRegistered"),
    ("Other - VALORANT", r"(?i)^(get)?val|^ValHome"),
    ("Other - TFT and Tacticians", r"(?i)tft|tactician|RegionalFinals"),
    ("Other - Compete and organizers", r"(?i)compete|organizer|^GetRb|^PlayerTournaments|^GetOrgSign"),
    ("Other - Content, drops, sponsors", r"(?i)article|news|widgets|sponsor|distributor|drop"),
    ("Other - Account and misc", r"."),
]

# Variables tested in tests 1-3 (docs/investigations/2026-10-16-lolesports-graphql-api.md),
# the other operations get a template.
OC_EXAMPLES = {
    "getLeagues": {"hl": "{{hl}}", "sport": ["lol"]},
    "homeLeagues": {"hl": "{{hl}}", "sport": ["lol"], "flags": ["excludeHidden", "excludeWithoutTournaments"]},
    "GetOtherLeagues": {"locale": "{{hl}}", "sport": "lol"},
    "GetSeasonForNavigation": {"hl": "{{hl}}", "seasonId": "115547545029543948"},
    "GetGprSeasons": {"hl": "{{hl}}", "sport": "lol"},
    "scheduleEvents": {"hl": "{{hl}}", "sport": ["lol"], "pageSize": 20},
    "homeEvents": {"hl": "{{hl}}", "sport": ["lol"], "eventState": ["inProgress"], "eventType": "all", "pageSize": 50},
    "watchLiveQuery": {"hl": "{{hl}}", "sport": ["lol"], "eventState": ["inProgress"], "eventType": "all"},
    "watchVodQuery": {"hl": "{{hl}}", "eventId": "115660588583040710"},
    "GetLoLTeamOverview": {"hl": "{{hl}}", "sport": ["lol"], "teamId": ["107251245690956393"]},
    "GetTeamGpr": {"hl": "{{hl}}", "step": 10, "startDate": "2026-01-01", "endDate": "2026-10-16", "sport": "lol"},
    "GetSeasonTournamentDisplay": {"hl": "{{hl}}", "tournamentId": ["115660540725177488"]},
    "GetSeasonTournament": {"hl": "{{hl}}", "tournamentId": ["115660540725177488"]},
    "GetSeasonStage": {"hl": "{{hl}}", "stageIds": ["115660540725570705"]},
    "GetSeasonTournamentEvents": {"hl": "{{hl}}", "sport": ["lol"], "tournamentId": ["115660540725177488"]},
    "GetTeamEvents": {"hl": "{{hl}}", "sport": ["lol"], "teamId": ["107251245690956393"], "pageSize": 20},
    "GetLoLTeamEvents": {"hl": "{{hl}}", "sport": ["lol"], "teamId": ["107251245690956393"], "pageSize": 20},
    "Season": {"hl": "{{hl}}", "seasonIds": ["115547545029543948"]},
}


def oc_variable_template(op):
    """Example variables from the operation signature: hl and sport filled, the other required ones as placeholders."""
    head = op["body"].split("{", 1)[0]
    out = {}
    for name, typ in re.findall(r"\$(\w+)\s*:\s*([\[\]\w!]+)", head):
        base = typ.strip("[]!")
        is_list, required = typ.startswith("["), typ.endswith("!")
        if base == "Locale":
            out[name] = "{{hl}}"
        elif base == "Sport":
            out[name] = ["lol"] if is_list else "lol"
        elif required:
            sample = {"ID": "<id>", "String": "", "Int": 10, "Boolean": False, "Date": "2026-01-01T00:00:00Z"}.get(base, "<%s>" % base)
            out[name] = [sample] if is_list else sample
    return out


def yaml_scalar(v):
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    return json.dumps(v, ensure_ascii=False)          # a JSON string is also a valid YAML double-quoted string


def to_yaml(v, indent=0):
    """Small YAML writer: dicts, lists and scalars, multi-line strings as literal blocks."""
    pad = "  " * indent
    lines = []
    if isinstance(v, dict):
        for k, x in v.items():
            if isinstance(x, (dict, list)) and x:
                lines.append("%s%s:" % (pad, k))
                lines.append(to_yaml(x, indent + 1))
            elif isinstance(x, str) and "\n" in x:
                lines.append("%s%s: |-" % (pad, k))
                lines.extend(("%s  %s" % (pad, ln)).rstrip() for ln in x.split("\n"))
            else:
                lines.append("%s%s: %s" % (pad, k, yaml_scalar(x) if not isinstance(x, (dict, list)) else ("{}" if isinstance(x, dict) else "[]")))
    elif isinstance(v, list):
        for x in v:
            if isinstance(x, (dict, list)) and x:
                sub = to_yaml(x, indent + 1).split("\n")
                lines.append("%s- %s" % (pad, sub[0].lstrip()))
                lines.extend(sub[1:])
            else:
                lines.append("%s- %s" % (pad, yaml_scalar(x)))
    return "\n".join(lines)


def cmd_opencollection(args):
    m = load(args.manifest)
    version = args.client_version or (m.get("_meta") or {}).get("client_version") or "acab248"
    ops = [o for o in m["operations"] if o["type"] == "query" or args.include_mutations]
    folders = {name: [] for name, _ in OC_FOLDERS}
    for o in sorted(ops, key=lambda o: o["name"].lower()):
        for name, rx in OC_FOLDERS:
            if re.search(rx, o["name"]):
                folders[name].append(o)
                break
    items = []
    for fseq, (fname, _) in enumerate(OC_FOLDERS, 1):
        reqs = []
        for seq, o in enumerate(folders[fname], 1):
            variables = OC_EXAMPLES.get(o["name"]) or oc_variable_template(o)
            tested = o["name"] in OC_EXAMPLES
            signature = o["body"].split("{", 1)[0].strip()
            docs = ("**%s** `%s` (persisted query, build `%s`)\n\nid / sha256: `%s`\n\n%s\n\n"
                    "Change the `variables` param (JSON) to change the arguments. The hash in `extensions` is the sha256 of the query below, "
                    "if the site is rebuilt generate the collection again (`gql_tool.py extract` + `opencollection`).\n\n"
                    "```graphql\n%s\n```") % (o["type"], o["name"], version, o["id"],
                                              "Variables tested from a script (tests 1-3)." if tested else
                                              "Variables are a template from the signature, change the `<...>` placeholders.",
                                              o["body"])
            reqs.append({
                "info": {"name": o["name"], "type": "http", "seq": seq},
                "http": {
                    "method": "GET",
                    "url": "{{baseUrl}}",
                    "params": [
                        {"name": "operationName", "value": o["name"], "type": "query"},
                        {"name": "variables", "value": json.dumps(variables, separators=(",", ":")), "type": "query",
                         "description": signature},
                        {"name": "extensions", "value": json.dumps(
                            {"persistedQuery": {"version": 1, "sha256Hash": o["id"]}}, separators=(",", ":")), "type": "query"},
                    ],
                    "auth": "inherit",
                },
                # Bruno sends params raw unless encodeUrl is on, then the server can't parse the JSON
                # in `extensions` and answers 400 "There was no GraphQL operation to execute".
                "settings": {"encodeUrl": True},
                "docs": docs,
            })
        if reqs:
            items.append({"info": {"name": fname, "type": "folder", "seq": fseq}, "items": reqs})
    collection = {
        "opencollection": "1.0.0",
        "info": {"name": "lolesports.com GraphQL (%s)" % version,
                 "summary": "lolesports.com persisted queries as plain GET requests.",
                 "version": version},
        "config": {"environments": [{
            "name": "lolesports.com",
            "variables": [
                {"name": "baseUrl", "value": ENDPOINT},
                {"name": "hl", "value": "en-US"},
                {"name": "clientVersion", "value": version},
            ]}]},
        "request": {"headers": [
            {"name": "apollographql-client-name", "value": "Esports Web"},
            {"name": "apollographql-client-version", "value": "{{clientVersion}}"},
            {"name": "Referer", "value": "https://lolesports.com/"},
            {"name": "content-type", "value": "application/json"},
            {"name": "accept", "value": "application/graphql-response+json,application/json;q=0.9"},
        ]},
        "docs": ("Exported with gql_tool.py from the site manifest (build %s).\n\n"
                 "- The server only runs the operations it knows, by sha256 hash. Free GraphQL queries are refused (PERSISTED_QUERY_ID_REQUIRED).\n"
                 "- It needs the apollographql-* headers and a lolesports.com Referer/Origin, otherwise 401 \"No client headers set\".\n"
                 "- Use the environment \"lolesports.com\", change `hl` there for other languages.\n"
                 "- The CDN caches the answers 60-120 s, plus stale-while-revalidate.\n"
                 "- %s") % (version, "Mutations are included, they need a logged-in Riot account and they change data." if args.include_mutations
                            else "Mutations are not included, they need a logged-in Riot account and they change data."),
        "bundled": True,
        "items": items,
    }
    text = "# OpenCollection (Bruno) - generated by gql_tool.py\n" + to_yaml(collection) + "\n"
    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(text)
    print("wrote %s: %d requests in %d folders (build %s)" % (
        args.out, sum(len(f["items"]) for f in items), len(items), version))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("extract")
    p.add_argument("paths", nargs="+")
    p.add_argument("-o", "--out", default="manifest.json")
    p.set_defaults(fn=cmd_extract)
    p = sub.add_parser("list")
    p.add_argument("manifest")
    p.add_argument("filter", nargs="?")
    p.set_defaults(fn=cmd_list)
    p = sub.add_parser("show")
    p.add_argument("manifest")
    p.add_argument("name")
    p.set_defaults(fn=cmd_show)
    p = sub.add_parser("call")
    p.add_argument("manifest")
    p.add_argument("name")
    p.add_argument("variables", nargs="?")
    p.add_argument("--dry-run", action="store_true", help="print the URL instead of calling")
    p.add_argument("-o", "--out", help="save the raw response")
    p.add_argument("--max-chars", type=int, default=4000)
    p.add_argument("--client-version", help="apollographql-client-version to send (default: from the manifest)")
    p.add_argument("-H", "--header", action="append", help="extra header 'Name: value' (repeatable)")
    p.set_defaults(fn=cmd_call)
    p = sub.add_parser("opencollection", help="export the manifest as an OpenCollection YAML (Bruno import)")
    p.add_argument("manifest")
    p.add_argument("-o", "--out", default="lolesports.opencollection.yml")
    p.add_argument("--client-version", help="build to put in the environment (default: from the manifest)")
    p.add_argument("--include-mutations", action="store_true", help="also export mutations (need a Riot login)")
    p.set_defaults(fn=cmd_opencollection)
    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
