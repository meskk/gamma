#!/usr/bin/env python3
"""Seed the local dev database with demo content so the app has something to
design against (users, posts incl. a couple private ones, and follows).

Runs entirely over the public HTTP API (register / login / create post / follow),
so it always matches the real request contract and produces valid argon2 password
hashes — there is no direct DB access here. Standard library only (no pip install).

Prerequisites: the stack is up and `core-api` is reachable (see
docs/DESIGNER-SETUP.md). Local dev has `GAMMA_RATE_LIMIT_DISABLED=true`, so the
burst of registrations is not throttled.

Usage:
    python3 ops/seed-demo.py                 # against http://localhost:8080/v1
    SEED_API_BASE=http://host:8080/v1 python3 ops/seed-demo.py

Idempotent: accounts that already exist are reused (login), and their content is
NOT re-created, so re-running does not pile up duplicate posts.

All demo accounts share the password below — throwaway local data only.
"""

import json
import os
import sys
import urllib.error
import urllib.request

API_BASE = os.environ.get("SEED_API_BASE", "http://localhost:8080/v1").rstrip("/")
PASSWORD = "poolsite-demo-2026"  # >= 8 chars; local throwaway accounts only

# --- demo personas -----------------------------------------------------------
# (email, [categories], [public post bodies], [private post bodies])
USERS = [
    (
        "mia.foto@demo.poolsite",
        ["fotografie", "reisen"],
        [
            "Goldene Stunde über den Dächern von Lissabon 🌅",
            "Neue Serie: Straßenporträts aus Neapel. Analog, Kodak Portra 400.",
            "Nebelmorgen im Schwarzwald – 5 Uhr aufstehen hat sich gelohnt.",
            "Behind the Scenes vom gestrigen Shooting am Hafen.",
            "Welches Objektiv für Available-Light-Porträts? Meine drei Favoriten.",
            "Langzeitbelichtung: die Seine bei Nacht wird zu Seide.",
            "Kleiner Tipp: RAW schießen und in den Schatten Reserven lassen.",
            "Roadtrip an die Amalfiküste – Teil 1 der Bilderstrecke.",
        ],
        [
            "Exklusiv: das komplette Lightroom-Preset-Pack zu meiner Lissabon-Serie.",
            "Ungeschnittene Behind-the-Scenes-Galerie vom Hafen-Shooting (48 Bilder).",
        ],
    ),
    (
        "jonas.beats@demo.poolsite",
        ["musik"],
        [
            "Neuer Loop am Start – 90 BPM, viel Rhodes, wenig Schlaf. 🎹",
            "Warum ich wieder auf Hardware-Sampler umgestiegen bin.",
            "Snippet aus dem kommenden Tape. Feedback willkommen!",
            "Mixdown-Nacht. Kaffee Nummer vier.",
            "Drei Plugins, die meinen Sound komplett verändert haben.",
            "Live-Set vom Wochenende ist jetzt oben.",
        ],
        [],
    ),
    (
        "lena.kocht@demo.poolsite",
        ["food", "kochen"],
        [
            "One-Pot-Pasta mit Zitrone und Spinat – 15 Minuten, ein Topf. 🍋",
            "Sauerteig-Update: Tag 6, und Anton (mein Starter) lebt.",
            "Meal-Prep für die Woche: fünf Bowls, ein Einkauf.",
            "Das beste Schokoladenbrownie-Rezept, das ich je gebacken habe.",
            "Marktbesuch am Samstag: was gerade Saison hat.",
            "Ramen from scratch – die Brühe köchelt seit 12 Stunden.",
            "Reste-Küche: aus drei Zutaten wird Abendessen.",
        ],
        [],
    ),
    (
        "tim.code@demo.poolsite",
        ["tech", "programmieren"],
        [
            "Warum Rust für uns die richtige Wahl war – ein kurzer Thread.",
            "Heute gelernt: Postgres-Indizes sind kein Allheilmittel.",
            "Mein Terminal-Setup 2026. Ja, wieder mal neu.",
            "Kleiner Refactor, große Wirkung: -200 Zeilen, gleiche Features.",
            "Debugging-Story: der Bug war natürlich in meinem eigenen Code.",
            "CI grün, Kaffee leer, Freitag gerettet.",
        ],
        [],
    ),
    (
        "designer@demo.poolsite",
        ["design"],
        [
            "Playground-Account zum Ausprobieren des Glass-Looks.",
            "Test-Post mit etwas längerem Text, um die Kachel-Umbrüche im Grid zu sehen.",
            "Noch ein Post fürs Layout.",
        ],
        [],
    ),
]

FOLLOWS = {
    "mia.foto@demo.poolsite": ["lena.kocht@demo.poolsite", "jonas.beats@demo.poolsite"],
    "jonas.beats@demo.poolsite": ["mia.foto@demo.poolsite", "tim.code@demo.poolsite"],
    "lena.kocht@demo.poolsite": ["mia.foto@demo.poolsite"],
    "tim.code@demo.poolsite": ["mia.foto@demo.poolsite", "jonas.beats@demo.poolsite", "lena.kocht@demo.poolsite"],
    "designer@demo.poolsite": [
        "mia.foto@demo.poolsite",
        "jonas.beats@demo.poolsite",
        "lena.kocht@demo.poolsite",
        "tim.code@demo.poolsite",
    ],
}


def _req(method, path, token=None, body=None):
    url = API_BASE + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            parsed = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            parsed = raw
        return e.code, parsed
    except urllib.error.URLError as e:
        print(f"\n✗ Kann {url} nicht erreichen: {e.reason}", file=sys.stderr)
        print("  Läuft `core-api` auf dem erwarteten Port? Siehe docs/DESIGNER-SETUP.md.", file=sys.stderr)
        sys.exit(1)


def register_or_login(email, categories):
    """Return (token, user_id, is_new)."""
    status, resp = _req("POST", "/auth/register", body={
        "email": email, "password": PASSWORD, "declared_categories": categories,
    })
    if status in (200, 201) and resp:
        return resp["token"], resp["user_id"], True
    # Already exists (or any other register failure) → try to log in.
    status, resp = _req("POST", "/auth/login", body={"email": email, "password": PASSWORD})
    if status in (200, 201) and resp:
        return resp["token"], resp["user_id"], False
    print(f"✗ {email}: weder Register noch Login erfolgreich (HTTP {status}: {resp})", file=sys.stderr)
    return None, None, False


def main():
    print(f"Seed → {API_BASE}\n")
    accounts = {}   # email -> (token, user_id)
    new_emails = set()

    # 1) accounts
    for email, categories, *_ in USERS:
        token, uid, is_new = register_or_login(email, categories)
        if token is None:
            continue
        accounts[email] = (token, uid)
        if is_new:
            new_emails.add(email)
        print(f"  {'angelegt ' if is_new else 'vorhanden'}  {email}  (id {uid})")

    # 2) posts — only for freshly created accounts, so re-runs stay idempotent
    print()
    for email, _cats, public_bodies, private_bodies in USERS:
        if email not in accounts or email not in new_emails:
            continue
        token, _uid = accounts[email]
        n = 0
        for body in public_bodies:
            status, _ = _req("POST", "/posts", token=token, body={"body": body})
            n += 1 if status in (200, 201) else 0
        for body in private_bodies:
            status, _ = _req("POST", "/posts", token=token, body={"body": body, "area": "private"})
            n += 1 if status in (200, 201) else 0
        print(f"  {email}: {n} Posts ({len(private_bodies)} privat)")

    # 3) follows — only for freshly created followers (idempotent-ish; PUT is
    #    idempotent anyway, so re-adding an existing follow is harmless)
    print()
    for follower, targets in FOLLOWS.items():
        if follower not in accounts:
            continue
        token, _uid = accounts[follower]
        done = 0
        for target in targets:
            if target not in accounts:
                continue
            _tok, target_id = accounts[target]
            status, _ = _req("PUT", f"/me/following/{target_id}", token=token)
            done += 1 if status in (200, 201, 204) else 0
        print(f"  {follower} folgt {done}")

    print("\n✓ Fertig.\n")
    print("Login (alle Demo-Accounts, gleiches Passwort):")
    print(f"  Passwort: {PASSWORD}")
    for email in accounts:
        print(f"  · {email}")
    print("\nTipp: Als mia.foto@demo.poolsite einloggen und das eigene Profil öffnen —")
    print("dort erscheinen auch die privaten Posts mit Lock-Badge.")


if __name__ == "__main__":
    main()
