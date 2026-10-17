#!/usr/bin/env python3
"""
recorder.py - record the lolesports API during live matches, to analyse later.

Polls the old lolesports API (still working) without abusing it, and saves every
response with timestamps. With that we can see when and how the states change:
series start, start/pause/end of each game, and series end.

    python3 recorder.py                       # auto: follow every live / about-to-start match
    python3 recorder.py --match 115660588583040710   # follow one match only
    python3 recorder.py --league worlds --hours 6 --details

Standard library only, Python 3.8+. Stop it with Ctrl+C.

Polling (the CDN caches everything 60 s, polling faster gives nothing):
    getLive, getSchedule ............ every 60 s
    getEventDetails ................. every 30 s per tracked match
    livestats window ................ every 10 s per live game
    livestats details (--details) ... every 10 s per live game (per-player stats)

Disk (gzip): ~1 MB per hour of live game, ~2.5 MB/hour with --details.
A Bo5 is usually 5-10 MB. Then run:  python3 summarize.py <recording folder>

Output: recordings/<start time>/
    api.jsonl.gz          getLive / getSchedule / getEventDetails
    feed_<gameId>.jsonl.gz livestats responses for that game
    recorder.log          what you saw on screen
Each line is one response: {"t", "server_t", "kind", "url", "status", "date",
"age", "x_cache", "same", "body"}. "same": true means the body is the same as
the previous response for that URL (then the body is not saved).
"""
import argparse
import gzip
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from email.utils import parsedate_to_datetime

API = "https://esports-api.lolesports.com/persisted/gw"
FEED = "https://feed.lolesports.com/livestats/v1"
# public key of the lolesports web client, LOLESPORTS_API_KEY overrides it if Riot changes it
API_KEY = os.environ.get("LOLESPORTS_API_KEY", "0TvQnueqKa5mxJntVWt0w4LpLfEkrV1Ta8rQBb9Z")
UA = "lol-hobby-recorder/1.0 (personal, non-commercial; polls at most every 10 s)"

LIVE_EVERY, SCHEDULE_EVERY, EVENT_EVERY, FEED_EVERY, NEXT_GAME_EVERY = 60, 60, 30, 10, 30
FEED_LAG = 40          # windows must end >= 30 s ago (the server refuses windows < 20 s old)
UPCOMING = timedelta(minutes=45)   # start following a match this long before it starts
SETTLE = timedelta(minutes=30)     # keep following a match this long after it ends


def utcnow():
    return datetime.now(timezone.utc)


def iso(dt):
    return dt.strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def parse_iso(s):
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except (AttributeError, ValueError):
        return None


class Recorder:
    def __init__(self, args):
        self.args = args
        self.out = os.path.join(args.out, utcnow().strftime("%Y%m%d-%H%M%S"))
        os.makedirs(self.out, exist_ok=True)
        self.log_fh = open(os.path.join(self.out, "recorder.log"), "a", encoding="utf-8")
        self.last_hash = {}
        self.clock_offset = 0.0            # server time - local time, seconds
        self.clock_samples = 0
        self.matches = {}                  # match id -> {"until": dt or None, "label": str, "next": ts}
        self.games = {}                    # game id -> {"match", "number", "next", "finished_polls", "first_done"}
        self.next_game_probe = {}          # game id -> {"match", "next"} (unstarted game right after a finished one)
        self.due = {"live": 0, "schedule": 0}
        if args.match:
            for m in args.match:
                self.matches[m] = {"until": None, "label": m, "next": 0, "pinned": True}

    # --- io ---
    def log(self, msg):
        line = "%s  %s" % (utcnow().strftime("%H:%M:%S"), msg)
        print(line, flush=True)
        self.log_fh.write(line + "\n")
        self.log_fh.flush()

    def server_now(self):
        return utcnow() + timedelta(seconds=self.clock_offset)

    def fetch(self, kind, url, file, api_key=False):
        headers = {"User-Agent": UA, "Accept": "application/json", "Accept-Encoding": "gzip"}
        if api_key:
            headers["x-api-key"] = API_KEY
        req = urllib.request.Request(url, headers=headers)
        t0 = utcnow()
        status, raw, hdrs = None, b"", {}
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                status, raw, hdrs = r.status, r.read(), dict(r.headers)
        except urllib.error.HTTPError as e:
            status, raw, hdrs = e.code, e.read(), dict(e.headers or {})
        except Exception as e:  # network error: save it and continue
            self.write(file, {"t": iso(t0), "kind": kind, "url": url, "status": None, "error": repr(e)})
            self.log("!! %s network error: %s" % (kind, e))
            return None, None
        if hdrs.get("Content-Encoding", "").lower() == "gzip" and raw:
            raw = gzip.decompress(raw)
        # clock: server time = Date + Age (Age > 0 when the answer comes from the CloudFront cache)
        try:
            server = parsedate_to_datetime(hdrs["Date"]) + timedelta(seconds=int(hdrs.get("Age", 0) or 0))
            sample = (server - t0).total_seconds()
            if self.clock_samples == 0:
                self.clock_offset = sample
                if abs(sample) > 5:
                    self.log("note: your clock differs from the server by %.0f s; correcting for it" % sample)
            else:
                self.clock_offset = 0.8 * self.clock_offset + 0.2 * sample
            self.clock_samples += 1
        except (KeyError, TypeError, ValueError):
            pass
        text = raw.decode("utf-8", "replace")
        try:
            body = json.loads(text) if text else None
        except ValueError:
            body = text
        h = hashlib.sha1(raw).hexdigest()
        same = self.last_hash.get(url) == h
        self.last_hash[url] = h
        rec = {"t": iso(t0), "server_t": iso(self.server_now()), "kind": kind, "url": url, "status": status,
               "date": hdrs.get("Date"), "age": hdrs.get("Age"), "x_cache": hdrs.get("X-Cache"), "same": same}
        if not same:
            rec["body"] = body
        self.write(file, rec)
        return status, body

    def write(self, file, rec):
        with gzip.open(os.path.join(self.out, file), "at", encoding="utf-8") as fh:
            fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")) + "\n")

    # --- discovery ---
    def wanted(self, league_slug):
        return not self.args.league or (league_slug or "") in self.args.league

    def follow(self, mid, label, until=None):
        m = self.matches.get(mid)
        if m is None:
            self.matches[mid] = {"until": until, "label": label, "next": 0, "pinned": False}
            self.log("+ following match %s  %s" % (mid, label))
        elif not m.get("pinned"):
            m["until"] = until
            m["label"] = label

    def poll_live(self):
        status, body = self.fetch("getLive", "%s/getLive?hl=en-US" % API, "api.jsonl.gz", api_key=True)
        if status != 200 or not isinstance(body, dict):
            return
        for ev in ((body.get("data") or {}).get("schedule") or {}).get("events") or []:
            m = ev.get("match") or {}
            if ev.get("type") == "match" and m.get("id") and self.wanted((ev.get("league") or {}).get("slug")):
                self.follow(m["id"], self.label(ev))

    def poll_schedule(self):
        status, body = self.fetch("getSchedule", "%s/getSchedule?hl=en-US" % API, "api.jsonl.gz", api_key=True)
        if status != 200 or not isinstance(body, dict):
            return
        now = self.server_now()
        for ev in ((body.get("data") or {}).get("schedule") or {}).get("events") or []:
            m = ev.get("match") or {}
            if ev.get("type") != "match" or not m.get("id") or not self.wanted((ev.get("league") or {}).get("slug")):
                continue
            start = parse_iso(ev.get("startTime"))
            if ev.get("state") == "inProgress" or (
                    ev.get("state") == "unstarted" and start and now - SETTLE < start <= now + UPCOMING):
                self.follow(m["id"], self.label(ev))

    @staticmethod
    def label(ev):
        m = ev.get("match") or {}
        teams = m.get("teams") or []
        codes = " vs ".join(t.get("code") or "?" for t in teams)
        bo = (m.get("strategy") or {}).get("count")
        return "%s %s (Bo%s, %s)" % ((ev.get("league") or {}).get("slug", "?"), codes, bo, ev.get("startTime", "")[:16])

    # --- per match ---
    def poll_event(self, mid):
        status, body = self.fetch("getEventDetails", "%s/getEventDetails?hl=en-US&id=%s" % (API, mid),
                                  "api.jsonl.gz", api_key=True)
        if status != 200 or not isinstance(body, dict):
            return
        ev = (body.get("data") or {}).get("event") or {}
        match = ev.get("match") or {}
        teams = match.get("teams") or []
        games = match.get("games") or []
        score = " ".join("%s %s" % (t.get("code"), (t.get("result") or {}).get("gameWins")) for t in teams)
        gstates = " ".join("%s:%s" % (g.get("number"), (g.get("state") or "?")[:4]) for g in games)
        self.log("match %s  %s  Bo%s  games[%s]" % (mid, score, (match.get("strategy") or {}).get("count"), gstates))
        m = self.matches[mid]
        live_game = False
        for i, g in enumerate(games):
            gid, st = g.get("id"), g.get("state")
            if st in ("completed", "unneeded") and gid in self.games:
                self.log("  game %s (#%s) is %s in the API; stop polling its feed" % (gid, g.get("number"), st))
                del self.games[gid]
            if st == "inProgress":
                live_game = True
                if gid not in self.games:
                    self.games[gid] = {"match": mid, "number": g.get("number"), "next": 0,
                                       "finished_polls": 0, "first_done": False}
                    self.log("  + game %s (#%s) is live, polling feed every %ss" % (gid, g.get("number"), FEED_EVERY))
                self.next_game_probe.pop(gid, None)
            elif st == "completed" and i + 1 < len(games) and games[i + 1].get("state") == "unstarted":
                nxt = games[i + 1].get("id")
                if nxt and nxt not in self.games and nxt not in self.next_game_probe:
                    # check the feed for the next game, it can start before the API says so
                    self.next_game_probe[nxt] = {"match": mid, "next": 0}
        # decide how long to keep following this match
        all_done = games and all(g.get("state") in ("completed", "unneeded") for g in games)
        if all_done and not live_game and m.get("until") is None and not m.get("pinned"):
            m["until"] = self.server_now() + SETTLE
            self.log("  series looks finished; will keep recording it for %d more min" % (SETTLE.seconds // 60))

    # --- per game ---
    def poll_feed(self, gid, g):
        """Fetch every 10-s window in order, so we don't skip one if a poll is late.

        Windows are asked one after the other from the last one we have, up to the newest
        allowed one (floor10(server_now - FEED_LAG)). A 400 (too early) or a network
        error keeps the position, so that window is asked again next time.
        After the feed says "finished", only the newest window is fetched.
        """
        file = "feed_%s.jsonl.gz" % gid
        now = self.server_now() - timedelta(seconds=FEED_LAG)
        latest = now.replace(second=now.second - now.second % 10, microsecond=0)
        if g.get("next_window") is None or g["finished_polls"]:
            g["next_window"] = latest
        last_frames, last_status, fetched = None, None, 0
        while g["next_window"] <= latest and fetched < 6:   # catch up at most 1 min per tick
            st = g["next_window"].strftime("%Y-%m-%dT%H:%M:%S.000Z")
            status, body = self.fetch("window", "%s/window/%s?startingTime=%s" % (FEED, gid, st), file)
            last_status = status
            if status not in (200, 204):
                break
            if self.args.details:
                self.fetch("details", "%s/details/%s?startingTime=%s" % (FEED, gid, st), file)
            frames = body.get("frames") if isinstance(body, dict) else None
            if frames:
                last_frames = frames
                if not g["first_done"]:
                    # the game was already running when we started, get its first frames too
                    self.fetch("window-first", "%s/window/%s" % (FEED, gid), file)
            g["first_done"] = True
            g["next_window"] += timedelta(seconds=10)
            fetched += 1
        if last_frames:
            f = last_frames[-1]
            b, r = f.get("blueTeam") or {}, f.get("redTeam") or {}
            self.log("  feed %s  %s  %-9s kills %s-%s  gold %.1fk-%.1fk  towers %s-%s%s" % (
                gid, f.get("rfc460Timestamp", "")[11:19], f.get("gameState"), b.get("totalKills"),
                r.get("totalKills"), (b.get("totalGold") or 0) / 1000, (r.get("totalGold") or 0) / 1000,
                b.get("towers"), r.get("towers"), "  (+%d windows)" % (fetched - 1) if fetched > 1 else ""))
            if any(fr.get("gameState") == "finished" for fr in last_frames):
                g["finished_polls"] += 1
        elif last_status == 204:
            self.log("  feed %s  no data yet (HTTP 204: game not started)" % gid)
        elif last_status not in (200, None):
            self.log("  feed %s  HTTP %s" % (gid, last_status))
        return g["finished_polls"] >= 3

    # --- loop ---
    def run(self):
        self.log("recording to %s  (Ctrl+C to stop)" % self.out)
        end = time.time() + self.args.hours * 3600 if self.args.hours else None
        while end is None or time.time() < end:
            try:
                self.tick()
            except Exception as e:  # one bad response must not stop the recorder
                self.log("!! unexpected error (continuing): %r" % e)
            time.sleep(1)
        self.log("time limit reached, stopping")

    def tick(self):
        t = time.time()
        if not self.args.match:
            if t >= self.due["live"]:
                self.due["live"] = t + LIVE_EVERY
                self.poll_live()
            if t >= self.due["schedule"]:
                self.due["schedule"] = t + SCHEDULE_EVERY
                self.poll_schedule()
        now = self.server_now()
        for mid in list(self.matches):
            m = self.matches[mid]
            if m["until"] and now > m["until"]:
                self.log("- stop following match %s" % mid)
                del self.matches[mid]
                continue
            if t >= m["next"]:
                m["next"] = t + EVENT_EVERY
                self.poll_event(mid)
        for gid in list(self.games):
            g = self.games[gid]
            if g["match"] not in self.matches:
                del self.games[gid]
                continue
            if t >= g["next"]:
                finished = self.poll_feed(gid, g)
                # After the feed says "finished", keep checking slowly until the API marks the
                # game completed. That delay is what we want to measure.
                g["next"] = t + (EVENT_EVERY if finished else FEED_EVERY)
                if finished and not g.get("finish_logged"):
                    g["finish_logged"] = True
                    self.log("  game %s is finished in the feed; waiting for the API to agree" % gid)
        for gid in list(self.next_game_probe):
            p = self.next_game_probe[gid]
            if p["match"] not in self.matches:
                del self.next_game_probe[gid]
            elif t >= p["next"]:
                p["next"] = t + NEXT_GAME_EVERY
                st, body = self.fetch("window-probe", "%s/window/%s" % (FEED, gid), "feed_%s.jsonl.gz" % gid)
                if st == 200 and isinstance(body, dict) and body.get("frames"):
                    self.log("  feed for next game %s already has data" % gid)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--match", action="append", help="match id to follow (repeatable); disables auto-discovery")
    ap.add_argument("--league", action="append", help="only follow these league slugs, e.g. worlds, lec, lck")
    ap.add_argument("--details", action="store_true", help="also record livestats/details (per-player stats, large)")
    ap.add_argument("--hours", type=float, default=0, help="stop after this many hours (default: run until Ctrl+C)")
    ap.add_argument("--out", default="recordings", help="output folder (default: ./recordings)")
    rec = Recorder(ap.parse_args())
    try:
        rec.run()
    except KeyboardInterrupt:
        rec.log("stopped by user")
    print("\nTo get a small summary:  python3 summarize.py %s" % rec.out)


if __name__ == "__main__":
    sys.exit(main())
