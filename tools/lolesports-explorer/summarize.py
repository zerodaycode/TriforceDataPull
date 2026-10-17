#!/usr/bin/env python3
"""
summarize.py - turn a recorder.py folder into a short timeline to share.

    python3 summarize.py recordings/20261015-225800
    python3 summarize.py recordings/20261015-225800 --frames     # also export frame CSVs

Writes in the recording folder:
    timeline.md     key moments per game and series, every state change in order,
                    feed errors, gaps and possible pauses (a few KB)
    summary.json    the same as JSON
    frames_<gameId>.csv   (only with --frames) one row per unique feed frame

Standard library only, Python 3.8+. Truncated .gz files are fine (for example after Ctrl+C).
"""
import argparse
import csv
import glob
import gzip
import json
import os
import re
import sys
import zlib
from collections import defaultdict
from datetime import datetime, timedelta

GAP_SECONDS = 30        # no frames for this long while in game -> "gap"
FROZEN_SECONDS = 60     # total gold unchanged this long while in_game -> "possible pause" (gold goes up every second in a normal game)


def read_jsonl_gz(path):
    try:
        with gzip.open(path, "rt", encoding="utf-8") as fh:
            for line in fh:
                try:
                    yield json.loads(line)
                except ValueError:
                    continue
    except (EOFError, OSError, zlib.error):
        return  # file cut at the end (recorder stopped while writing): keep what we have


def ts(s):
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except (AttributeError, ValueError):
        return None


def secs(a, b):
    a, b = ts(a), ts(b)
    return round((b - a).total_seconds(), 1) if a and b else None


def hms(s):
    return s[11:19] if s else "-"


class Summary:
    def __init__(self, folder):
        self.folder = folder
        self.events = []                      # (t, source, match_id, text)
        self.matches = {}                     # match id -> info
        self.games = {}                       # game id -> info
        self.errors = defaultdict(lambda: {"count": 0, "first": None, "last": None, "example": None})
        self.first_t = self.last_t = None

    def seen(self, t):
        if t:
            self.first_t = min(self.first_t or t, t)
            self.last_t = max(self.last_t or t, t)

    def ev(self, t, source, mid, text):
        self.events.append((t, source, mid, text))

    def match(self, mid):
        return self.matches.setdefault(mid, {"id": mid, "label": mid, "bestOf": None, "teams": {},
                                             "moments": {}, "games": {}})

    def moment(self, mid, key, t, note=""):
        m = self.match(mid)["moments"]
        if key not in m:
            m[key] = {"t": t, "note": note}

    def error(self, source, rec):
        body = rec.get("body")
        e = self.errors["%s HTTP %s" % (source, rec.get("status") or "network error")]
        e["count"] += 1
        e["first"] = e["first"] or rec.get("t")
        e["last"] = rec.get("t")
        if rec.get("same"):
            e["cached_repeats"] = e.get("cached_repeats", 0) + 1
        if e["example"] is None:
            if isinstance(body, dict):
                e["example"] = ("%s: %s" % (body.get("errorCode") or "", body.get("message") or ""))[:300]
            elif rec.get("error"):
                e["example"] = rec["error"][:300]

    # --- api ---
    def load_api(self):
        recs = []
        for path in sorted(glob.glob(os.path.join(self.folder, "api*.jsonl.gz"))):
            recs.extend(read_jsonl_gz(path))
        recs.sort(key=lambda r: r.get("t") or "")
        followed = {re.search(r"[?&]id=(\d+)", r["url"]).group(1) for r in recs
                    if r.get("kind") == "getEventDetails" and re.search(r"[?&]id=(\d+)", r.get("url", ""))}
        last_sig, live_prev = {}, None
        for r in recs:
            t, kind = r.get("t"), r.get("kind")
            self.seen(t)
            if r.get("status") != 200:
                self.error(kind, r)
                continue
            if r.get("same") or not isinstance(r.get("body"), dict):
                continue
            data = r["body"].get("data") or {}
            if kind in ("getSchedule", "getLive"):
                events = (data.get("schedule") or {}).get("events") or []
                ids_now = set()
                for e in events:
                    m = e.get("match") or {}
                    mid = m.get("id")
                    if not mid or mid not in followed:
                        continue
                    ids_now.add(mid)
                    info = self.match(mid)
                    teams = m.get("teams") or []
                    codes = [tm.get("code") for tm in teams]
                    bo = (m.get("strategy") or {}).get("count")
                    info["bestOf"] = info["bestOf"] or bo
                    info["label"] = "%s (%s, Bo%s)" % (" vs ".join(c or "?" for c in codes),
                                                       (e.get("league") or {}).get("slug", "?"), bo)
                    res = [((tm.get("result") or {}).get("gameWins"), (tm.get("result") or {}).get("outcome"))
                           for tm in teams]
                    sig = (e.get("state"), tuple(res))
                    key = (kind, mid)
                    if last_sig.get(key) != sig:
                        old = last_sig.get(key)
                        last_sig[key] = sig
                        score = " ".join("%s %s%s" % (c, w if w is not None else "-", "(%s)" % o if o else "")
                                         for c, (w, o) in zip(codes, res))
                        self.ev(t, kind, mid, "%s%s  %s" % ("" if old else "first seen: ", e.get("state"), score))
                        self.series_moments(mid, kind, t, e.get("state"), res, bo)
                if kind == "getLive":
                    if live_prev is not None:
                        for mid in ids_now - live_prev:
                            self.ev(t, kind, mid, "appeared in getLive")
                        for mid in live_prev - ids_now:
                            self.ev(t, kind, mid, "left getLive")
                            self.moment(mid, "series_end:getLive_left", t)
                    live_prev = ids_now
            elif kind == "getEventDetails":
                ev = data.get("event") or {}
                mid = ev.get("id")
                m = ev.get("match") or {}
                if not mid:
                    continue
                info = self.match(mid)
                teams = m.get("teams") or []
                bo = (m.get("strategy") or {}).get("count")
                info["bestOf"] = info["bestOf"] or bo
                for tm in teams:
                    info["teams"][tm.get("id")] = tm.get("code")
                wins = tuple((tm.get("code"), (tm.get("result") or {}).get("gameWins")) for tm in teams)
                games = tuple((g.get("number"), g.get("id"), g.get("state")) for g in m.get("games") or [])
                for num, gid, _ in games:
                    info["games"][gid] = num
                    self.games.setdefault(gid, {"id": gid, "match": mid, "number": num})
                # other details: side per game, VODs per game, number of streams
                code = info["teams"]
                extra = {"sides": {g.get("number"): "/".join(
                             "%s %s" % (code.get(tm.get("id"), tm.get("id")), tm.get("side"))
                             for tm in g.get("teams") or []) for g in m.get("games") or []},
                         "vods": {g.get("number"): len(g.get("vods") or []) for g in m.get("games") or []},
                         "streams": len(ev.get("streams") or [])}
                okey = ("getEventDetails-extra", mid)
                oextra = last_sig.get(okey)
                if oextra and oextra != extra:
                    for n, s in extra["sides"].items():
                        if oextra["sides"].get(n) != s:
                            self.ev(t, kind, mid, "game %s sides: %s -> %s" % (n, oextra["sides"].get(n), s))
                    for n, v in extra["vods"].items():
                        if oextra["vods"].get(n) != v:
                            self.ev(t, kind, mid, "game %s VODs: %s -> %s" % (n, oextra["vods"].get(n), v))
                    if oextra["streams"] != extra["streams"]:
                        self.ev(t, kind, mid, "streams: %s -> %s" % (oextra["streams"], extra["streams"]))
                last_sig[okey] = extra
                key = ("getEventDetails", mid)
                old = last_sig.get(key)
                if old != (wins, games):
                    last_sig[key] = (wins, games)
                    if old is None:
                        self.ev(t, kind, mid, "first seen: %s  games[%s]" % (
                            " ".join("%s %s" % w for w in wins), " ".join("%s:%s" % (n, s) for n, _, s in games)))
                    else:
                        ow, og = old
                        if ow != wins:
                            self.ev(t, kind, mid, "score %s -> %s" % (" ".join("%s %s" % w for w in ow),
                                                                       " ".join("%s %s" % w for w in wins)))
                        oldg = {gid: s for _, gid, s in og}
                        for num, gid, s in games:
                            if oldg.get(gid) != s:
                                self.ev(t, kind, mid, "game %s: %s -> %s" % (num, oldg.get(gid, "(new)"), s))
                    for num, gid, s in games:
                        if s == "inProgress":
                            self.moment(mid, "game%s_start:api" % num, t)
                        if s in ("completed", "unneeded"):
                            self.moment(mid, "game%s_end:api" % num, t, s)
                    res = [(w, None) for _, w in wins]
                    st = "completed" if games and all(s in ("completed", "unneeded") for _, _, s in games) else None
                    self.series_moments(mid, kind, t, st, res, bo)

    def series_moments(self, mid, source, t, state, res, bo):
        wins = [w for w, _ in res if isinstance(w, int)]
        if bo and wins and max(wins) >= bo // 2 + 1:
            self.moment(mid, "series_end:%s_majority" % source, t, "a team reached %d wins" % (bo // 2 + 1))
        if any(o for _, o in res):
            self.moment(mid, "series_end:%s_outcome" % source, t, "outcome set")
        if state == "completed":
            self.moment(mid, "series_end:%s_state" % source, t, "state completed")

    # --- feed ---
    def load_feed(self, export_frames):
        for path in sorted(glob.glob(os.path.join(self.folder, "feed_*.jsonl.gz"))):
            gid = re.search(r"feed_(\d+)", path).group(1)
            g = self.games.setdefault(gid, {"id": gid, "match": None, "number": None})
            frames = {}                     # rfc460Timestamp -> (first poll t, frame, kind)
            asked = []                      # startingTime of every successful window poll
            polls = stale = 0
            stale_streak_start, longest_stale = None, (0, None, None)
            newest = None
            meta = None
            for r in sorted(read_jsonl_gz(path), key=lambda x: x.get("t") or ""):
                t = r.get("t")
                self.seen(t)
                kind = r.get("kind")
                if r.get("status") not in (200, 204):
                    self.error("feed " + kind, r)
                    continue
                if kind == "details":
                    continue
                polls += 1
                m = re.search(r"startingTime=([^&]+)", r.get("url", ""))
                if m:
                    asked.append(m.group(1))
                body = r.get("body")
                if r.get("same"):
                    body = None
                new = False
                if isinstance(body, dict):
                    g["match"] = g.get("match") or body.get("esportsMatchId")
                    meta = meta or body.get("gameMetadata")
                    for f in body.get("frames") or []:
                        k = f.get("rfc460Timestamp")
                        if k and k not in frames:
                            frames[k] = (t, f, kind)
                            if newest is None or k > newest:
                                newest, new = k, True
                if kind in ("window", "window-probe", "window-first") and not new and frames:
                    stale += 1
                    stale_streak_start = stale_streak_start or t
                    d = secs(stale_streak_start, t) or 0
                    if d > longest_stale[0]:
                        longest_stale = (d, stale_streak_start, t)
                elif new:
                    stale_streak_start = None
            g.update(self.analyse_frames(gid, frames, meta, asked))
            # 10-s windows the recorder never asked for (before the game finished) = holes in our data
            fin = next((tr["frame"] for tr in g.get("transitions", []) if tr["to"] == "finished"), None)
            got = sorted({ts(a) for a in asked if ts(a)})
            if got:
                stop = min(got[-1], ts(fin)) if fin and ts(fin) else got[-1]
                have, n, cur = set(got), 0, got[0]
                while cur < stop:
                    n += cur not in have
                    cur += timedelta(seconds=10)
                g["skipped_windows"] = n
            g["polls"] = polls
            g["stale_polls"] = stale
            g["longest_stale"] = {"seconds": longest_stale[0], "from": longest_stale[1], "to": longest_stale[2]}
            if export_frames and frames:
                self.write_frames(gid, frames)

    def analyse_frames(self, gid, frames, meta, asked):
        out = {"frames": len(frames), "transitions": [], "gaps": [], "frozen": []}
        if not frames:
            return out
        order = sorted(frames)
        out["first_frame"], out["last_frame"] = order[0], order[-1]
        prev_state, prev_k = None, None
        gold_since, gold_val = None, None
        g = self.games[gid]
        mid = g.get("match")
        num = g.get("number")
        for k in order:
            seen_t, f, kind = frames[k]
            if kind != "window":
                # frames from the "first frames" call are not from the normal polling,
                # use them only for the game start, not for gaps or pauses
                if prev_state is None:
                    out["transitions"].append({"frame": k, "seen": seen_t, "from": None, "to": f.get("gameState")})
                    if mid:
                        self.ev(seen_t, "feed", mid, "game %s: first frame at %s (%s)" % (
                            num or gid, hms(k), f.get("gameState")))
                        self.moment(mid, "game%s_start:feed" % num, k, "first frame")
                    prev_state = f.get("gameState")
                continue
            st = f.get("gameState")
            if st != prev_state:
                out["transitions"].append({"frame": k, "seen": seen_t, "from": prev_state, "to": st})
                if mid:
                    label = "game %s" % (num or gid)
                    self.ev(seen_t, "feed", mid, "%s: gameState %s -> %s (frame time %s)" % (
                        label, prev_state or "(start)", st, hms(k)))
                    if prev_state is None:
                        self.moment(mid, "game%s_start:feed" % num, k, "first frame")
                    if st == "finished":
                        self.moment(mid, "game%s_end:feed_frame" % num, k, "frame time")
                        self.moment(mid, "game%s_end:feed_seen" % num, seen_t, "when we downloaded it")
                    if st == "paused":
                        self.moment(mid, "game%s_pause:feed" % num, k)
                prev_state = st
            if prev_k and st == "in_game":
                gap = secs(prev_k, k)
                if gap and gap >= GAP_SECONDS:
                    # did we ask for windows inside the gap? yes -> the feed had nothing; no -> we weren't polling
                    tried = sum(1 for a in asked if prev_k < a < k)
                    out["gaps"].append({"from": prev_k, "to": k, "seconds": gap, "windows_requested": tried,
                                        "cause": "feed had no frames" if tried else "recorder was not polling"})
            gold = ((f.get("blueTeam") or {}).get("totalGold"), (f.get("redTeam") or {}).get("totalGold"))
            if st == "in_game" and gold == gold_val:
                d = secs(gold_since, k)
                if d and d >= FROZEN_SECONDS and (not out["frozen"] or out["frozen"][-1]["from"] != gold_since):
                    out["frozen"].append({"from": gold_since, "to": k, "seconds": d})
                elif out["frozen"] and out["frozen"][-1]["from"] == gold_since:
                    out["frozen"][-1].update({"to": k, "seconds": d})
            else:
                gold_since, gold_val = k, gold
            prev_k = k
        last = frames[order[-1]][1]
        teams = self.matches.get(mid, {}).get("teams", {}) if mid else {}
        side_team = {}
        if meta:
            for side in ("blue", "red"):
                tid = (meta.get("%sTeamMetadata" % side) or {}).get("esportsTeamId")
                side_team[side] = teams.get(tid, tid)
        out["final"] = {side: {"team": side_team.get(side),
                               **{k: (last.get("%sTeam" % side) or {}).get(k) for k in
                                  ("totalKills", "towers", "inhibitors", "barons", "totalGold")},
                               "dragons": len((last.get("%sTeam" % side) or {}).get("dragons") or [])}
                        for side in ("blue", "red")}
        return out

    def write_frames(self, gid, frames):
        path = os.path.join(self.folder, "frames_%s.csv" % gid)
        with open(path, "w", newline="", encoding="utf-8") as fh:
            w = csv.writer(fh)
            w.writerow(["frame_time", "first_seen", "gameState", "blue_kills", "red_kills", "blue_gold", "red_gold",
                        "blue_towers", "red_towers", "blue_inhibs", "red_inhibs", "blue_barons", "red_barons",
                        "blue_dragons", "red_dragons"])
            for k in sorted(frames):
                t, f, _ = frames[k]
                b, r = f.get("blueTeam") or {}, f.get("redTeam") or {}
                w.writerow([k, t, f.get("gameState"), b.get("totalKills"), r.get("totalKills"), b.get("totalGold"),
                            r.get("totalGold"), b.get("towers"), r.get("towers"), b.get("inhibitors"),
                            r.get("inhibitors"), b.get("barons"), r.get("barons"), len(b.get("dragons") or []),
                            len(r.get("dragons") or [])])

    # --- output ---
    def write(self):
        L = ["# Recording summary", ""]
        L.append("- Folder: `%s`" % os.path.basename(os.path.normpath(self.folder)))
        L.append("- From %s to %s UTC (%s min)" % (
            (self.first_t or "-")[:19], (self.last_t or "-")[:19],
            round((secs(self.first_t, self.last_t) or 0) / 60)))
        sizes = sum(os.path.getsize(p) for p in glob.glob(os.path.join(self.folder, "*.gz")))
        L.append("- Raw data: %.1f MB compressed" % (sizes / 1e6))
        L.append("")
        for mid, m in self.matches.items():
            L.append("## %s  `%s`" % (m["label"], mid))
            L.append("")
            mo = m["moments"]
            if mo:
                L.append("| Moment | Source | Time (UTC) | Note |")
                L.append("|---|---|---|---|")
                for key in sorted(mo, key=lambda k: (mo[k]["t"] or "")):
                    what, src = key.split(":", 1)
                    what = re.sub(r"game(\d+|None)", r"game \1", what).replace("_", " ")
                    L.append("| %s | %s | %s | %s |" % (what, src.replace("_", " "), (mo[key]["t"] or "")[11:23],
                                                        mo[key]["note"]))
                L.append("")
                L.extend(self.delays(mid, mo))
            L.append("### Every change, in order")
            L.append("")
            L.append("```")
            for t, src, emid, text in sorted(e for e in self.events if e[2] == mid):
                L.append("%s  %-16s %s" % ((t or "")[11:19], src, text))
            L.append("```")
            L.append("")
        L.append("## Feed, per game")
        L.append("")
        for gid, g in sorted(self.games.items(), key=lambda kv: (kv[1].get("match") or "", kv[1].get("number") or 0)):
            if not g.get("polls"):
                continue
            L.append("### game %s  `%s`  (match `%s`)" % (g.get("number") or "?", gid, g.get("match")))
            L.append("- frames: %s, from %s to %s" % (g.get("frames"), hms(g.get("first_frame")), hms(g.get("last_frame"))))
            L.append("- polls: %s, polls with no new frames: %s, longest stretch without new frames: %ss (%s to %s)" % (
                g["polls"], g["stale_polls"], g["longest_stale"]["seconds"], hms(g["longest_stale"]["from"]),
                hms(g["longest_stale"]["to"])))
            if g.get("skipped_windows") is not None:
                L.append("- 10-s windows the recorder never requested before the end: %s%s" % (
                    g["skipped_windows"], " (holes in this recording, not in the feed)" if g["skipped_windows"] else ""))
            for tr in g.get("transitions", []):
                L.append("- gameState %s -> **%s** at frame %s (downloaded %s)" % (
                    tr["from"] or "(start)", tr["to"], hms(tr["frame"]), hms(tr["seen"])))
            for gp in g.get("gaps", []):
                L.append("- gap with no frames: %ss (%s to %s): %s (%d windows requested inside it)" % (
                    gp["seconds"], hms(gp["from"]), hms(gp["to"]), gp["cause"], gp["windows_requested"]))
            for fz in g.get("frozen", []):
                L.append("- possible pause (in_game, gold frozen): %ss (%s to %s)" % (
                    fz["seconds"], hms(fz["from"]), hms(fz["to"])))
            fin = g.get("final")
            if fin:
                L.append("- last frame: " + " | ".join(
                    "%s %s: %s kills, %s towers, %s inhibs, %s barons, %s drakes, %.1fk gold" % (
                        side, v["team"], v["totalKills"], v["towers"], v["inhibitors"], v["barons"], v["dragons"],
                        (v["totalGold"] or 0) / 1000) for side, v in fin.items()))
            L.append("")
        L.append("## Errors")
        L.append("")
        if not self.errors:
            L.append("None.")
        for k, e in sorted(self.errors.items()):
            L.append("- `%s`: %d times%s (%s to %s) %s" % (
                k, e["count"], (", %d identical repeats" % e["cached_repeats"]) if e.get("cached_repeats") else "",
                hms(e["first"]), hms(e["last"]), ("— " + e["example"]) if e["example"] else ""))
        L.append("")
        with open(os.path.join(self.folder, "timeline.md"), "w", encoding="utf-8") as fh:
            fh.write("\n".join(L))
        with open(os.path.join(self.folder, "summary.json"), "w", encoding="utf-8") as fh:
            json.dump({"from": self.first_t, "to": self.last_t, "matches": self.matches, "games": self.games,
                       "errors": self.errors, "events": self.events}, fh, indent=1, ensure_ascii=False)

    def delays(self, mid, mo):
        """How long each source took to show a game end / series end, compared with the feed."""
        out = []
        nums = sorted({int(k.split("_")[0][4:]) for k in mo if k.startswith("game") and k.split("_")[0][4:].isdigit()})
        rows = []
        for n in nums:
            first = (mo.get("game%d_start:feed" % n) or {}).get("t")
            if first and "game%d_start:api" % n in mo:
                d = secs(mo["game%d_start:api" % n]["t"], first)
                atleast = "at least " if (secs(self.first_t, mo["game%d_start:api" % n]["t"]) or 0) < 5 else ""
                rows.append("| game %d start | api said inProgress | %s%s before the first feed frame |" % (
                    n, atleast, ("%dm%02ds" % (d // 60, d % 60)) if d and d > 0 else "%ss" % d))
            ref = (mo.get("game%d_end:feed_frame" % n) or {}).get("t")
            for key in ("game%d_end:feed_seen" % n, "game%d_end:api" % n):
                if ref and key in mo:
                    rows.append("| game %d end | %s | +%ss |" % (n, key.split(":")[1], secs(ref, mo[key]["t"])))
        last = max(nums) if nums else None
        ref = (mo.get("game%d_end:feed_frame" % last) or {}).get("t") if last else None
        for key in sorted(k for k in mo if k.startswith("series_end")):
            if ref:
                rows.append("| series end | %s | +%ss |" % (key.split(":")[1], secs(ref, mo[key]["t"])))
        if rows:
            out += ["**Timing compared with the feed:**", "", "| What | Source | Delay |", "|---|---|---|"]
            out += rows + [""]
        return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder", help="a recordings/<start time> folder")
    ap.add_argument("--frames", action="store_true", help="also write frames_<gameId>.csv")
    args = ap.parse_args()
    s = Summary(args.folder)
    s.load_api()
    s.load_feed(args.frames)
    s.write()
    print("Wrote %s (%d matches, %d games with feed data, %d changes)" % (
        os.path.join(args.folder, "timeline.md"), len(s.matches),
        sum(1 for g in s.games.values() if g.get("polls")), len(s.events)))


if __name__ == "__main__":
    sys.exit(main())
