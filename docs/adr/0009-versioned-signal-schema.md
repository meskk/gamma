# ADR 0009 — versioned signal schema (the AI's contract with the platform)

Status: accepted · Date: 2026-07-08 · Owner-Entscheidungen: MASTERPLAN §8 (2026-07-08)

## Context

ADR 0006 built the ingestion seam and deliberately left `content_signals.signals`
free-form JSONB "until a future ADR". That future is now — M2.4 (real model)
must not start against an unversioned contract. What the freedom costs today:

- **No shape at all.** The API validates only a non-empty `model_version`; the
  integration tests already store mutually inconsistent key sets (`word_count`
  vs. `words` vs. `topic` vs. `x`). Nothing stops silent drift between analyzer
  versions.
- **Blind last-writer-wins.** `post_id` is the sole primary key and the upsert
  compares nothing — a write from an OLDER model version silently clobbers a
  newer row.
- **The model swap is not actually executable.** RUNBOOK §6 step 4 promises a
  version-targeted re-enqueue (prep-plan P4), but the backfill query only finds
  posts with NO signals row — an analysed corpus can never be re-analysed.
- **The owner's Human-in-the-Loop decision has no home.** M1 decided: the AI
  produces reviewable PROPOSALS (bot-gate flags, parameter recommendations)
  with reasons, and only operators act on them — but no proposal storage or
  schema exists anywhere.

Owner decisions feeding this ADR (M1 2026-07-05 + §8 Frage 5, 2026-07-08):
feed ranking WILL read signals behind a flag (M2.7); payouts stay deterministic
in gem-engine, AI influence only ever as a future econ-params-gated factor;
v1 signal set = quality, bot-likelihood, topics, language, NSFW **plus
embeddings**; topic taxonomy = the app's own category set; backfill target =
the whole corpus; proposal schema defined now, built with the model.

## Decision

### 1. Two versions, two meanings

Every signal row carries BOTH:

- `model_version` (exists today): WHO produced it — owned by the analyzer
  implementation, opaque, never ordered.
- `schema_version` (new SMALLINT column, wire field next to `model_version`):
  WHICH CONTRACT the `signals` object follows. Existing rows get `0` (legacy,
  no contract); this ADR defines version `1`. Consumers gate on
  `schema_version`, never on `model_version`.

Evolution rule: **additive-only within a schema version** (new optional fields
never bump it); a breaking change (rename, type change, semantics change)
bumps `schema_version`, and consumers ignore rows below their minimum.

### 2. Schema v1 — a typed core plus an open annex

`signals` (JSONB) under `schema_version = 1`:

| Feld | Typ | Bedeutung |
|---|---|---|
| `quality` | f64 ∈ [0,1], optional | Inhaltsqualität (Ranking-Feature) |
| `bot_likelihood` | f64 ∈ [0,1], optional | Bot-Evidenz DIESES Posts (Aggregation → Vorschläge) |
| `topics` | [string], optional | NUR Werte aus dem kanonischen Kategorien-Set der App (Owner: eigene Taxonomie abgelehnt — Feed-Matching funktioniert sofort) |
| `language` | string, optional | primäres BCP-47-Tag (`de`, `en`, …) |
| `nsfw_likelihood` | f64 ∈ [0,1], optional | Moderations-Hinweis, nie Auto-Takedown |
| `extras` | object, optional | analyzer-eigener Anhang, frei — wird von KEINEM Konsumenten gelesen |

The API VALIDATES the core on write (types + ranges + taxonomy membership;
unknown top-level keys are rejected — additions go through this ADR's annex or
a schema bump). All core fields are optional so the heuristic stays honest: it
cannot produce `quality`, so it writes `schema_version 1` with only
`language`/`extras` (its current keys move into `extras` as `heuristic-v1`).
Optional ≠ ungetypt: wenn ein Feld da ist, stimmt sein Typ.

### 3. Embeddings live next door, not inside

Owner wants embeddings in v1. They are infrastructure (Phase-2 personalization
/ similarity), not a ranking signal, and would bloat every signals read — so:

- Neue Tabelle `post_embeddings (post_id PK → posts ON DELETE CASCADE,
  model_version TEXT, dim SMALLINT, embedding REAL[], updated_at)`.
- Transport über DENSELBEN Write-back: optionales Top-Level-Feld `embedding:
  [f32]` in `PUT /v1/posts/:id/signals`; die API legt es getrennt ab.
- `GET /v1/posts/:id/signals` liefert Embeddings NICHT aus (Status-Endpoint
  zählt sie). Kein Index/pgvector, bis ein Konsument existiert (Phase 2) —
  plain `REAL[]` genügt und ist verlustfrei migrierbar.

### 4. One current row per post; the swap converges instead of guarding

Die Zeile pro Post bleibt EINE (der Ranker braucht genau eine aktuelle Sicht;
Signale sind kein Geld — Historie wäre Ballast). Statt eines DB-seitigen
Versions-Guards (model_versions sind unordbar):

- **P4 wird gebaut (M2.6):** Backfill lernt Versions-Targeting —
  `POST /v1/admin/ingestion/backfill?target_model=<v>` enqueued Posts, deren
  Zeile fehlt ODER deren `model_version != v` ist. Owner-Backfill-Ziel: der
  GANZE Korpus; Konvergenz ist über `GET …/status` (`by_model_version`)
  beobachtbar und erreicht, wenn dort nur noch `<v>` steht.
- **Out-of-order-Schutz ist eine operative Invariante, die DIESES ADR erhebt**
  (und mit demselben Commit in RUNBOOK §6 verankert): Es konsumiert zu jedem
  Zeitpunkt genau EIN Worker `gamma:ingestion`, und der Swap stoppt den alten
  Worker, BEVOR der neue startet. Das ist nicht nur Vorsicht — zwei parallele
  Worker sind aktiv destruktiv, weil sie sich die Processing-Liste teilen und
  `recover_stranded()` beim Start die In-Flight-IDs des jeweils anderen
  requeued; ein alter Worker kann so nach einem Retry eine neuere Analyse per
  blindem Upsert überschreiben. Die Queue trägt nur Post-IDs, deshalb
  analysiert ein DLQ-Replay immer mit dem AKTUELLEN Analyzer — das Risiko
  liegt allein im Zwei-Worker-Fenster, das die Invariante schließt. Wird sie
  doch verletzt, ist der Schaden per P4-Backfill reparierbar (Status zeigt
  Misch-Versionen), nicht still: darum Konvergenz-Check als fester Teil des
  Swaps.

### 5. Consumption contracts (wer darf was lesen)

- **Feed (M2.7):** liest ausschließlich Kern-Felder (`quality`, `topics`,
  `language`) aus Zeilen mit `schema_version ≥ 1`, hinter einem Laufzeit-Flag
  (`GAMMA_FEED_SIGNALS`, default AUS), strikt additiv zum bestehenden Score —
  Signale ordnen um, sie unterdrücken nicht. `nsfw_likelihood` ist bewusst
  KEIN Feed-Input: ein score-getriebener Filter wäre eine automatisierte
  Moderationswirkung (die Feed-Hälfte eines Takedowns) und damit eine
  Owner-Entscheidung, die nicht getroffen ist — bis dahin bleibt es reiner
  Moderations-Hinweis für den Operator. Die konkrete Formel ist M2.7; die
  Grenze (welche Felder, Flag, nie payout-relevant) ist DIESES ADR.
  Der DEFERRED-BOUNDARY-Kommentar in `feed/mod.rs` fällt erst mit M2.7.
- **Payouts:** `gem-engine`/`settlement` lesen `content_signals` NIE. Ein
  künftiger Signal-Einfluss auf Gewichte wäre ein eigener ADR mit
  econ-params-versioniertem Faktor (Owner-Beschluss M1) — in 1a: keiner.
- **Vorschläge (Human-in-the-Loop; Schema jetzt, Bau mit dem Modell):**
  `ai_proposals (id, created_at, model_version, target_kind ∈ user|post|param,
  target_id, proposed_action, confidence f64, reason TEXT NOT NULL,
  evidence JSONB, status ∈ pending|accepted|rejected, decided_by, decided_at)`.
  Service-Rolle schreibt, Operator entscheidet; die WIRKUNG läuft ausschließlich
  über die bestehenden operator-only Pfade (`PUT /users/:id/verification`,
  econ-params-Bump) — ein Vorschlag wendet sich nie selbst an. `reason` ist
  NOT NULL: unbegründete Vorschläge sind per Schema unmöglich (M1-Auflage).
  Die Heuristik erzeugt keine Vorschläge; Tabelle + Endpoints + Admin-Ansicht
  entstehen mit dem ersten echten Erzeuger (M2.4/M2.5).

### 6. Unverändert

Rollen (Write = Service-oder-Operator, Read = operator-only), 204-Semantik,
`ContentSignal` bleibt außerhalb des ts-rs-Frontend-Vertrags, Queue-Format
(Post-IDs), Analyzer-Protokoll (`model_version`-Property + `analyze(post) →
dict` — das dict bekommt jetzt einen Vertrag).

## Consequences

- M2.4 implementiert: `schema_version`-Spalte + Kern-Validierung im
  Signals-Service, Heuristik → `heuristic-v1` (Kern leer, extras befüllt,
  `language` wenn billig erkennbar), `post_embeddings`-Tabelle + optionales
  `embedding`-Feld, Modell-Analyzer liefert das v1-Kern-Set.
- M2.6 implementiert P4 (`target_model`) und fährt den Ganzer-Korpus-Backfill;
  RUNBOOK §6 wird damit wahr statt aspirational.
- M2.7 verdrahtet den Feed hinter `GAMMA_FEED_SIGNALS` und ersetzt den
  DEFERRED-Kommentar in `feed/mod.rs` durch einen Verweis hierher.
- `ai_proposals` + Review-Fläche entstehen mit dem Modell (M2.4/M2.5).
- Legacy-Zeilen (`schema_version 0`) verschwinden mit dem Korpus-Backfill von
  selbst; bis dahin liest kein Konsument sie.
