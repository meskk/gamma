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

### M0.5 — Auth-Härtungs-Cluster (direkt nach M0; parallel zu M1 möglich)
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

### M4 — Ops-Readiness für Phase 1a-β (bewusst klein; kein k8s)
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
| M1.1+M1.4 (teilw.) | 2026-07-05 | *(dieser Commit)* | Docs | Owner-Antworten eingearbeitet: Backlog P-1..P-3, AI-Block-Prinzip + Payout-Grenze, 10k-Ziel, HLS verschoben; Restfragen in §8 |

## 5. Produkt-Backlog (gefüllt in M1.1 durch den Owner; Stand 2026-07-05)

### P-1 — Launch-Funktionsumfang definieren *(in 1a; Owner-Session nötig)*
Festlegen, welche Funktionen Usern zum Start sichtbar/nutzbar sind. Vorgehen: eine
Feature-Matrix (sichtbar / versteckt / operator-only) über den existierenden Bestand —
Posts, Kommentare, Likes/Interaktionen, Follows, Feed (For-you/Following), Compose +
Medien-Upload, Paid-Unlocks, Gems-Anzeige, Profil, Referral (P-2) — die der Owner
abtickt. **Akzeptanz:** Matrix hier eingetragen; Frontend blendet Nicht-Launch-Features
aus (Config, kein Code-Löschen).

### P-2 — Referral-System *(in 1a; Bau nach dem Auth-Cluster, vor 1a-β)*
User werben User per Referral-Link; der Referrer erhält einen Anteil (Cut) an den
Gem-Erträgen der Geworbenen. Design-Leitplanken (aus der Architektur zwingend):
- Cut-Höhe als **econ-params-Knob** (`referral_bps`), niemals hardcoded.
- **Konservierend:** Der Cut kommt AUS dem Payout des Geworbenen, es wird nichts
  zusätzlich gemintet — Invariante i (Σ payouts == emission) bleibt exakt.
- **Anti-Abuse:** Referral-Erträge zählen nur aus Nutzern hinter dem Bot-Gate
  (`v_i = true`) — sonst ist Referral + Bots ein direkter Harvest-Vektor.
- Empfehlung: nur EINE Referral-Ebene (mehrstufig = Pyramiden-Optik + Abuse-Fläche).
**Offen (Owner):** Cut-Höhe (bps), Dauer (lebenslang vs. erste N Epochen).
**Akzeptanz:** Registrierung nimmt Referral-Code an; Settlement bucht den Cut als
eigene `ledger_entries`-Art (`referral`); Summen bleiben exakt (Hamilton); Tests
decken Konservierung + Gate ab.

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

**Weiter offen (Owner):**
1. P-1-Feature-Matrix abticken (welche Funktionen zum Launch sichtbar sind).
2. Referral-Parameter: Cut-Höhe (`referral_bps`), Dauer, (Empfehlung: 1 Ebene).
3. Payout-Drittanbieter auswählen (P-3).
4. Modell-Spezifikation im Detail — welche Signale genau (Qualitäts-Score,
   Bot-Likelihood, Embeddings?), Backfill-Ziel *(→ ADR 0009 / M2.3–M2.4)*.
5. GPU-Provider, EU-Region, Monatsbudget.
6. ADR-0008-Timing — vor 1a-β auflösen (empfohlen) oder formal 1b-Eingangstor?
7. Domain + VM-Provider/Region (z. B. Hetzner DE); Monitoring-Default
   (Uptime-Ping + `/metrics`) gilt als angenommen, falls kein Widerspruch.

## 9. Ops-Index

- `services/ingestion/RUNBOOK.md` — Ingestion-Betrieb, Modell-Bring-up, DLQ-Replay.
- `docs/OPERATIONS.md` — *(entsteht in M4.4)* Ein-VM-Deploy-Story.
- Go/No-Go-Checkliste 1a-β — *(entsteht in M4.8, hier verlinkt)*.
- `docs/adr/` — Architektur-Entscheidungen (0001–0008; 0009/0010 geplant).
