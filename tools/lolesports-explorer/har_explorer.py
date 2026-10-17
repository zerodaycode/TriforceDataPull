#!/usr/bin/env python3
"""
har_explorer.py - map the lolesports.com web API from browser HAR recordings.

Usage:
    python3 har_explorer.py <har files or folders...> [-o OUTPUT_DIR]

What it does (standard library only, Python 3.8+):
  * Lists every endpoint the site called, grouped by pattern (numeric IDs -> {id}),
    with methods, status codes, content types, useful headers, cache policy
    and GraphQL operation names / persisted-query hashes / variables.
  * Decodes every response body it knows:
        - JSON (GraphQL)                         application/json
        - React Server Component payloads        text/x-component
        - Full HTML page loads (self.__next_f)   text/html
        - JS bundles: greps for GraphQL operations, sha256 hashes and API hosts
  * Builds a schema from every object with a "__typename"
    (field types, nullability, example values and enum-like values).
  * Extracts every Match it finds, merges the copies by id, and checks them
    (winner without more game wins, unstarted after start time,
    in progress for too long, missing start times...).

Outputs (in OUTPUT_DIR, default ./har_report):
    report.md      summary to read
    schema.json    inferred types
    matches.csv    one row per match (latest/fullest copy)
    bodies/        every decoded response body, to grep by hand
"""
import argparse
import csv
import glob
import json
import os
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from urllib.parse import parse_qs, unquote, urlparse

ID_RE = re.compile(r"\d{6,}")
INTERESTING_REQ_HEADERS = {
    "rsc", "next-router-prefetch", "next-router-state-tree", "next-url", "next-action",
    "x-api-key", "apollographql-client-name", "apollographql-client-version",
    "authorization", "content-type", "accept",
}
INTERESTING_RESP_HEADERS = {"cache-control", "cache-status", "age", "content-type", "x-middleware-rewrite"}
NOISE_HOSTS = ("featureassets.org", "statsigapi.net", "cloudflare-dns.com", "datadoghq", "google", "doubleclick")
ENUM_SKIP = {"id", "name", "slug", "image", "darkImage", "description", "code", "teamId",
             "structuralId", "startTime", "endTime", "url", "title", "subtitle"}


# --- rsc ---
def parse_rsc(text):
    """Parse a React Server Components flight stream into {row_id: (kind, value)}."""
    data = text.encode("utf-8")
    i, rows = 0, {}
    while i < len(data):
        m = re.match(rb"([0-9a-f]+):", data[i:i + 24])
        if not m:
            j = data.find(b"\n", i)
            i = len(data) if j < 0 else j + 1
            continue
        rid = m.group(1).decode()
        i += m.end()
        if data[i:i + 1] == b"T":  # text row: T<hexlen>,<bytes>
            comma = data.find(b",", i)
            try:
                n = int(data[i + 1:comma], 16)
            except ValueError:
                n = 0
            rows[rid] = ("T", data[comma + 1:comma + 1 + n].decode("utf-8", "replace"))
            i = comma + 1 + n
            continue
        j = data.find(b"\n", i)
        j = len(data) if j < 0 else j
        line = data[i:j].decode("utf-8", "replace")
        i = j + 1
        if line[:1] == "I":
            rows[rid] = ("I", line[1:])
        elif line[:2] == "HL":
            rows[rid] = ("HL", line[2:])
        else:
            try:
                rows[rid] = ("J", json.loads(line))
            except ValueError:
                rows[rid] = ("RAW", line)
    return rows


def rsc_from_html(html):
    """Rebuild the flight stream embedded in a Next.js HTML page."""
    chunks = []
    for m in re.finditer(r"self\.__next_f\.push\(\[1,(\"(?:[^\"\\]|\\.)*\")\]\)", html):
        try:
            chunks.append(json.loads(m.group(1)))
        except ValueError:
            pass
    return "".join(chunks)


def rsc_components(rows):
    """Map '$L<id>' -> 'ExportName#moduleId' from import rows."""
    out = {}
    for rid, (kind, val) in rows.items():
        if kind == "I":
            try:
                arr = json.loads(val)
                out["$L" + rid] = "%s#%s" % (arr[2] or "default", arr[0])
            except (ValueError, IndexError, TypeError):
                pass
    return out


# --- inference ---
class Collector:
    def __init__(self):
        self.types = defaultdict(lambda: defaultdict(lambda: {"types": Counter(), "examples": Counter(), "n": 0}))
        self.type_pages = defaultdict(set)
        self.matches = defaultdict(list)          # id -> [(page, captured_at, obj)]
        self.component_props = defaultdict(set)   # page pattern -> {"<Comp> {prop: Type}"}
        self.js_findings = defaultdict(set)

    @staticmethod
    def kind(v):
        if v is None or v == "$undefined":
            return "null"
        if isinstance(v, bool):
            return "bool"
        if isinstance(v, int):
            return "int"
        if isinstance(v, float):
            return "float"
        if isinstance(v, str):
            return "ref" if v.startswith("$") and ":" in v else "str"
        if isinstance(v, list):
            inner = sorted({Collector.kind(x) for x in v})
            return "list[%s]" % "|".join(inner) if inner else "list[]"
        if isinstance(v, dict):
            return v.get("__typename", "object")
        return type(v).__name__

    def walk(self, v, page, captured_at):
        if isinstance(v, dict):
            tn = v.get("__typename")
            if tn:
                self.type_pages[tn].add(page)
                T = self.types[tn]
                for k, x in v.items():
                    if k == "__typename":
                        continue
                    F = T[k]
                    F["n"] += 1
                    F["types"][self.kind(x)] += 1
                    if isinstance(x, (str, int, float, bool)) and x != "$undefined" and not (isinstance(x, str) and x.startswith("$")):
                        F["examples"][x] += 1
                    elif isinstance(x, list):
                        for y in x:
                            if isinstance(y, (str, int)):
                                F["examples"][y] += 1
                if tn == "Match" and "id" in v:
                    self.matches[v["id"]].append((page, captured_at, v))
            for x in v.values():
                self.walk(x, page, captured_at)
        elif isinstance(v, list):
            for x in v:
                self.walk(x, page, captured_at)

    def walk_components(self, rows, page):
        comps = rsc_components(rows)

        def has_tn(v, depth=0):
            if depth > 6:
                return False
            if isinstance(v, dict):
                return "__typename" in v or any(has_tn(x, depth + 1) for x in v.values())
            if isinstance(v, list):
                return any(has_tn(x, depth + 1) for x in v[:5])
            return False

        def shape(v):
            if isinstance(v, dict) and "__typename" in v:
                return v["__typename"]
            if isinstance(v, list):
                return "[%s]" % (shape(v[0]) if v else "")
            if isinstance(v, dict):
                keys = list(v.items())
                if keys and all(k.startswith("match_") for k, _ in keys):
                    return "{structuralId: %s}" % shape(keys[0][1])
                return "{" + ", ".join("%s: %s" % (k, shape(x)) for k, x in keys[:6]) + "}"
            return "str" if isinstance(v, str) else type(v).__name__

        def walk(v):
            if isinstance(v, list) and len(v) == 4 and v[0] == "$" and isinstance(v[3], dict):
                comp = v[1]
                if isinstance(comp, str) and comp.startswith("$L"):
                    props = {k: shape(x) for k, x in v[3].items() if has_tn(x)}
                    if props:
                        self.component_props[page].add("%s  %s" % (comps.get(comp, comp), json.dumps(props)))
                for x in v[3].values():
                    walk(x)
            elif isinstance(v, list):
                for x in v:
                    walk(x)
            elif isinstance(v, dict):
                for x in v.values():
                    walk(x)

        for _, (kind, val) in rows.items():
            if kind == "J":
                walk(val)

    def scan_js(self, text, url):
        for m in re.finditer(r"\b(query|mutation|subscription)\s+([A-Z][A-Za-z0-9_]+)\s*[({]", text):
            self.js_findings["graphql operation"].add("%s %s" % (m.group(1), m.group(2)))
        for m in re.finditer(r"operationName[\"']?\s*[:=]\s*[\"']([A-Za-z0-9_]+)", text):
            self.js_findings["operationName literal"].add(m.group(1))
        for m in re.finditer(r"[\"']([A-Z][A-Za-z0-9_]+)[\"']\s*:\s*[\"']([0-9a-f]{64})[\"']", text):
            self.js_findings["persisted query hash"].add("%s = %s" % (m.group(1), m.group(2)))
        for m in re.finditer(r"https?://[a-z0-9.-]*(?:lolesports|riotgames|rgpub|leagueoflegends)[a-z0-9./_-]*", text):
            self.js_findings["url in js"].add(m.group(0))
        for w in ("persisted/gw", "livestats", "getLive", "getSchedule", "getEventDetails", "window/", "details/", "x-api-key"):
            if w in text:
                self.js_findings["keyword in js"].add("%s  (in %s)" % (w, urlparse(url).path[-60:]))


# --- endpoints ---
def endpoint_key(url):
    u = urlparse(url)
    path = ID_RE.sub("{id}", u.path)
    path = re.sub(r"^/[a-z]{2}-[A-Z]{2}(?=/|$)", "/{locale}", path)
    key = "%s%s" % (u.netloc, path)
    if u.path.endswith("/api/gql") or "graphql" in u.path:
        op = parse_qs(u.query).get("operationName", ["?"])[0]
        key += "  [%s]" % op
    return key


def load_entries(paths):
    files = []
    for p in paths:
        files += sorted(glob.glob(os.path.join(p, "*.har"))) if os.path.isdir(p) else [p]
    for f in files:
        with open(f, encoding="utf-8") as fh:
            har = json.load(fh)
        for e in har["log"]["entries"]:
            yield os.path.basename(f), e


def body_text(entry):
    c = entry["response"].get("content", {})
    t = c.get("text")
    if t and c.get("encoding") == "base64":
        import base64
        try:
            t = base64.b64decode(t).decode("utf-8", "replace")
        except ValueError:
            return None
    return t


def parse_time(s):
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except (ValueError, AttributeError):
        return None


# --- main ---
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("paths", nargs="+")
    ap.add_argument("-o", "--out", default="har_report")
    args = ap.parse_args()
    os.makedirs(os.path.join(args.out, "bodies"), exist_ok=True)

    col = Collector()
    eps = defaultdict(lambda: {"methods": Counter(), "status": Counter(), "mime": Counter(), "n": 0,
                               "req_headers": defaultdict(set), "resp_headers": defaultdict(set),
                               "params": set(), "gql": set(), "example": None, "sizes": []})
    files_seen = set()
    n_entries = 0
    for fname, e in load_entries(args.paths):
        files_seen.add(fname)
        n_entries += 1
        req, resp = e["request"], e["response"]
        url = req["url"]
        host = urlparse(url).netloc
        if any(h in host for h in NOISE_HOSTS):
            continue
        key = endpoint_key(url)
        ep = eps[key]
        ep["n"] += 1
        ep["methods"][req["method"]] += 1
        ep["status"][resp["status"]] += 1
        mime = (resp.get("content", {}).get("mimeType") or "").split(";")[0]
        ep["mime"][mime or "-"] += 1
        ep["example"] = ep["example"] or unquote(url)
        for h in req["headers"]:
            name = h["name"].lower()
            if name in INTERESTING_REQ_HEADERS:
                if name == "authorization":
                    # only the scheme ("Bearer"), never the token: HARs recorded logged in have it
                    scheme, _, token = h["value"].partition(" ")
                    v = scheme + " <redacted>" if token else "<redacted>"
                elif name == "next-router-state-tree":
                    v = "<url-encoded router tree>"
                else:
                    v = h["value"]
                ep["req_headers"][name].add(v[:120])
        for h in resp["headers"]:
            if h["name"].lower() in INTERESTING_RESP_HEADERS:
                ep["resp_headers"][h["name"].lower()].add(h["value"][:120])
        q = parse_qs(urlparse(url).query)
        ep["params"].update(q.keys())
        if "operationName" in q:
            ext = q.get("extensions", ["{}"])[0]
            try:
                h = json.loads(ext).get("persistedQuery", {}).get("sha256Hash", "?")
            except ValueError:
                h = "?"
            ep["gql"].add("%s  vars=%s  sha256=%s" % (q["operationName"][0], q.get("variables", [""])[0], h))
        if req.get("postData", {}).get("text") and "graphql" in (req["postData"].get("mimeType") or "") + url:
            ep["gql"].add("POST body: " + req["postData"]["text"][:300])

        text = body_text(e)
        if not text:
            continue
        ep["sizes"].append(len(text))
        captured = e.get("startedDateTime", "")
        page = re.sub(r"^/[a-z]{2}-[A-Z]{2}", "/{locale}", ID_RE.sub("{id}", urlparse(url).path)) or "/"
        safe = re.sub(r"[^A-Za-z0-9]+", "_", urlparse(url).path)[-80:]
        ext = {"application/json": ".json", "text/x-component": ".rsc", "text/html": ".html"}.get(mime, "")
        if ext or "javascript" in mime:
            with open(os.path.join(args.out, "bodies", "%03d%s%s" % (n_entries, safe, ext or ".js")), "w", encoding="utf-8") as fh:
                fh.write(text)

        if mime == "application/json" or text[:1] in "{[" and mime != "text/x-component":
            try:
                col.walk(json.loads(text), "json:" + key, captured)
            except ValueError:
                pass
        elif mime == "text/x-component":
            rows = parse_rsc(text)
            for _, (kind, val) in rows.items():
                if kind == "J":
                    col.walk(val, page, captured)
            col.walk_components(rows, page)
        elif mime == "text/html":
            stream = rsc_from_html(text)
            if stream:
                rows = parse_rsc(stream)
                for _, (kind, val) in rows.items():
                    if kind == "J":
                        col.walk(val, page + " (html)", captured)
                col.walk_components(rows, page + " (html)")
        elif "javascript" in mime:
            col.scan_js(text, url)

    # --- schema ---
    schema = {}
    for tn, fields in col.types.items():
        schema[tn] = {}
        for k, F in fields.items():
            ex = [x for x, _ in F["examples"].most_common()]
            enum_like = (k not in ENUM_SKIP and all(isinstance(x, str) for x in ex) and 0 < len(ex) <= 12
                         and sum(F["examples"].values()) > len(ex))
            schema[tn][k] = {"types": dict(F["types"]), "seen": F["n"],
                             "values" if enum_like else "example": ex if enum_like else (ex[0] if ex else None)}
    with open(os.path.join(args.out, "schema.json"), "w", encoding="utf-8") as fh:
        json.dump(schema, fh, indent=1, ensure_ascii=False, default=str)

    # --- matches ---
    def fullness(m):
        return sum(1 for v in m.values() if v not in (None, "$undefined") and not (isinstance(v, str) and v.startswith("$")))

    def teams_of(m):
        mt = m.get("matchTeams")
        return [t for t in mt if isinstance(t, dict)] if isinstance(mt, list) else []

    def real(v):
        return not (v == "$undefined" or (isinstance(v, str) and v.startswith("$") and ":" in v))

    rows_out, checks = [], defaultdict(list)
    for mid, copies in col.matches.items():
        # Merge all the copies of a match, oldest capture first, so the newest one wins.
        # RSC payloads repeat a match many times, some copies are stubs ({id, state})
        # and some fields are references ("$5:1:props:...") and not values.
        copies = sorted(copies, key=lambda c: (c[1], fullness(c[2])))
        m = {}
        for _, _, c in copies:
            for k, v in c.items():
                if real(v) and (v is not None or k not in m):
                    m[k] = v
                elif k not in m and v == "$undefined":
                    m[k] = None
        page = max(copies, key=lambda c: fullness(c[2]))[0]
        cap = copies[-1][1]
        states = sorted({c[2].get("state") for c in copies if c[2].get("state")})
        teams = teams_of(m)
        if not teams:
            checks["only seen as a stub (id + state, no teams)"].append(
                {"id": mid, "state": m.get("state"), "description": "", "team1": "", "wins1": "", "wins2": "",
                 "team2": "", "startTime": "", "captured": cap})
            continue
        res = [(t.get("code"), (t.get("result") or {}).get("gameWins") if isinstance(t.get("result"), dict) else None,
                (t.get("result") or {}).get("outcome") if isinstance(t.get("result"), dict) else None) for t in teams]
        st = m.get("startTime")
        st = None if st in (None, "$undefined") or (isinstance(st, str) and st.startswith("$")) else st
        row = {"id": mid, "state": m.get("state"), "startTime": st or "", "description": m.get("description", ""),
               "team1": res[0][0] if len(res) > 0 else "", "wins1": res[0][1] if len(res) > 0 else "",
               "outcome1": res[0][2] if len(res) > 0 else "",
               "team2": res[1][0] if len(res) > 1 else "", "wins2": res[1][1] if len(res) > 1 else "",
               "outcome2": res[1][2] if len(res) > 1 else "",
               "flags": "|".join(m["flags"]) if isinstance(m.get("flags"), list) else "",
               "states_seen": "|".join(states), "captured": cap, "copies": len(copies), "page": page}
        rows_out.append(row)
        capdt, stdt = parse_time(cap), parse_time(st) if st else None
        if len(states) > 1:
            checks["state changed between captures"].append(row)
        if not st:
            checks["no startTime (unscheduled / TBD)"].append(row)
        if row["state"] == "completed":
            w = [r for r in res if r[2] == "win"]
            lo = [r for r in res if r[2] == "loss"]
            if not w or not lo:
                checks["completed but no winner/loser outcome"].append(row)
            elif (w[0][1] or 0) <= (lo[0][1] or 0):
                checks["completed but winner has <= game wins"].append(row)
        if row["state"] == "unstarted" and capdt and stdt and stdt < capdt:
            checks["unstarted although startTime has passed"].append(row)
        if row["state"] == "unstarted" and any(r[1] for r in res):
            checks["unstarted but has game wins"].append(row)
        if row["state"] == "inProgress":
            age = (capdt - stdt).total_seconds() / 3600 if capdt and stdt else None
            row["hours_since_start"] = round(age, 2) if age is not None else ""
            checks["inProgress at capture time"].append(row)

    rows_out.sort(key=lambda r: (r["startTime"] or "9999", r["id"]))
    fields = ["id", "state", "startTime", "description", "team1", "wins1", "outcome1", "team2", "wins2",
              "outcome2", "flags", "states_seen", "captured", "copies", "page"]
    with open(os.path.join(args.out, "matches.csv"), "w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=fields, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows_out)

    # --- report ---
    L = []
    L.append("# HAR exploration report\n")
    L.append("Generated %s from %d HAR file(s), %d requests.\n" % (
        datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC"), len(files_seen), n_entries))
    L.append("## Endpoints\n")
    for key, ep in sorted(eps.items(), key=lambda kv: -kv[1]["n"]):
        L.append("### `%s`\n" % key)
        L.append("- calls: %d, methods: %s, status: %s, content: %s" % (
            ep["n"], dict(ep["methods"]), dict(ep["status"]), dict(ep["mime"])))
        if ep["sizes"]:
            L.append("- body size: %d to %d chars" % (min(ep["sizes"]), max(ep["sizes"])))
        if ep["params"]:
            L.append("- query params: %s" % ", ".join(sorted(ep["params"])))
        for h, vals in sorted(ep["req_headers"].items()):
            L.append("- request header `%s`: %s" % (h, " / ".join(sorted(vals)[:4])))
        for h, vals in sorted(ep["resp_headers"].items()):
            L.append("- response header `%s`: %s" % (h, " / ".join(sorted(vals)[:3])))
        for g in sorted(ep["gql"]):
            L.append("- GraphQL: `%s`" % g)
        L.append("- example: `%s`\n" % ep["example"][:300])

    L.append("## Data components per page (RSC)\n")
    for page, comps in sorted(col.component_props.items()):
        L.append("**%s**\n" % page)
        for c in sorted(comps):
            L.append("- `%s`" % c[:400])
        L.append("")

    L.append("## Inferred types\n")
    for tn in sorted(schema):
        L.append("### %s  (seen on %d page patterns)\n" % (tn, len(col.type_pages[tn])))
        L.append("| field | types seen | values / example |")
        L.append("|---|---|---|")
        for k, F in schema[tn].items():
            types = ", ".join("%s(%d)" % (t, n) for t, n in sorted(F["types"].items(), key=lambda x: -x[1]))
            if "values" in F:
                ex = "enum: " + ", ".join("`%s`" % v for v in F["values"])
            else:
                ex = "`%s`" % str(F.get("example"))[:70] if F.get("example") is not None else ""
            L.append("| %s | %s | %s |" % (k, types, ex.replace("|", "\\|")))
        L.append("")

    L.append("## Match sanity checks\n")
    L.append("%d unique matches. State counts: %s\n" % (
        len(rows_out), dict(Counter(r["state"] for r in rows_out))))
    for name in ("completed but no winner/loser outcome", "completed but winner has <= game wins",
                 "unstarted although startTime has passed", "unstarted but has game wins",
                 "state changed between captures", "inProgress at capture time"):
        checks.setdefault(name, [])
    for name in sorted(checks):
        rs = checks[name]
        L.append("### %s: %d\n" % (name, len(rs)))
        for r in rs[:12]:
            L.append("- %s `%s` %s %s %s-%s %s start=%s captured=%s%s" % (
                r["id"], r["state"], r["description"], r["team1"], r["wins1"], r["wins2"], r["team2"],
                r["startTime"] or "-", r["captured"][:19],
                ("  (%sh since start)" % r["hours_since_start"]) if r.get("hours_since_start") not in (None, "") else ""))
        if len(rs) > 12:
            L.append("- ... %d more in matches.csv" % (len(rs) - 12))
        L.append("")

    if col.js_findings:
        L.append("## Found in JS bundles\n")
        for k, vals in sorted(col.js_findings.items()):
            L.append("### %s (%d)\n" % (k, len(vals)))
            for v in sorted(vals)[:80]:
                L.append("- `%s`" % v)
            L.append("")
    else:
        L.append("## JS bundles\n\nNone captured. Record with the Network filter set to *All* to include them.\n")

    with open(os.path.join(args.out, "report.md"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(L))
    print("Wrote %s (endpoints: %d, types: %d, matches: %d)" % (
        os.path.join(args.out, "report.md"), len(eps), len(schema), len(rows_out)))


if __name__ == "__main__":
    sys.exit(main())
