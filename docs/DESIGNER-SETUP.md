# Poolsite lokal einrichten (für Designer)

Diese Anleitung bringt die **komplette Plattform lokal auf deinem Mac** zum
Laufen — mit Demo-Inhalten zum Durchdesignen. Du arbeitest danach nur am
**Frontend** (`frontend/`, Next.js + TypeScript); das Backend läuft im
Hintergrund und liefert die Daten.

Zeitaufwand beim ersten Mal: ~20–30 Min (der Rust-Build dauert einmalig).

---

## 0. Was läuft am Ende?

| Teil | Was | Adresse |
|------|-----|---------|
| Frontend (dein Arbeitsbereich) | Next.js Dev-Server | http://localhost:3000 |
| Backend-API | Rust `core-api` | http://localhost:8080 |
| Datenbank + Cache + Storage | Postgres / Redis / MinIO (in Docker) | — |

Du brauchst **vier Terminal-Tabs** (Docker, Backend, Frontend, + einer für
Befehle). Details unten.

---

## 1. Einmalige Installation (Voraussetzungen)

Alles über [Homebrew](https://brew.sh). Falls Homebrew fehlt, zuerst das
installieren.

```sh
# Container-Laufzeit (kein Docker Desktop nötig)
brew install colima docker docker-compose

# Rust (die Projekt-Toolchain 1.96 wird automatisch aus rust-toolchain.toml gezogen)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Node 22 (für das Frontend)
brew install node@22
```

Python 3 ist auf dem Mac vorinstalliert (für das Seed-Skript, siehe Schritt 5).

---

## 2. Repo holen

Du brauchst Lesezugriff auf das GitHub-Repo (Antonio lädt dich ein).

```sh
git clone https://github.com/meskk/gamma.git
cd gamma
```

---

## 3. Konfiguration anlegen

Zwei lokale Config-Dateien aus den Vorlagen kopieren (beide sind absichtlich
nicht eingecheckt):

```sh
cp backend/.env.example  backend/.env
cp frontend/.env.example frontend/.env.local
```

Die Standardwerte passen für lokal — **nichts ändern nötig**. (Wichtig ist nur,
dass `GAMMA_RATE_LIMIT_DISABLED=true` in `backend/.env` steht; das ist die
Vorgabe.)

---

## 4. Backend + Infrastruktur starten

**Tab 1 – Infrastruktur** (Postgres/Redis/MinIO):

```sh
colima start                 # startet die Container-VM (einmal pro Boot)
cd backend
docker compose up -d         # DB, Cache, Storage im Hintergrund
```

**Tab 2 – Backend-API:**

```sh
cd backend
cargo run -p core-api        # erster Build dauert ein paar Minuten
```

Warte, bis in der Ausgabe steht:

```
core_api: database connected and migrations applied
```

Die Datenbankstruktur wird dabei **automatisch** angelegt — kein manueller
Migrations-Schritt.

---

## 5. Demo-Daten einspielen

**Tab 3 – Befehle** (Backend muss aus Schritt 4 laufen):

```sh
cd backend
python3 ops/seed-demo.py
```

Das legt fünf Demo-Accounts mit Posts (inkl. privater Posts) und Follows an und
gibt am Ende die Login-Daten aus. Alle Accounts haben dasselbe Passwort:

```
Passwort:  poolsite-demo-2026
Accounts:  mia.foto@demo.poolsite      (viele Posts, 2 privat)
           jonas.beats@demo.poolsite
           lena.kocht@demo.poolsite
           tim.code@demo.poolsite
           designer@demo.poolsite      (dein Spiel-Account)
```

Das Skript ist **wiederholbar** — nochmals ausführen legt keine Doubletten an.
Du kannst dich natürlich auch jederzeit über „Registrieren" selbst anmelden.

---

## 6. Frontend starten (dein Arbeitsbereich)

**Tab 4 – Frontend:**

```sh
cd frontend
npm install                  # einmalig
npm run dev                  # http://localhost:3000
```

Öffne http://localhost:3000, melde dich mit einem Demo-Account an (Schritt 5) —
fertig. Änderungen an Dateien in `frontend/` erscheinen sofort (Hot Reload).

Tipp: Als `mia.foto@demo.poolsite` einloggen und oben rechts aufs Profil — dort
siehst du das „Glass · Profile"-Design mit den privaten Posts (Lock-Badge).

---

## Täglicher Start (nach dem ersten Setup)

```sh
colima start                                   # Tab 1
cd backend && docker compose up -d             # Tab 1
cd backend && cargo run -p core-api            # Tab 2
cd frontend && npm run dev                     # Tab 4
```

Zum Stoppen: in den Tabs `Ctrl-C`, dann optional `colima stop`.

---

## Woran du arbeitest

- Der ganze sichtbare Code liegt in **`frontend/`** (Next.js App Router, TypeScript).
- Styling-Ansatz ist offen — aktuell Inline-Styles / CSS-Module. Du kannst
  Tailwind, CSS-Module o. Ä. ergänzen, wie du magst.
- Die bereits umgesetzten „Glass"-Screens als Referenz:
  `app/login/`, `app/feed/` (Reels) und `app/users/[id]/` (Profil);
  gemeinsame Icons in `components/reels/icons.tsx`.
- **Eine Regel:** alles, was die API-Grenze kreuzt, geht über `@contract/*`
  (die aus dem Backend generierten Typen) — dann bleibt die Kompilier-Sicherheit
  erhalten. Details in `frontend/README.md`.

## Optional / nicht nötig fürs Design

Die Zusatz-Prozesse `transcode_worker` (Video-Transcoding) und
`settlement_scheduler` (Ökonomie-Epochen) brauchst du fürs Durchdesignen **nicht**
zu starten.

---

## Wenn etwas klemmt

| Symptom | Ursache / Lösung |
|--------|------------------|
| `cannot connect to the Docker daemon` | `colima start` vergessen. |
| `core-api`: `Address already in use` | Es läuft schon eine Instanz auf :8080 — die reicht. |
| Seed-Skript: „Kann … nicht erreichen" | Backend (Tab 2) läuft noch nicht / noch nicht fertig gebaut. |
| Login schlägt fehl | Demo-Daten noch nicht eingespielt (Schritt 5). |
| Frontend zeigt leere Seiten | Nicht eingeloggt, oder Backend nicht erreichbar (`.env.local` prüfen). |
| `npm run dev`: Port 3000 belegt | Anderer Dev-Server läuft; diesen beenden oder Port freigeben. |

Bei allem anderen: Screenshot + die Fehlermeldung aus dem jeweiligen Terminal an
Antonio.
