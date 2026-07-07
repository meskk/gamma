# Gamma — Masterplan Phase 1a

> **Single source of truth für die Roadmap.** CLAUDE.md verweist hierher und wird nur an
> Meilenstein-Grenzen aktualisiert. Jede Session (Mensch oder KI) misst ihre Arbeit an
> diesem Dokument. Erstellt 2026-07-05 aus einem Full-Repo-Review (3 parallele
> Explorationen + 3 unabhängige Architektur-Entwürfe), vom Owner genehmigt.

## 1. Zweck & Arbeitsvertrag

**Warum dieses Dokument existiert:** Bisher wurde iterativ gebaut und Qualitätsmängel
fielen erst im Nachhinein auf (Beispiel: der Login-„Time-Limiter"). Die Antwort ist keine
pauschale Neuentwicklung, sondern eine **Soll-Architektur als Messlatte**: Jeder
bestehende Teil bekommt ein Urteil (**BEHALTEN / VERBESSERN / NEU**); neu geschrieben
wird nur, was der Soll-Architektur strukturell widerspricht.

**Arbeitsvertrag (gilt für jede Session):**

1. **Ein Schritt = ein Commit.** Ein reviewbares Diff, das eine Frage beantwortet.
   Ein Schritt beginnt erst, wenn der vorherige reviewed ist.
2. **Gates grün VOR dem Commit** (je nach berührtem Bereich, siehe §7).
3. **CI grün nach dem Push** — „fix CI"-Folge-Commits sind nicht die Norm.
4. Bindings (`backend/bindings/*.ts`), Migrations (forward-only) und `.sqlx`-Cache
   synchron im selben Commit.
5. Keine Ökonomie-Konstante außerhalb `econ-params`; keine Floats auf konservierten
   Größen.
6. Architektur-Entscheidung ⇒ ADR im selben Schritt.
7. Jeder abgeschlossene Schritt bekommt eine Zeile im Step-Ledger (§4);
   die Commit-Message nennt den Schritt (z. B. `M2.4: …`).

## 2. Scope & Non-Goals

**Scope: Phase 1a fertigstellen** — Off-Chain-Social-Produkt, Gems als Punkte, plus
Ops-Readiness für eine kleine 1a-β (gedeckelte Fiat-Auszahlung).

**Non-Goals (ausdrücklich AUSSEN VOR, egal wie naheliegend):**
- Alle Phase-1b-Krypto-Themen: USDC-Reserve, PEER-Mint/Redemption, Solana,
  Advertiser-Sweep. Aktuelle Strategie ist Fiat-Ad-Revenue-Share ohne Token
  (tradbarer Coin nur falls/nachdem lizenziert).
- Kubernetes/IaC — 1a-β läuft auf einer VM mit docker compose.
- Feed-Ranking aus `content_signals` vor ADR 0009 (Signal-Schema).

**Phase-1a-Exitkriterien:** werden in M1.5 (Scope-Freeze) finalisiert. Bis dahin gilt:
M0–M4 abgeschlossen = Phase 1a fertig.

**1b-Eingangstore (nicht vergessen, nicht jetzt bauen):** HttpOnly-Cookie+CSRF statt
sessionStorage-Token; Secrets-Manager; ggf. ADR-0008-Auflösung falls M1 sie auf 1b
verschiebt. *(Der Ingestion-Service-Account wurde per Owner-Entscheidung auf M2.8
vorgezogen.)*

## 3. Meilensteine

Details (Design-Begründungen, Policies, Testlisten) stehen in §6-Referenzen und den ADRs;
hier steht, WAS in welcher Reihenfolge fertig wird.

### M0 — Baseline & Leitplanken — ✅ ABGESCHLOSSEN 2026-07-05
1. ✅ `devIndicators`-Änderung committen.
2. ✅ `review/audit-fixes` → `main` gemergt (Merge-Commit), CI grün, Tag `phase1a-baseline`.
3. ✅ Dieses Dokument committet; CLAUDE.md-Verweis.
4. ✅ CI: alle Actions auf SHA gepinnt, MinIO-Image auf Digest.
5. ✅ CI: Security-Scans blockierend — `cargo deny` (+ `deny.toml`), `pip-audit`,
   `npm audit --omit=dev --audit-level=high`. (Report-Modus-Runde übersprungen:
   Triage erfolgte lokal, die Scans landeten direkt grün.)
6. ✅ Wöchentlicher `schedule:`-Lauf (Mo 06:00 UTC) des Security-Workflows.
7. ✅ Branch-Protection auf `main`: alle 7 Job-Checks Pflicht (`enforce_admins`
   bewusst aus, damit der Ein-Schritt-Direct-Push-Workflow des Owners funktioniert).

**Done-Kriterium erfüllt:** `main` == Branch-Tip, Tag gesetzt, keine mutable
Action-Refs, Security-Jobs blockierend und auf Anhieb grün (Run 28745802549).

### M0.5 — Auth-Härtungs-Cluster — ✅ ABGESCHLOSSEN 2026-07-05
(E2E im Browser verifiziert: 5 Fehlversuche sperren, richtiges Passwort bekommt
429 + tickenden Countdown, nach Ablauf Login 200. Bonus-Fund in A4: der alte
Edge-Limiter hatte einen Einheiten-Bug — `per_second(10)` war eine PERIODE,
also 1 Request pro 10 s statt 10 r/s; gefixt.)
Nicht produkt-facing, keine Abhängigkeiten — wartet auf nichts. Sieben Commits:

Backend (Soll-Design: zwei unabhängige Schutzschichten — Per-Route-IP-Limit +
Per-Account-Backoff; Details §6.1):
- **A1** `ApiError::TooManyRequests { retry_after_secs }` → 429 + `Retry-After` +
  Body `{"error":"rate_limited"}` (XS).
- **A2** Migration `0017_login_throttle.sql` (`login_throttle(email PK, failed_count,
  last_failed_at, locked_until)`) + Repo-Methoden + pure Policy `auth/throttle.rs`
  (Fehlversuche 1–4 frei; ab 5.: 60 s, verdoppelnd, Cap 15 min; Reset bei Erfolg;
  Sweep nach 24 h) (S).
- **A3** Backoff in `AuthService::login` verdrahten — Schlüssel = normalisierte E-Mail
  (kein Enumeration-Orakel), Lock-Check vor argon2, atomarer ON-CONFLICT-Zähler.
  **Damit ist das Login-Problem behoben.** (S)
- **A4** `RateLimitConfig` in `app()`; enger Governor-Bucket auf `/v1/auth/*`
  (Default burst 5, 1 Req/2 s, `GAMMA_RATE_LIMIT_AUTH_*`); globaler Edge-Backstop
  bleibt, Default angehoben; gemeinsamer IP-Extractor-Helper; Governor-429 im
  ApiError-JSON-Format (M).
- **A5** Housekeeping-Task im core-api-Binary: stündlich Session-Purge +
  Throttle-Sweep; Knob `GAMMA_SESSION_TTL_DAYS` (S).

Frontend:
- **FA1** `ApiError.retryAfter` aus dem `Retry-After`-Header (S).
- **FA2** Login-Cooldown-UI: Countdown im Button („Warte 27 s…"), statischer
  Fehlertext im `role="alert"`, Cooldown überlebt Tab-Wechsel; `flowGen` unangetastet (S).

**Done-Kriterium:** Backend-Test weist 429 nach N Fehlversuchen nach (auch für
unbekannte E-Mails, identische Sequenz); Frontend-Test weist Cooldown-UI nach.

### M1 — Produkt-Entscheidungs-Checkpoint (NICHTS Produkt-Facing vorher)
1. Owner zählt alle gewünschten Produktänderungen auf → §5 Produkt-Backlog
   (je: Beschreibung, Akzeptanzkriterien, in/out-1a).
2./3. Soll-Architekturen (§6) bestätigen; nötige ADRs schreiben.
4. Offene Fragen (§8) beantworten; dossier-gebundene Punkte explizit geparkt.
5. **Scope-Freeze:** Phase-1a-Exitkriterien final in §2.

### M2 — Ingestion & echtes Modell (höchstes technisches Risiko; direkt nach M1)
1. `GET /v1/posts/:id/signals`-Lesepfad (P12) + Binding + Tests.
2. Feed-Boundary-Notiz (P13, Docs).
3. **ADR 0009:** versioniertes Signal-Schema (nutzt M1-Antworten; strikt vor Schritt 7).
4. Modell-Analyzer hinter dem `Analyzer`-Seam (der EINE Stub in
   `services/ingestion/src/gamma_ingestion/analyzer.py`); CI bleibt hardwarefrei.
5. GPU-Bring-up (EU-Cloud-Miete), `GAMMA_ANALYZER=model`, RUNBOOK-Deltas.
6. Korpus-Backfill über Admin-Endpoints; DLQ beobachten.
7. *(M1 hat ja gesagt)* Feed-Ranker liest `content_signals` — Config-Flag,
   nie Payout-relevant.
8. Service-Account für den AI-Block: eigene, minimal berechtigte Identität statt
   geteilter Operator-Credentials (vorgezogen aus den 1b-Gates — Konsequenz der
   Owner-Entscheidung „AI ist ein eigenständiger, sicher getrennter Block").

**Done-Kriterium:** kein `NotImplementedError` mehr; echte Signale für den
Bestandskorpus; Lesepfad typisiert in `bindings/`.

### M3 — Restliche Härtung + Produktänderungen
Backend-Sequenz **B1→C2** (§6.1): Feed-Cursor-Pagination (`FeedPage`), System-Account-Fix
(Migration 0018 + FKs), Sybil-/Bot-Gate-Proptests, Golden-Vector-Snapshot,
Scale-Smoke (`#[ignore]`).
Frontend-Sequenz **B1→F** (§6.2): `useFetch` + Migrationen, `useLike`/`useUnlock`,
`usePagedFeed` + ReelsFeed-Paging, Compose-/Admin-Tests, Styling (CSS Modules) +
Deutsch-Vereinheitlichung zuletzt.
Danach: Produkt-Items aus §5; HLS-Ladder nur falls M1 sie in 1a behält.

**Done-Kriterium:** alle §6-1a-Punkte geschlossen; jedes §5-Item geliefert
(Akzeptanzkriterien) oder formal re-deferred.

### M4 — Ops-Readiness für Phase 1a-β (bewusst klein; kein k8s) — ✅ ABGESCHLOSSEN 2026-07-07
*(Artefakt-seitig komplett, M4.1–M4.8, jede Zeile lokal gedrillt. Das
Real-VM-Bein des Done-Kriteriums — „frische EU-VM von Null auf TLS" — läuft
bewusst als Go/No-Go-Gate C1 weiter, weil es die offene §8-Entscheidung
Domain/Provider voraussetzt.)*
1. Ingestion `/healthz` (P9b) + Compose-Healthcheck.
2. Backend-Dockerfile (Multi-Stage, `SQLX_OFFLINE=true`, non-root; ein Image,
   Binary per Command) + CI-Build-Job.
3. `compose.prod.yml` (digest-gepinnt, Restart-Policies, Healthchecks,
   `.env.prod` uncommitted + Example committed).
4. `docs/OPERATIONS.md`: Ein-VM-Story, Caddy/nginx-TLS, Firewall (nur 80/443),
   Deploy = `compose pull && up -d`, Rollback = voriger Digest.
5. Backup/Restore: nächtlicher `pg_dump` + einmal geprobter Restore-Drill.
6. CI-Publish-Job (GHCR, git-SHA-Tags, `workflow_dispatch`); Deploy bleibt manuell.
7. Load-Smoke-Skript (Feed-Read + Unlock-Pfad) mit dokumentierten Schwellen.
8. Go/No-Go-Checkliste 1a-β + finale CLAUDE.md-Auffrischung.

**Done-Kriterium:** frische EU-VM kommt allein mit OPERATIONS.md von Null auf
„läuft über TLS"; Restore-Drill einmal erfolgreich.

## 4. Step-Ledger (append-only)

| Schritt | Datum | Commit | Gates | Ergebnis |
|---|---|---|---|---|
| M0.1 | 2026-07-05 | a1a4a0b | FE: typecheck+lint+test+build ✓ | devIndicators committet |
| M0.2 | 2026-07-05 | d7e9a55 | CI auf main | Merge + Tag `phase1a-baseline` |
| M0.3 | 2026-07-05 | d848de3 | Docs | MASTERPLAN.md angelegt |
| M0.4 | 2026-07-05 | 7e3d086 | CI auf main ✓ (Run 28745581000) | Actions auf SHA, MinIO auf Digest gepinnt |
| M0.5a | 2026-07-05 | ef88de1 | fmt+clippy+test --all ✓ | anyhow-Advisory gefixt, aws-sdk-s3 Bump |
| M0.5b | 2026-07-05 | f22715a | lokal: cargo deny ✓, pip-audit ✓, npm audit(high) ✓; CI Security-Run 28745802549 ✓ | Security-Workflow blockierend; Triage: 3 webpki-Waiver (Legacy-Pfad, ungenutzt), npm 2× moderate (postcss via Next, unter High-Schwelle) |
| M0.6 | 2026-07-05 | 5644018 | CI + Security auf main ✓ | Wöchentlicher Security-Schedule (Mo 06:00 UTC) |
| M0.7 | 2026-07-05 | *(GitHub-Setting, kein Commit)* | gh api verifiziert | Branch-Protection: 7 Pflicht-Checks auf main; enforce_admins aus |
| M0 ✅ | 2026-07-05 | e8e7dfd | CI ✓ + Security ✓ (Runs 28745802538/28745802549) | Meilenstein abgeschlossen |
| M1.1+M1.4 (teilw.) | 2026-07-05 | 8ec3636 | Docs | Owner-Antworten eingearbeitet: Backlog P-1..P-3, AI-Block-Prinzip + Payout-Grenze, 10k-Ziel, HLS verschoben |
| M1.1+M1.4 (Forts.) | 2026-07-05 | b941601 | Docs | P-1-Matrix entschieden; P-2-Parameter (3 %/6 Mon., Creator-Overrides); P-4 Private Area (non-custodial) neu; AI-Vorschlagswesen präzisiert |
| M1.1+M1.4 (Forts. 2) | 2026-07-05 | e5f0111 | Docs | P-5 Finance-Area + YouTube-Earnings-Modell |
| M1.1+M1.4 (Forts. 3) | 2026-07-05 | 0d2ebf7 | Docs | P-5-Rechtskonflikt explizit (gespeicherte Balance vs. 2026-07-01-Struktur) + Versöhnungsvorschlag |
| A1 | 2026-07-05 | cadd5b2 | fmt+clippy+test --all ✓ | ApiError::TooManyRequests, 429 + Retry-After |
| A2 | 2026-07-05 | a055210 | fmt+clippy+test --all ✓, sqlx-Cache regeneriert | Migration 0017, pure Throttle-Policy, Repo-Methoden |
| A3 | 2026-07-05 | b27fa18 | fmt+clippy+test --all ✓ (4 neue Integrationstests) | Backoff im Login verdrahtet — Kernproblem behoben |
| A4 | 2026-07-05 | f6a4c3e | fmt+clippy+test --all ✓ (3 neue Tests) | Per-Route-Governor auf /v1/auth/*; Edge-Backstop-Einheiten-Bug gefixt (per_second war Periode) |
| A5 | 2026-07-05 | 0dd2342 | fmt+clippy+test --all ✓ (2 neue Tests) | Housekeeping-Task (Session-Purge + Throttle-Sweep), GAMMA_SESSION_TTL_DAYS |
| FA1 | 2026-07-05 | 5ce9800 | FE-Gates ✓ (3 neue Tests) | ApiError.retryAfter (Delta/HTTP-Datum, Cap 15 min) |
| FA2 | 2026-07-05 | 3cb0dbb | FE-Gates ✓ (2 neue Tests) | Login-Cooldown-UI; E2E im Browser verifiziert (5 Fehlversuche → 429 → Countdown → Ablauf → Login 200) |
| M0.5 ✅ | 2026-07-05 | 61cbd8c | Alle Gates + E2E-Browser-Beweis; CI+Security remote ✓ | Auth-Härtungs-Cluster abgeschlossen |
| M2.1 ✓(vorhanden) | 2026-07-05 | — | Befund | GET /posts/:id/signals existiert bereits (operator-only — vor ADR 0009 korrekt); kein Commit nötig |
| P-1 | 2026-07-05 | 2c99498 | FE-Gates ✓ (4 neue Tests) | Launch-Ausblendungen: Tip/Save/Gem-Preis hinter NEXT_PUBLIC_FEATURE_*-Flags |
| P-2/R1 | 2026-07-05 | 0725671 | fmt+clippy+test --all ✓ | Migration 0018 (referral_code, referrals, referral_terms, Ledger-Kind) + econ-params v2 (300 bps / 183 Epochen) |
| P-2/R2 | 2026-07-05 | 97585de | fmt+clippy+test --all ✓, Bindings + FE-Typecheck ✓ (4 neue Tests) | Registrierung nimmt Codes an (Terms-Snapshot, 400 bei Tippfehler); /auth/me liefert eigenen Code |
| P-2/R3 | 2026-07-05 | f9973c8 | fmt+clippy+test --all ✓ (4 Unit- + 2 Integrationstests) | Konservierender Settlement-Cut (pure, eine Ebene, Floor; nur verifizierte Referrer; 'referral'-Journal; doppelter Fail-closed-Check) |
| P-2/R4 | 2026-07-05 | b10b614 | fmt+clippy+test --all ✓, Bindings ✓ (1 neuer Test) | PUT /users/:id/referral-terms (operator-only, Upsert, Audit-Log) |
| P-2/R5 | 2026-07-05 | 69c8efc | FE-Gates ✓ (2 neue Tests) | Einladungslink /login?ref=CODE → Registrierung; ungültiger Code wird nach Fehler verworfen |
| P-2 ✅ | 2026-07-05 | be853d5 | E2E im Browser: /login?ref=… → Registrierung → DB-Zeile 300 bps/183 Epochen ✓ | Referral-System komplett (Anzeige des eigenen Links folgt mit der Finance-Area, P-5) |
| M2.8 | 2026-07-05 | c68a119 | fmt+clippy+test --all ✓, FE-Typecheck ✓ (1 neuer Test) | Service-Rolle: Signals-Write unter Maschinen-Identität; keine Operator-Rechte; RUNBOOK-Provisionierung |
| B1 | 2026-07-05 | c3be0a9 | fmt+clippy+test --all ✓ (2 Unit- + 2 neue Integrationstests) | Feed-Cursor: eingefrorene Ranking-Uhr + Keyset, FeedPage-Binding, invalid/stale → 400 |
| D1–D3 | 2026-07-05 | 4dbe7c2 | FE-Gates ✓ (5 neue Tests) | usePagedFeed (20er-Seiten, Legacy-Fallback, Dedupe) + ReelsFeed-Prefetch ab 3 Slides vor Ende |
| B1+D ✅ | 2026-07-05 | b42962c | E2E im Browser: Seite 1 (20 Slides) → Blättern → Cursor-Request → 25 Slides ✓ | Feed-Paging komplett (Backend + Frontend) |
| FE-B1 | 2026-07-05 | fa0eff6 | FE-Gates ✓ (5 neue Tests) | lib/useFetch — der EINE Fetch-Hook (Stale-Guard, reload, enabled) |
| FE-B2 | 2026-07-05 | b4ac368 | FE-Gates ✓ (6 neue Tests) | Admin-Seiten migriert (reports bekam den fehlenden Stale-Guard); Operator-Guard-Tests |
| FE-B3 | 2026-07-05 | 1f7be85 | FE-Gates ✓ | Comments migriert |
| FE-B4 | 2026-07-05 | 9b9725f | FE-Gates ✓ (Regressionstests unverändert grün) | posts/[id] + users/[id] migriert; Follow-Toggle = lokales Override über Server-Wahrheit |
| C1+C2 | 2026-07-05 | 0b8bfab | FE-Gates ✓ (6 neue Tests) | useLike + useUnlock dedupliziert; Unlock-Fehler bleibt retry-bar; Vitest-Hook-Falle (returned mock = Teardown) gefixt |
| FE-Block ✅ | 2026-07-05 | d7c040d | 55 FE-Tests gesamt | Frontend-Vereinheitlichung abgeschlossen — kein handgerollter Stale-Guard mehr im Code |
| F (Copy) | 2026-07-05 | da8cec9 | FE-Gates ✓; Browser-Check: Profil/Post/Nav deutsch ✓ | Deutsch-Vereinheitlichung aller User-Seiten (@user-Handles einheitlich; /admin bleibt englisch). CSS-Module-/Token-Konvergenz bleibt opportunistisch offen |
| M4.1 | 2026-07-06 | 02f7ef3 | ruff+mypy+pytest ✓ (69 Tests); Ingestion-Image gebaut ✓ | Ingestion /healthz (GAMMA_HEALTH_PORT, Default 8081) + Dockerfile-HEALTHCHECK; RUNBOOK §7 bereinigt |
| M4.2 | 2026-07-06 | 191dc14 | Lokal: Image gebaut (754 MB), 3 Binaries ✓, core-api antwortet in-Container auf /health ✓; CI backend-docker ✓ + als 8. Pflicht-Check | Backend-Dockerfile (ein Image, drei Binaries) + backend-docker-CI-Job. Hinweis: Colima-VM dafür von 2 auf 6 GiB vergrößert (Release-Build-OOM) |
| M4.3+M4.4 | 2026-07-06 | 860ab1e | Abnahme-Drill lokal: 7 Services healthy, /health+/ready 200, Service-Account-Provisionierung + Worker-Selbstheilung durchgespielt | compose.prod.yml (digest-gepinnt, keine öffentlichen DB-Ports, .env.prod.example) + OPERATIONS.md (Ein-VM-Story, Caddy, Firewall, Deploy/Rollback). Fund dokumentiert: Worker-Restart-Schleife triggert die eigene Login-Bremse — Ausweg in §3 |
| M4.5 | 2026-07-06 | aba1e21 | shellcheck ✓; Multi-Agent-Review des Diffs (8 bestätigte Findings, alle gefixt — größter: pg_restore --clean crasht core-api, wenn das Live-Schema neuer ist als der Dump); Drill lokal gegen compose.prod.yml in BEIDEN Pfaden: Bad-Deploy mit Schema-Drift (DB==Dump, kein Crash-Loop, alter Token 401, gestoppter Scheduler bleibt gestoppt) + Totalverlust (Volume weg → Zählungen+Marker identisch, /health+/ready 200); Rotations-Test mit gealterten Dumps ✓ | ops/pg-backup.sh (umask 077, Archiv-Validierung, trap-Cleanup, Prune nur top-level + nie unter den letzten Dump) + ops/pg-restore.sh (Schema-Reset statt --clean, Session-Invalidierung, startet nur vorher laufende Services, GAMMA_CONFIRM_RESTORE-Pflicht) + OPERATIONS.md §7 (Cron, Off-VM-Kopie als Pflicht, Drill-Protokoll) |
| M4.6 | 2026-07-06 | *(dieser Commit)* | actionlint + shellcheck ✓; compose-Interpolation beide Modi verifiziert (leer → :selfbuilt, gesetzt → :SHA); Selfbuild-Fallback-Smoke lokal (up -d --build → healthy); Multi-Agent-Review (6 bestätigte Findings, alle gefixt — u. a.: nacktes `up -d` nach fehlgeschlagenem Pull baut STILL das Checkout unter dem SHA-Namen, live nachgewiesen → überall `pull && up -d --no-build`; Tag-Immutabilität war Behauptung → jetzt per manifest-inspect-Skip erzwungen); Abnahme nach Push: Publish-Dispatch → beide Images in GHCR, SHA-getaggt | .github/workflows/publish.yml (workflow_dispatch, GHCR, git-SHA-Tags, Smoke vor Push, Skip-statt-Überschreiben, concurrency pro SHA, Registry-Digests in der Run-Summary) + compose.prod.yml image+build-Hybrid (GAMMA_IMAGE_TAG via .env-Symlink) + ops/pg-restore.sh `--no-build` + OPERATIONS §2/§3/§6 Pull-Deploy-Story (Rollback = voriger SHA; Migrations-Grenze fail-closed dokumentiert). Frontend-Container-Image bewusst separat (NEXT_PUBLIC-Bake-Entscheidung, OPERATIONS §10 *(Verweis korrigiert in M4.7 — §9 wurde durch die Renummerierung zum Load-Smoke)*) |
| M4.7 | 2026-07-07 | 2bd064f | ruff + py_compile ✓; Multi-Agent-Review (6 bestätigte Findings, alle gefixt — Blocker: settlement-scheduler settlet die Zeitmaschinen-Epoche leer, bevor der Smoke sie füllt → per Profil aus dem Smoke-Stack; dazu Rate-Schwelle ≥ 95 % gegen Closed-Loop-Maskierung, per Simulation belegt); Re-Drill gegen frischen Stack ohne Scheduler: PASS — 100,5 req/s über 60 s, 0 Fehler, Feed p95 3 ms, 150 parallele Unlocks p95 32 ms, Idempotenz sauber, Konservierung exakt | ops/load-smoke.py (stdlib-Python, echter API-Pfad inkl. presigned Upload + finalize; Zeitmaschine, weil aktuelle Epoche by design nicht settlebar; Schwellen: Fehler 0, Rate ≥ 95 %, Feed p95 ≤ 300 ms, Unlock p95 ≤ 500 ms, Geld exakt; keine Tick-Nachholung) + ops/compose.smoke.yml (MinIO auf 127.0.0.1:9100; Scheduler ausgeschlossen) + OPERATIONS §9 (Lastmodell 10k → 100 req/s, Laptop/VM-Varianten nach §3-Regel, Referenzwerte; Offen-Liste → §10; M4.6-Zeilen-Verweis §9→§10 korrigiert) |
| Sec-Triage | 2026-07-07 | 7a7a1c1 | fmt+clippy+test --all ✓ (37 Suites); cargo deny advisories lokal ✓; CI+Security remote ✓ | RUSTSEC-2026-0204 (crossbeam-epoch 0.9.18, fmt::Pointer-Deref; via metrics-exporter-prometheus): Lock-Bump auf 0.9.20 — Advisory landete upstream nach dem gestrigen grünen Scan, der M4.7-Push hat sie nur zuerst gesehen |
| M4.8 | 2026-07-07 | *(dieser Commit)* | Docs; Multi-Agent-Review des Diffs (9 bestätigte Findings, 12 widerlegt — alle 9 eingearbeitet; größter: die Checkliste gate-te nicht, WIE aus Punkte-Gems ein €-Betrag wird → neue A-Box Gems→€-Basis + payout-Journal-Kind + C12 Test-Payout-Probe; dazu DSGVO-Lösch-Prozess-Gate, C7-Volume-Falle, Konto-Sperre/Passwort-Reset ehrlich in D) | docs/GO-NO-GO-1a-beta.md (Owner-Entscheidungen A, Rechts-Gates B, VM-Abnahme C1–C12, akzeptierte Beta-Risiken D; neu festgehaltener Blocker C3: presigned URLs des gebündelten MinIO zeigen auf compose-internes minio:9000 — öffentlicher Media-Endpoint nötig, Fund aus M4.7) + CLAUDE.md-Snapshot auf die M4-Grenze verdichtet (P4 korrekt als offen geführt) + M4-Header ✅ (Real-VM-Bein → Go/No-Go C1) + Ops-Index verlinkt. **M4 damit artefakt-seitig abgeschlossen.** |

## 5. Produkt-Backlog (gefüllt in M1.1 durch den Owner; Stand 2026-07-05)

### P-1 — Launch-Funktionsumfang *(ENTSCHIEDEN 2026-07-05)*
| Feature | Launch | Anmerkung |
|---|---|---|
| Posts, Kommentare, Likes, Follows | sichtbar | Kern; füttert die Payout-Formel |
| Feed (For-you + Folge ich) | sichtbar | |
| Compose + Medien-Upload | sichtbar | Bild/Video/Audio |
| Profil-Seiten, Gems-Kontostand | sichtbar | |
| Referral-Link | sichtbar | sobald P-2 gebaut |
| Melden (Report) | sichtbar | |
| Tip-Button (Reel-Leiste) | **versteckt** | heute nur Attrappe; eigenes Item, wenn gebaut |
| Save-Button (Reel-Leiste) | **versteckt** | speichert nur lokal — nichts vortäuschen |
| Gem-Paid-Unlock (Bestand) | **versteckt** | Launch-Modell für bezahlten Content ist P-4 (Private Area), nicht der Gem-Unlock; Code bleibt (Config-Ausblendung, kein Löschen) |
| Admin-Bereich | operator-only | fix |

**Akzeptanz:** Frontend blendet die drei versteckten Features per Config aus
(kein Code-Löschen); ein Test pro Ausblendung.

### P-2 — Referral-System *(✅ GEBAUT 2026-07-05; Link-Anzeige folgt mit P-5)*
User werben User per Referral-Link; der Referrer erhält einen Anteil (Cut) an den
Gem-Erträgen der Geworbenen. Design-Leitplanken (aus der Architektur zwingend):
- Cut-Höhe als **econ-params-Knob** (`referral_bps`), niemals hardcoded.
- **Konservierend:** Der Cut kommt AUS dem Payout des Geworbenen, es wird nichts
  zusätzlich gemintet — Invariante i (Σ payouts == emission) bleibt exakt.
- **Anti-Abuse:** Referral-Erträge zählen nur aus Nutzern hinter dem Bot-Gate
  (`v_i = true`) — sonst ist Referral + Bots ein direkter Harvest-Vektor.
- Empfehlung: nur EINE Referral-Ebene (mehrstufig = Pyramiden-Optik + Abuse-Fläche).
**ENTSCHIEDEN (2026-07-05):** Default **3 % für 6 Monate** (≈183 Tages-Epochen),
beides als econ-params-Knöpfe (`referral_bps_default`, `referral_duration_epochs`).
Zusätzlich **Creator-Verträge**: pro Nutzer überschreibbare Konditionen (z. B. 5 %
mit frei definierter Laufzeit), gesetzt über einen operator-only Endpoint
(`referral_terms`-Override: user_id, bps, gültig_bis). Eine Ebene.
**Akzeptanz:** Registrierung nimmt Referral-Code an; Settlement bucht den Cut als
eigene `ledger_entries`-Art (`referral`); Override-Endpoint operator-only mit
Audit-Spur; Summen bleiben exakt (Hamilton); Tests decken Konservierung + Gate +
Override-Ablauf ab.

### P-4 — Private Area: non-custodial Creator-Marktplatz *(Scoping offen)*
Owner-Vision (2026-07-05): Die Plattform ist im Vordergrund wie Instagram (öffentliche
Posts, Feed). Auf jedem Profil gibt es einen **vierten Reiter „Private Area"**: Der
Nutzer entscheidet, ob dieser Bereich öffentlich ist oder **nur gegen Bezahlung**
sichtbar, und bepreist seinen Content selbst. Kauf läuft **non-custodial** — die
Plattform ist reiner Mittelmann und hält NIE Kundengelder (bewusste Konsequenz der
fehlenden Verwahrlizenz): entweder (a) Fiat über Drittanbieter (z. B. Stripe Connect,
Direct Charge + Application Fee = unser Cut) oder (b) Wallet-zu-Wallet per Smart
Contract mit Protokoll-Cut.
**Einordnung:** Das konkretisiert Rail 1 aus dem Dossier (Creator-Marktplatz) und
ERSETZT den Gem-Paid-Unlock als Launch-Modell für bezahlten Content (P-1: Bestand
wird versteckt, nicht gelöscht).
**Empfohlene Sequenz:** Stripe-Pfad zuerst (juristisch + technisch der kürzeste Weg,
Cut sauber als Application Fee); Wallet-/Smart-Contract-Pfad als zweite Ausbaustufe
(Chain-Wahl, Wallet-Connect, Contract-Audit — eigenes Gate).
**Offen (Owner):** Ist die Private Area **launch-blockierend** (1a) oder kommt sie
mit 1a-β? Cut-Höhe (%)? Nur Abo-artiger Zugang zum ganzen Bereich oder auch
Einzel-Content-Kauf? **Rechts-Check** (Plattformhaftung, Steuern, Adult-Content-Policy)
vor dem Bau — passt zu den dokumentierten Anwalt-Next-Steps der Monetarisierungsstrategie.

### P-5 — Finance-/Wallet-Bereich + YouTube-Earnings-Modell *(Scoping mit P-4)*
Owner-Vision (2026-07-05): **Es werden überhaupt keine Coins distribuiert** (rechtliche
Leitplanke, bestätigt). Stattdessen agiert die Plattform **wie YouTube**: Nutzer sehen
eine **Balance, die die Plattform ihnen schuldet** (Verbindlichkeit, kein Token).
Auszahlung erst **ab einer Schwelle**, in Fiat oder Krypto, über die non-custodialen
Pfade aus P-3/P-4. Mit der Balance können Nutzer außerdem in der Plattform
**Super-Posts / Super-Likes** machen (Boosts).
Bestandteile:
- **Finance-Area** in der App: Balance-Anzeige, Transaktionshistorie (direkt aus dem
  vorhandenen `ledger_entries`-Journal), Auszahlungsantrag ab Schwelle
  (`payout_threshold` als econ-params-Knob), Wallet-Connect (Phantom/Solana) als
  Verknüpfung fürs Krypto-Auszahlungsziel.
- **Super-Post/Super-Like:** mappt auf den VORHANDENEN konkaven Burn-Multiplier der
  Gewichtsformel (Einsatz erhöht Gewicht/Sichtbarkeit, konkav gegen Pay-to-win);
  Knöpfe existieren in `econ-params` (`burn_scale`, κ) — das Feature ist die
  UI + Ledger-Buchung dazu, keine neue Ökonomie.
- **Gems ↔ Balance:** Wie die 1a-Punkte-Gems in die geschuldete Balance übergehen
  (Umrechnung bei 1a-β, Anzeige beider Größen?), definiert ein eigener ADR mit dem
  Dossier — offen.
**Rechts-Check (Pflicht vor Bau — hier liegt ein bekannter Konflikt):** Die am
2026-07-01 geschärfte Rechtsstruktur war ausdrücklich „KEINE gespeicherte Balance"
(Pass-Through, direkte Auszahlung), um E-Geld/ZAG zu vermeiden. Das P-5-Modell
(geschuldete Schwellen-Balance, in-platform ausgebbar) weicht davon ab:
- **Angesammelte Schwellen-Balance, NUR auszahlbar** = wie YouTube/AdSense real
  arbeitet; vertretbar als Handelsverbindlichkeit — vom Anwalt bestätigen lassen
  (ist Frage 1 des fertigen Anwalts-Briefs).
- **Dieselbe Balance in-platform AUSGEBEN** (Super-Post/Like) rückt sie Richtung
  gespeicherter Wert / E-Geld — der riskante Teil.
- **Vorgeschlagene Struktur, die beides liefert:** Die €-Verbindlichkeit bleibt
  strikt „nur auszahlbar"; Boosts werden aus den PUNKTE-Gems bezahlt (Burn), nicht
  aus der €-Balance. Nutzererlebnis bleibt („verdienen + boosten"), die €-Schiene
  bleibt sauber. Owner + Anwalt entscheiden.
**Offen (Owner):** Schwellenhöhe; Boosts aus €-Balance oder aus Punkte-Gems
(Empfehlung: Gems); startet der Finance-Bereich mit 1a-β (empfohlen) oder früher
als reine Anzeige?

### P-3 — Payout-Rail über Drittanbieter *(1a-β)*
Echte Auszahlungen (gedeckelt) laufen zunächst über einen Drittanbieter — KYC liegt
beim Anbieter; eigenes KYC wird später evaluiert (passt zur hinterlegten
Fiat-Revenue-Share-Strategie). **Offen (Owner):** Anbieterwahl (z. B. Stripe Connect /
PayPal Payouts / Wise), Länderabdeckung, Gebühren. **Akzeptanz:** dokumentierter
(halb-)manueller Payout-Prozess für die Beta in OPERATIONS.md.

## 6. Soll-Architektur (Referenz; Urteile & Designs)

Gesamturteil aus dem Review 2026-07-05: **Das Fundament ist gut; ein Groß-Rewrite ist
nicht gerechtfertigt.** Einziger echter (kleiner) Rewrite: der Rate-Limiter, der sich in
`backend/crates/core-api/src/main.rs` selbst als Platzhalter markiert.

### 6.1 Backend
- **BLEIBT unangetastet:** Auth-Kern (argon2 + Dummy-Hash-Timing, spawn_blocking,
  SHA-256-Token), Ökonomie-Kern (gem-engine/settlement/ledger/econ-params), Layering,
  Feed-Repo/Ranker, Posts/Comments-Pagination (limit/offset).
- **Login-Schutz (M0.5, A1–A5):** zwei unabhängige Schichten — enger Per-Route-IP-Bucket
  auf `/v1/auth/*` + Per-Account-Backoff in Postgres, geschlüsselt nach normalisierter
  E-Mail (unbekannte E-Mails drosseln identisch → kein Enumeration-Orakel; Lock-Check
  vor argon2 → kein Timing-Orakel). `/auth/check-email` bekommt keinen eigenen Bucket.
- **Session-Lifecycle (A5):** tokio-Interval-Housekeeping im core-api-Binary (NICHT im
  settlement_scheduler); TTL bleibt 30 Tage.
- **Feed-Cursor (B1):** eingefrorene Ranking-Zeit + Keyset `{ranked_at, score_bits,
  last_id}` als opakes base64-Token; `FeedPage { items, next_cursor }`; kein SQL-Change;
  neues Modul `feed/cursor.rs`; stale/invalid Cursor → 400.
- **System-Account (B2):** echte Zeile `users.id=0, is_system=true` (kein Login,
  `bot_gate_v=false`), Migration 0018 + die in 0016 aufgeschobenen FKs;
  `COMPANY_ACCOUNT_ID` nach `crates/domain` (Identität, kein Ökonomie-Knopf);
  System-Zeilen aus öffentlichen Reads gefiltert.
- **Guard-Tests (C1–C3):** Sybil-Split- und Bot-Gate-Proptests in gem-engine;
  Golden-Vector-Snapshot VOR jeder Formel-Entscheidung; Scale-Smoke 10k–50k Knoten
  (`#[ignore]`). Bewusst KEINE Binary-Level-Tests für scheduler/worker (dünne Schleifen
  über integrationsgetestete Funktionen).

### 6.2 Frontend
- **BLEIBT:** `apiFetch`/`ApiError`/`Wire<T>`/AuthProvider/Guards; Reels-Track/Gesten/
  Debounce; Login-Inline-`<style>`-Block (fertiges Figma-Design, grandfathered);
  sessionStorage-Token für 1a (CSP als Kompensation; Cookie+CSRF = 1b).
- **Kein react-query/SWR** (6 Call-Sites, kein Cache-Bedarf, Zero-Deps-Linie) —
  stattdessen `lib/useFetch.ts` (~40 Zeilen: Generation-Counter, `reload`, `enabled`);
  Migration mechanisch, ein Commit pro Seite (admin/* zuerst — dort fehlt der
  Stale-Guard heute komplett).
- **Dedup:** `lib/useLike.ts` (optimistisch, Revert; die EINE Stelle für den späteren
  `liked_by_me`-Fix) + `lib/useUnlock.ts` (auf useFetch, `enabled` für Reel-Lazy-Load).
- **Paging:** `lib/usePagedFeed.ts` gegen `FeedPage`; Shape-Sniffing-Fallback
  (Array = Legacy-Einzelseite) → Client vor dem Backend shipbar; Nachladen bei
  `idx >= posts.length - 3`; limit 50→20 erst nach Backend-Cursor.
- **Konventionen:** CSS Modules; Design-Tokens als CSS-Variablen in `globals.css`;
  User-Seiten auf Deutsch (admin darf englisch bleiben).

### 6.3 Ingestion
Architektur (Analyzer-Seam, reliable Queue, DLQ, Retries, Metriken) **BLEIBT**.
Offen nur: Modell-Stub füllen (M2.4), `/healthz` (M4.1), Service-Account (1b-Gate).

## 7. Qualitäts-Gates

Vor jedem Commit, je nach berührtem Bereich, aus dem jeweiligen Verzeichnis:
- **Rust** (`backend/`): `cargo fmt --all -- --check` &&
  `cargo clippy --all-targets --all-features -- -D warnings` && `cargo test --all`
  (Services laufen; auf dem Mac vorher `colima start`).
- **Python** (`services/ingestion/`): `ruff check .` && `mypy` && `pytest -q`.
- **Frontend** (`frontend/`): `npm run typecheck` && `npm run lint` && `npm test` &&
  `npm run build`.
- **API-Typen berührt:** `cargo test` regeneriert `bindings/`; die regenerierten Dateien
  gehören in denselben Commit (CI-Drift-Check erzwingt das).

CI ergänzt ab M0.5–M0.6: `cargo deny`, `pip-audit`, `npm audit` (blockierend nach
Triage) + wöchentlicher Schedule-Lauf.

## 8. Entscheidungen & offene Fragen

**Entschieden (2026-07-05, Owner):**
- Baseline = `review/audit-fixes`-Stand; kein Groß-Rewrite.
- Phase-1b-Krypto out of scope (Fiat-Revenue-Share-Strategie).
- ADR-Nummern: 0009 = Signal-Schema (M2.3); **0010** = Gewichtsformel-Entscheidung
  (löst den Schwebezustand von ADR 0008 auf; Deadline: vor 1a-β, nicht früher).

**Entschieden (2026-07-05, M1 teilweise — Owner-Antworten):**
- **Feed-Ranking über AI: JA.** M2.7 ist in scope; ADR 0009 definiert das Schema,
  der Ranker liest `content_signals` hinter einem Config-Flag.
- **AI = eigenständiger Block.** Strikt separiert von der Plattform; Kommunikation
  NUR über die bestehenden Seams: Queue rein, authentifizierte Write-backs raus,
  Reads über die öffentliche API. Kein Direktzugriff auf DB oder Ledger. (Bestätigt
  ADR 0006; Härtung dazu: eigener Service-Account statt geteilter
  Operator-Credentials wird von „1b-Gate" auf **M2** vorgezogen.)
- **Payout-Grenze (wichtig):** Die AI **liefert Signale** (Qualität,
  Bot-Wahrscheinlichkeit, Ranking-Features) — **wer was ausgezahlt bekommt,
  entscheidet weiterhin deterministisch `gem-engine`/`settlement`** mit den
  Konservierungs-Invarianten. AI-Einfluss auf Payouts ist nur als
  econ-params-gegateter Faktor im Gewichtsmodell zulässig, nie als freie Zuteilung
  durch das Modell. Grund: Auditierbarkeit, Reproduzierbarkeit, fail-closed —
  ein Modell-Output ist nicht deterministisch nachrechenbar, eine Gewichtsformel schon.
- **AI-Vorschlagswesen (Human-in-the-Loop, Owner-Präzisierung 2026-07-05):** Die AI
  tätigt keine Payouts und trifft keine finale Entscheidung. Sie LERNT und erzeugt
  **begründete Vorschläge** (z. B. Bot-Gate-Flags, Parameter-Empfehlungen), die der
  Operator bestätigt; erst dann wirken sie — ausschließlich über die bestehenden
  operator-only Pfade (`PUT /users/:id/verification`, econ-params-Versionsbump).
  Konsequenz für **ADR 0009**: Das Signal-/Vorschlags-Schema braucht
  Begründungsfelder (Evidenz/`reason`), damit Vorschläge reviewbar sind.
- **Payout-Rail:** Drittanbieter zuerst (P-3); eigenes KYC später evaluiert.
- **Referral-System:** gewollt → P-2.
- **Skalierungsziel:** stabil bis **10.000 Nutzer**; Wachstum = mehr Hardware,
  kein Umbau. Die ersten 10k laufen auf einer VM (M4 bleibt wie geplant; das
  C2-Scale-Smoke-Ziel 10k–50k Knoten und die M4.7-Load-Schwellen richten sich
  an dieser Zahl aus).
- **HLS-Ladder: verschoben.** Schwaches-Netz-Optimierung ist kein Beta-Ziel;
  bei gutem Netz muss es gut laufen (deckt die bestehende Single-Bitrate ab).
- **GPU: mieten → später kaufen** (bestätigt die hinterlegte Strategie; Migration
  bleibt verlustfrei by design).
- **Earnings-Modell = „wie YouTube" (Owner, 2026-07-05):** KEINE Coin-Distribution,
  in keiner Phase ohne Lizenz. Nutzer-Erträge sind eine **geschuldete Balance**
  (Plattform-Verbindlichkeit), auszahlbar ab Schwelle in Fiat/Krypto über
  non-custodiale Drittpfade, in-platform nutzbar für Boosts → P-5. Produktvision
  hängt zusammen: Social-Kern + Private Area (P-4) + Finance-Area (P-5) sind EIN
  kohärentes Ganzes und werden zusammen designt, aber gestuft gebaut.

**Weiter offen (Owner):**
1. ~~P-1-Feature-Matrix~~ ✅ entschieden (siehe P-1).
2. ~~Referral-Parameter~~ ✅ entschieden: 3 % / 6 Monate Default, Creator-Overrides
   operator-gesetzt (siehe P-2).
3. **P-4-Scoping:** Private Area launch-blockierend (1a) oder 1a-β? Cut-Höhe?
   Bereichs-Zugang vs. Einzelkauf? Rechts-Check terminieren.
4. Payout-/Zahlungs-Drittanbieter auswählen (P-3 + P-4-Fiat-Pfad; naheliegend:
   derselbe Anbieter, z. B. Stripe Connect).
5. Modell-Spezifikation im Detail — welche Signale genau (Qualitäts-Score,
   Bot-Likelihood, Embeddings?), Backfill-Ziel *(→ ADR 0009 / M2.3–M2.4)*.
6. GPU-Provider, EU-Region, Monatsbudget.
7. ADR-0008-Timing — vor 1a-β auflösen (empfohlen) oder formal 1b-Eingangstor?
8. Domain + VM-Provider/Region (z. B. Hetzner DE); Monitoring-Default
   (Uptime-Ping + `/metrics`) gilt als angenommen, falls kein Widerspruch.

## 9. Ops-Index

- `services/ingestion/RUNBOOK.md` — Ingestion-Betrieb, Modell-Bring-up, DLQ-Replay.
- `docs/OPERATIONS.md` — Ein-VM-Deploy-Story (TLS, Deploy/Rollback,
  Backup/Restore §7, Load-Smoke §9).
- `docs/GO-NO-GO-1a-beta.md` — Go/No-Go-Checkliste 1a-β (M4.8): Owner-
  Entscheidungen, Rechts-Gates, VM-Abnahme C1–C11, akzeptierte Beta-Risiken.
- `docs/adr/` — Architektur-Entscheidungen (0001–0008; 0009/0010 geplant).
