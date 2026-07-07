#!/usr/bin/env python3
"""Load smoke for the two hot paths (MASTERPLAN M4.7): feed reads + paid unlocks.

Stdlib only (Python >= 3.9) so it runs from any machine — the VM, a laptop, a
second box — without installing anything. It drives the REAL public API end to
end: register -> verify (bot gate) -> paid media (real presigned upload +
finalize) -> likes -> settle an epoch (mints gems) -> feed reads at a target
rate -> concurrent unlocks with exact money assertions -> conservation check.

Sizing model (documented in docs/OPERATIONS.md §9): 10k registered users,
~10% concurrently active, one feed page per active user per ~10 s => ~100
feed reads/s at peak. Unlocks are orders of magnitude rarer; the burst here
deliberately overshoots to surface write contention on the money path.

Two seeding steps need `docker compose exec postgres` (run it on the machine
that hosts the stack, or with DOCKER_HOST pointing there):
  * promoting the smoke operator (no public endpoint for that, by design), and
  * the "time machine": the current epoch is NOT settleable by design
    (epoch_not_closed), so the freshly recorded likes are shifted one epoch
    back before settling yesterday's epoch. Drill-only; touches nothing but
    the smoke's own fresh stack.

Run against a FRESH stack with rate limits disabled for the run
(GAMMA_RATE_LIMIT_DISABLED=true — we measure the app, not the limiter). The
ops/compose.smoke.yml override also keeps the settlement-scheduler OUT of the
stack — it would settle yesterday's epoch away (empty, marker) before the
time machine fills it. Laptop drill (GAMMA_IMAGE_TAG empty, self-build):

  docker compose -f compose.prod.yml -f ops/compose.smoke.yml up -d --build
  python3 ops/load-smoke.py --base-url http://localhost:8080

On a VM with GAMMA_IMAGE_TAG set, follow the M4.6 rule instead (never build
under the SHA name): `... pull && ... up -d --no-build` (OPERATIONS.md §9).

Exit code 0 iff every threshold holds — including the ACHIEVED feed rate, so
a throughput collapse cannot hide behind a healthy-looking p95 — and every
money assertion is exact.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from http.client import HTTPConnection, HTTPSConnection
from urllib.parse import urlparse

EPOCH_SECONDS = 86_400


# ── HTTP client (keep-alive, one connection per thread) ─────────────────────


class Api:
    def __init__(self, base_url: str):
        u = urlparse(base_url)
        self.scheme = u.scheme or "http"
        self.host = u.hostname or "localhost"
        self.port = u.port or (443 if self.scheme == "https" else 80)
        self._local = threading.local()

    def _conn(self):
        conn = getattr(self._local, "conn", None)
        if conn is None:
            cls = HTTPSConnection if self.scheme == "https" else HTTPConnection
            conn = cls(self.host, self.port, timeout=30)
            self._local.conn = conn
        return conn

    def request(self, method: str, path: str, token=None, body=None):
        """Returns (status, parsed_json_or_None, latency_seconds)."""
        payload = None
        headers = {}
        if body is not None:
            payload = json.dumps(body)
            headers["Content-Type"] = "application/json"
        if token:
            headers["Authorization"] = "Bearer " + token
        # One retry for a stale keep-alive connection, never for a real answer.
        for attempt in (1, 2):
            conn = self._conn()
            t0 = time.monotonic()
            try:
                conn.request(method, path, body=payload, headers=headers)
                resp = conn.getresponse()
                raw = resp.read()
                latency = time.monotonic() - t0
                data = json.loads(raw) if raw else None
                return resp.status, data, latency
            except Exception:
                self._local.conn = None
                if attempt == 2:
                    raise
        raise AssertionError("unreachable")


def expect(status, data, wanted, what: str):
    if isinstance(wanted, int):
        wanted = (wanted, 201) if wanted == 200 else (wanted,)
    if status not in wanted:
        if status == 429:
            die(
                f"{what}: 429 rate-limited — run the smoke stack with "
                "GAMMA_RATE_LIMIT_DISABLED=true (see OPERATIONS.md §9)"
            )
        die(f"{what}: expected {wanted}, got {status}: {data}")
    return data


def die(msg: str):
    print(f"FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


# ── docker-compose seeding helpers (operator promotion + time machine) ──────


def psql(compose_file: str, sql: str) -> str:
    # The statement travels as an env var into the container, so the in-container
    # shell never interpolates it; the values spliced into SQL below are only
    # integers this script generated.
    cmd = [
        "docker", "compose", "-f", compose_file, "exec", "-T",
        "--env", "GAMMA_SMOKE_SQL=" + sql, "postgres",
        "sh", "-c",
        'exec psql -U "${POSTGRES_USER:-gamma}" -d "${POSTGRES_DB:-gamma}" '
        '-v ON_ERROR_STOP=1 -Atc "$GAMMA_SMOKE_SQL"',
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        die(f"psql seeding step failed: {proc.stderr.strip()}")
    return proc.stdout.strip()


# ── presigned upload (Host header preserved, connect address overridable) ───


def upload_presigned(url: str, content_type: str, body: bytes, connect: str):
    u = urlparse(url)
    host_header = u.netloc
    if connect:
        chost, _, cport = connect.partition(":")
        port = int(cport or (443 if u.scheme == "https" else 80))
    else:
        chost, port = u.hostname, u.port or (443 if u.scheme == "https" else 80)
    cls = HTTPSConnection if u.scheme == "https" else HTTPConnection
    conn = cls(chost, port, timeout=30)
    path = u.path + ("?" + u.query if u.query else "")
    conn.putrequest("PUT", path, skip_host=True)
    conn.putheader("Host", host_header)  # SigV4 signs the Host header
    conn.putheader("Content-Type", content_type)
    conn.putheader("Content-Length", str(len(body)))
    conn.endheaders()
    conn.send(body)
    resp = conn.getresponse()
    resp.read()
    conn.close()
    if resp.status not in (200, 204):
        die(f"presigned upload got {resp.status} — with the bundled MinIO use "
            "ops/compose.smoke.yml and --s3-connect 127.0.0.1:9100")


# ── metrics ──────────────────────────────────────────────────────────────────


def percentile(samples, q: float) -> float:
    xs = sorted(samples)
    return xs[min(len(xs) - 1, int(len(xs) * q))]


def report_phase(name: str, samples, errors: int, wall: float):
    ms = [s * 1000 for s in samples]
    print(
        f"{name}: n={len(ms)} err={errors} rate={len(ms) / wall:.1f}/s "
        f"p50={percentile(ms, 0.50):.0f}ms p95={percentile(ms, 0.95):.0f}ms "
        f"p99={percentile(ms, 0.99):.0f}ms max={max(ms):.0f}ms"
    )


# ── the smoke ────────────────────────────────────────────────────────────────


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base-url", default="http://localhost:8080")
    ap.add_argument("--compose-file", default="compose.prod.yml",
                    help="stack that hosts postgres (for the two seeding steps)")
    ap.add_argument("--s3-connect", default="127.0.0.1:9100",
                    help="connect address for presigned uploads whose URL host "
                         "is compose-internal (ops/compose.smoke.yml publishes "
                         "MinIO there; empty = connect to the URL host)")
    ap.add_argument("--users", type=int, default=50)
    ap.add_argument("--likes-per-user", type=int, default=5)
    ap.add_argument("--unlock-price", type=int, default=1_000_000,
                    help="PT base units per paid asset")
    ap.add_argument("--feed-rps", type=int, default=100)
    ap.add_argument("--feed-secs", type=int, default=60)
    ap.add_argument("--feed-warmup-secs", type=int, default=5)
    ap.add_argument("--unlocks-per-user", type=int, default=3)
    ap.add_argument("--unlock-concurrency", type=int, default=20)
    ap.add_argument("--max-feed-p95-ms", type=float, default=300.0)
    ap.add_argument("--max-unlock-p95-ms", type=float, default=500.0)
    ap.add_argument("--min-feed-rate-ratio", type=float, default=0.95,
                    help="fail unless achieved feed rate >= ratio * --feed-rps")
    args = ap.parse_args()

    api = Api(args.base_url)
    run = str(int(time.time()) % 1_000_000)
    k = args.users

    # Liveness first — everything else is noise if this fails.
    st, _, _ = api.request("GET", "/health")
    if st != 200:
        die(f"/health returned {st} — is the stack up?")

    # ── Seed: users ─────────────────────────────────────────────────────────
    print(f"seed: registering {k} users (run {run}) …")
    users = []  # {id, token, asset, post}
    for i in range(k):
        st, data, _ = api.request(
            "POST", "/v1/auth/register",
            body={
                "email": f"smoke-{run}-{i}@smoke.example",
                "password": f"Smoke-{run}-pass-{i}!",
                "declared_categories": ["tech"],
            },
        )
        data = expect(st, data, 200, f"register user {i}")
        users.append({"id": data["user_id"], "token": data["token"]})

    # Operator = the first smoke user, promoted via SQL (no endpoint, by design).
    op = users[0]
    psql(args.compose_file,
         f"UPDATE users SET role = 'operator' WHERE id = {int(op['id'])};")
    print(f"seed: user {op['id']} promoted to operator")

    # Bot gate: only verified users are paid by settlement.
    for u in users:
        st, data, _ = api.request(
            "PUT", f"/v1/users/{u['id']}/verification",
            token=op["token"], body={"verified": True},
        )
        expect(st, data, 200, f"verify user {u['id']}")

    # ── Seed: one paid asset + post per user (real upload + finalize) ──────
    print("seed: paid media (upload + finalize) + posts …")
    blob = b"gamma-load-smoke-" + run.encode()
    for i, u in enumerate(users):
        st, ticket, _ = api.request(
            "POST", "/v1/media", token=u["token"],
            body={"kind": "image", "content_type": "image/png",
                  "unlock_price": args.unlock_price},
        )
        ticket = expect(st, ticket, 200, f"media ticket for user {i}")
        upload_presigned(ticket["upload_url"], "image/png", blob, args.s3_connect)
        st, asset, _ = api.request(
            "POST", f"/v1/media/{ticket['asset_id']}/finalize", token=u["token"])
        asset = expect(st, asset, 200, f"finalize asset of user {i}")
        if asset["status"] != "ready":
            die(f"asset of user {i} not ready after finalize: {asset['status']}")
        st, post, _ = api.request(
            "POST", "/v1/posts", token=u["token"],
            body={"category": "tech", "body": f"smoke {run} paid post {i}",
                  "media_id": ticket["asset_id"]},
        )
        post = expect(st, post, 200, f"post of user {i}")
        u["asset"] = ticket["asset_id"]
        u["post"] = post["id"]

    # ── Seed: likes (the interaction graph settlement will pay out on) ─────
    print(f"seed: {k * args.likes_per_user} likes …")
    for i, u in enumerate(users):
        for d in range(1, args.likes_per_user + 1):
            other = users[(i + d) % k]
            st, data, _ = api.request(
                "POST", "/v1/interactions", token=u["token"],
                body={"type": "like", "target_id": None, "post_id": other["post"]},
            )
            expect(st, data, 200, f"like {i}->{(i + d) % k}")

    # ── Time machine + settle: mint real balances ───────────────────────────
    # The current epoch is not settleable by design (epoch_not_closed), so
    # shift the smoke's interactions one epoch back and settle that epoch.
    today = int(time.time()) // EPOCH_SECONDS
    psql(
        args.compose_file,
        "UPDATE interaction_events SET epoch_k = epoch_k - 1, "
        "created_at = created_at - interval '1 day' "
        f"WHERE epoch_k = {today};",
    )
    st, summary, _ = api.request(
        "POST", f"/v1/epochs/{today - 1}/settle", token=op["token"])
    summary = expect(st, summary, 200, "settle")
    print(f"settle: epoch {today - 1} emission={summary['emission']} "
          f"users={summary['user_count']} already={summary['already_settled']}")
    if int(summary["emission"]) <= 0 or summary["already_settled"]:
        die("settlement returned already_settled / zero emission — either the "
            "stack was not fresh, or a settlement-scheduler ran in it and "
            "settled the epoch empty first (ops/compose.smoke.yml keeps it "
            "out of the smoke stack for exactly that reason)")

    def balance(uid) -> int:
        st, data, _ = api.request("GET", f"/v1/users/{uid}/gems", token=op["token"])
        return int(expect(st, data, 200, f"gems of {uid}")["balance"])

    need = args.unlock_price * args.unlocks_per_user
    balances_before = {int(u["id"]): balance(u["id"]) for u in users}
    company_before = balance(0)
    poorest = min(balances_before.values())
    if poorest < need:
        die(f"poorest user has {poorest} < {need} needed — raise emission side "
            "(more likes) or lower --unlock-price")
    print(f"balances: poorest={poorest} (need {need}/user) company={company_before}")

    # ── Phase 1: feed reads at target rate ──────────────────────────────────
    print(f"feed: {args.feed_rps} req/s for {args.feed_secs}s "
          f"(+{args.feed_warmup_secs}s warmup) …")
    samples, errors = [], [0]
    lock = threading.Lock()
    workers = min(32, args.feed_rps)
    per_worker_interval = workers / args.feed_rps
    t_start = time.monotonic()
    t_measure = t_start + args.feed_warmup_secs
    t_end = t_measure + args.feed_secs

    def feed_worker(widx: int):
        nxt = t_start + (widx / args.feed_rps)  # stagger the start
        while True:
            now = time.monotonic()
            if now >= t_end:
                return
            if nxt > now:
                time.sleep(nxt - now)
            # Never repay missed ticks as a catch-up burst: a server stall must
            # surface as a rate shortfall (asserted below), not be papered over.
            nxt = max(nxt + per_worker_interval, time.monotonic())
            u = users[(widx + int(nxt * 7)) % k]
            try:
                st, _, lat = api.request(
                    "GET", f"/v1/users/{u['id']}/feed?limit=20", token=u["token"])
            except Exception:
                st, lat = -1, 0.0
            if time.monotonic() >= t_measure:
                with lock:
                    if st == 200:
                        samples.append(lat)
                    else:
                        errors[0] += 1

    with ThreadPoolExecutor(max_workers=workers) as ex:
        list(ex.map(feed_worker, range(workers)))
    if not samples:
        die("feed phase produced no samples")
    report_phase("feed", samples, errors[0], args.feed_secs)
    feed_p95 = percentile([s * 1000 for s in samples], 0.95)
    feed_errors = errors[0]

    # ── Phase 2: concurrent unlocks with exact money assertions ────────────
    plan = [
        (u, users[(i + d) % k]["asset"], users[(i + d) % k]["id"])
        for i, u in enumerate(users)
        for d in range(1, args.unlocks_per_user + 1)
    ]
    print(f"unlock: {len(plan)} unlocks at concurrency "
          f"{args.unlock_concurrency} …")

    def unlock(viewer, asset_id):
        st, data, lat = api.request(
            "POST", f"/v1/media/{asset_id}/unlock", token=viewer["token"])
        return st, data, lat

    ulat, earned, spent = [], {}, {}
    total_burned = total_fees = 0
    t_unlock = time.monotonic()
    with ThreadPoolExecutor(max_workers=args.unlock_concurrency) as ex:
        results = list(ex.map(lambda p: unlock(p[0], p[1]), plan))
    unlock_wall = max(time.monotonic() - t_unlock, 0.001)
    for (viewer, asset_id, owner_id), (st, data, lat) in zip(plan, results):
        data = expect(st, data, 200, f"unlock asset {asset_id}")
        if data["already_unlocked"]:
            die(f"first unlock of asset {asset_id} claims already_unlocked")
        price, fee = int(data["price"]), int(data["company_fee"])
        burn, creator = int(data["burned"]), int(data["creator_received"])
        if price != args.unlock_price or creator + fee + burn != price:
            die(f"split not conserved for asset {asset_id}: "
                f"{creator}+{fee}+{burn} != {price}")
        ulat.append(lat)
        total_burned += burn
        total_fees += fee
        spent[int(viewer["id"])] = spent.get(int(viewer["id"]), 0) + price
        earned[int(owner_id)] = earned.get(int(owner_id), 0) + creator
    report_phase("unlock", ulat, 0, unlock_wall)
    unlock_p95 = percentile([s * 1000 for s in ulat], 0.95)

    # Idempotency: the same unlocks again must be free of charge.
    with ThreadPoolExecutor(max_workers=args.unlock_concurrency) as ex:
        results = list(ex.map(lambda p: unlock(p[0], p[1]), plan))
    for (viewer, asset_id, _), (st, data, _) in zip(plan, results):
        data = expect(st, data, 200, f"re-unlock asset {asset_id}")
        if not data["already_unlocked"]:
            die(f"re-unlock of asset {asset_id} charged again")
    print("unlock: idempotency round clean (all already_unlocked)")

    # Conservation: user+company deltas match the unlock receipts exactly.
    balances_after = {int(u["id"]): balance(u["id"]) for u in users}
    company_after = balance(0)
    for u in users:
        uid = int(u["id"])
        want = balances_before[uid] + earned.get(uid, 0) - spent.get(uid, 0)
        if balances_after[uid] != want:
            die(f"balance of user {uid} is {balances_after[uid]}, expected {want}")
    if company_after - company_before != total_fees:
        die(f"company fee mismatch: delta {company_after - company_before} "
            f"!= {total_fees}")
    total_delta = (sum(balances_after.values()) + company_after) - (
        sum(balances_before.values()) + company_before)
    if total_delta != -total_burned:
        die(f"conservation violated: total delta {total_delta} != -{total_burned}")
    print(f"money: conservation exact (burned={total_burned}, fees={total_fees})")

    # ── Thresholds ───────────────────────────────────────────────────────────
    failed = []
    if feed_errors:
        failed.append(f"feed errors={feed_errors} (threshold 0)")
    achieved = len(samples) / args.feed_secs
    if achieved < args.min_feed_rate_ratio * args.feed_rps:
        failed.append(
            f"feed rate={achieved:.1f}/s < "
            f"{args.min_feed_rate_ratio * args.feed_rps:.0f}/s — the load level "
            "was not actually delivered (server too slow or generator saturated)")
    if feed_p95 > args.max_feed_p95_ms:
        failed.append(f"feed p95={feed_p95:.0f}ms > {args.max_feed_p95_ms:.0f}ms")
    if unlock_p95 > args.max_unlock_p95_ms:
        failed.append(
            f"unlock p95={unlock_p95:.0f}ms > {args.max_unlock_p95_ms:.0f}ms")
    if failed:
        die("thresholds breached: " + "; ".join(failed))
    print("PASS: all thresholds held, money math exact")


if __name__ == "__main__":
    main()
