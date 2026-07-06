# Operations — die Ein-VM-Deploy-Story (Phase 1a-β)

Ziel (MASTERPLAN M4.4): Eine frische EU-VM kommt allein mit diesem Dokument von
Null auf „läuft über TLS". Bewusst klein: **eine VM, docker compose, Caddy als
TLS-Reverse-Proxy, manueller Deploy.** Kein Kubernetes, kein Auto-Deploy — das
sind spätere Entscheidungen, wenn 10.000 Nutzer real werden.

## 0. Voraussetzungen

- Eine Linux-VM (Debian/Ubuntu) bei einem EU-Anbieter (z. B. Hetzner; §8 im
  MASTERPLAN hält die endgültige Wahl fest). Richtwert für die Beta:
  4 vCPU / 8 GB RAM / 80 GB Disk — der Rust-Release-Build im `--build`-Deploy
  braucht allein ~4 GB.
- Eine Domain mit zwei A/AAAA-Records auf die VM, z. B. `api.example.com`
  (core-api) und `app.example.com` (Frontend).
- SSH-Zugang; alles Weitere passiert auf der VM.

## 1. Grundsystem

```sh
apt-get update && apt-get install -y docker.io docker-compose-v2 caddy ufw git
ufw allow OpenSSH && ufw allow 80/tcp && ufw allow 443/tcp && ufw enable
```

Nur SSH/80/443 sind offen. Postgres, Redis und MinIO werden von
`compose.prod.yml` NICHT veröffentlicht — sie existieren nur im
Compose-Netz; es gibt nichts weiter zu firewallen.

## 2. Code + Secrets

```sh
git clone https://github.com/meskk/gamma.git && cd gamma
cp .env.prod.example .env.prod
$EDITOR .env.prod        # CHANGE-ME-Werte setzen; Datei ist gitignored
```

Passwörter generieren: `openssl rand -base64 24`. `.env.prod` verlässt die VM
nie (ein echter Secrets-Manager ist ein 1b-Punkt, im MASTERPLAN §2 vermerkt).

## 3. Stack starten

```sh
docker compose -f compose.prod.yml up -d --build
docker compose -f compose.prod.yml ps    # alle Services healthy?
curl -s localhost:8080/health && curl -s localhost:8080/ready
```

Der erste `--build` kompiliert das Backend (~10 min). Migrationen laufen beim
core-api-Start automatisch (embedded, forward-only). Danach einmalig den
Ingestion-Service-Account anlegen (RUNBOOK §2: registrieren, dann
`UPDATE users SET role = 'service' …`) — die Credentials stehen ja schon in
`.env.prod`; der Worker heilt sich per Restart-Policy selbst.

**Bekanntes Verhalten (beobachtet im Abnahme-Drill):** Bis der Account
existiert, sammelt der neustartende Worker Login-Fehlversuche und läuft in die
eigene Brute-Force-Bremse (429). Das löst sich von allein (IP-Bucket erholt
sich in Sekunden, ein Konto-Lock nach Minuten). Wer nicht warten will:

```sh
docker compose -f compose.prod.yml exec postgres \
  psql -U gamma -d gamma -c "DELETE FROM login_throttle WHERE email = '<service-email>';"
docker compose -f compose.prod.yml restart ingestion
```

## 4. TLS (Caddy auf dem Host)

`/etc/caddy/Caddyfile`:

```
api.example.com {
    reverse_proxy 127.0.0.1:8080
}
app.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

`systemctl reload caddy` — Zertifikate holt Caddy selbst (Let's Encrypt).
Caddy setzt `X-Forwarded-For`; deshalb steht `GAMMA_TRUST_PROXY=true` in
`.env.prod`, damit die Rate-Limits den echten Client sehen.

## 5. Frontend

Das Frontend-Container-Image kommt mit M4.6 (GHCR-Publish). Für die Beta bis
dahin klassisch auf der VM:

```sh
cd frontend
NEXT_PUBLIC_API_BASE_URL=https://api.example.com/v1 npm ci && npm run build
npm start   # Port 3000; unter systemd legen (Restart=always)
```

## 6. Deploy & Rollback

Deploy (bis M4.6 baut die VM selbst):

```sh
git pull && docker compose -f compose.prod.yml up -d --build
```

Rollback = auf den letzten guten Stand zurück und identisch deployen:

```sh
git checkout <letzter-guter-tag-oder-commit>
docker compose -f compose.prod.yml up -d --build
```

Beides ist gefahrlos wiederholbar: Migrationen sind forward-only und
idempotent angewendet, Settlement und Queues sind crash-sicher/idempotent by
design. Mit M4.6 wird daraus `docker compose pull && up -d` mit
digest-gepinnten GHCR-Images; Rollback = voriger Digest.

## 7. Backup & Restore

**Was gesichert wird:** Postgres — die einzige Quelle der Wahrheit (Nutzer,
Posts, Graph, das `ledger_entries`-Geldjournal). Redis hält nur Queues und
braucht kein Backup: die Ingestion-Queue füllt der Admin-Backfill wieder
(RUNBOOK §5); verlorene Transcode-Jobs stößt der jeweilige Owner pro Asset neu
an (`POST /v1/media/:id/transcode`); ein Verlust der DLQ ist verschmerzbar.
MinIO-Medien sind NICHT im Dump — für die Beta deckt sie der
Provider-VM-Snapshot ab (unten); verlorene Medien kosten Content, kein Geld.

**Nächtlicher Dump** — `ops/pg-backup.sh` zieht einen `pg_dump`
(Custom-Format, komprimiert) aus dem Postgres-Container, validiert das Archiv
per `pg_restore --list` und rotiert (Default 14 Tage, nie unter den letzten
Dump). Als Cron-Job (`/etc/cron.d/gamma-backup`, Pfad ans Checkout anpassen):

```
30 3 * * * root cd /opt/gamma && ./ops/pg-backup.sh >> /var/log/gamma-backup.log 2>&1
```

**Off-VM-Kopie (Pflicht, sonst ist es kein Backup):** Ein Dump auf derselben
Platte überlebt den VM-Verlust nicht. Zwei einfache Wege, mindestens einer:

- Täglicher Provider-Snapshot der VM (z. B. Hetzner Snapshots) — deckt
  zugleich `miniodata` (Medien) ab.
- Pull von außerhalb (zweiter Rechner/Storage-Box):
  `rsync -az vm:/var/backups/gamma/ ./gamma-backups/`

**Restore** — `ops/pg-restore.sh` stoppt die App-Services, setzt das Schema
zurück (`DROP SCHEMA public CASCADE` — bewusst statt `pg_restore --clean`,
das nur Objekte abräumt, die im Dump existieren: nach einem schlechten Deploy
kann das Live-Schema NEUER sein als der Dump, und Überbleibsel würden
core-apis Migrations-Re-Run mit „relation already exists" crashen), spielt
den Dump ein und startet genau die Services wieder, die vorher liefen. Danach
entspricht die DB exakt dem Dump; ist der Code inzwischen neuer, wendet
core-api die fehlenden forward-only Migrationen beim Start selbst an. Ohne
`GAMMA_CONFIRM_RESTORE=yes` bricht das Skript ab; es funktioniert identisch
gegen ein frisches leeres Volume (neue VM, erst §3 „Stack starten").

```sh
GAMMA_CONFIRM_RESTORE=yes ./ops/pg-restore.sh /var/backups/gamma/gamma-<stamp>.dump
curl -s localhost:8080/health && curl -s localhost:8080/ready
```

Zwei bewusste Nebenwirkungen: Alle **Sessions werden invalidiert** — ein Dump
würde sonst seither ausgeloggte oder widerrufene Tokens reanimieren; alle
melden sich neu an, der Ingestion-Worker heilt sich selbst (§3). Und
**Operator-Aktionen nach dem Dump-Zeitpunkt** (Takedowns, Verifizierungen,
Referral-Overrides) sind verloren und müssen nachgezogen werden.

**Geprobter Drill (M4.5, 2026-07-06, lokal gegen `compose.prod.yml`), beide
Pfade:** *(a) Bad Deploy* — nach dem Dump Schema-Drift erzeugt (zusätzliche
Tabelle wie von einer neueren Migration, zusätzlicher Post), Restore per
Skript: DB exakt == Dump, Drift weg, alter Token 401, core-api startet ohne
Crash-Loop, ein bewusst gestoppter Scheduler blieb gestoppt. *(b)
Totalverlust* — `pgdata`-Volume gelöscht, Stack neu hochgefahren, Restore per
Skript: Zeilenzahlen und Marker-Post identisch, `/health` + `/ready` 200,
alle vorher laufenden Services wieder da (auch ein Worker mitten im
Restart-Backoff). Reproduzierbar mit denselben zwei Skripten, die auch die
VM benutzt.

## 8. Beobachten

- `docker compose -f compose.prod.yml logs -f core-api` (strukturierte Logs
  mit `x-request-id`).
- Health: `/health`, `/ready` (core-api, über `api.example.com` auch extern);
  Ingestion-`/healthz` prüft Docker selbst (Container-Status `healthy`).
- Metriken: `GET /metrics` (Prometheus-Format) — für die Beta reicht ein
  Uptime-Ping auf `https://api.example.com/health`; mehr Monitoring ist eine
  §8-Entscheidung im MASTERPLAN.
- Ingestion-Betrieb (DLQ, Backfill, Modell-Swap): `services/ingestion/RUNBOOK.md`.

## 9. Offen in M4

- GHCR-Publish-Job (M4.6), Load-Smoke (M4.7), Go/No-Go-Checkliste (M4.8).
