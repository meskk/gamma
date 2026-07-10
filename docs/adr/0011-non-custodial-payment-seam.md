# ADR 0011 — non-custodial payment seam (die Private Area bezahlt sich außerhalb)

Status: accepted · Date: 2026-07-08 · Owner-Entscheidungen: MASTERPLAN §5/P-4 (2026-07-08)

*(ADR 0010 bleibt für die Gewichtsformel-Entscheidung reserviert, §8.)*

## Context

P-4 (Private Area) ist entschieden: Jedes Profil bekommt einen vierten Reiter,
dessen Zugangsmodell der CREATOR wählt — kostenlos, Einmalpreis, Abo oder
Einzelzahlung — und der Kauf läuft **non-custodial**: Die Plattform hat keine
Verwahrlizenz und darf Kundengeld NIE halten (bewusste Rechtsstruktur,
2026-07-01). Der Owner will BEIDE Zahlwege — Stripe zuerst, Wallet/Smart-
Contract als Stufe 2 — und einen 10-%-Cut. Gebaut wird hinter Flags; live erst
nach Anwalt-Freigabe.

Gleichzeitig gilt der härteste Bestandsschutz des Projekts: `ledger_entries`
ist das konservierte PT-Punkte-Journal (Invariante i, fail-closed). Fiat- oder
Chain-Zahlungen dürfen diese Ökonomie nicht einmal berühren.

## Decision

### 1. Die non-custodiale Grenze (harte Invariante)

Kundengeld fließt **direkt vom Käufer zum Creator** über den Provider — bei
Stripe als Direct Charge auf das Stripe-Konto des Creators (Express-Account,
KYC bei Stripe). Unser Cut ist die **Application Fee**, die Stripe einbehält
und als PLATTFORM-UMSATZ auskehrt — das ist Firmengeld, nie verwahrtes
Kundengeld. Es gibt keinen Plattform-Topf, kein Treuhandkonto, keine
Auszahlungspflicht der Plattform gegenüber Creators: Wer dem Creator Geld
schuldet, ist Stripe, nicht wir.

### 2. Fiat berührt das PT-Journal NIE

`ledger_entries` bleibt ausschließlich die konservierte Punkte-Ökonomie.
Private-Area-Käufe erscheinen dort unter keinen Umständen — auch nicht als
„Info-Zeile". Stattdessen: eigene **`purchases`-Tabelle** als Audit-Spiegel
der Provider-Wahrheit (id, provider, provider_ref eindeutig, viewer, creator,
kind, amount_cents, currency, fee_cents, status, created_at). Sie ist NICHT
konserviert und beansprucht das auch nicht: Quelle der Wahrheit ist der
Provider (Stripe-Events), die Tabelle ist unser Nachvollzug. Die
Settlement-Invarianten bleiben unberührt — kein neuer Geldpfad im
konservierten System.

### 3. Der PaymentProvider-Seam (dritter Seam des Projekts)

Neues Crate `crates/payments` mit einem Trait analog zu
`LedgerBackend`/`Analyzer`:

- `onboarding_link(creator)` — der Creator verbindet sein Provider-Konto
  (Stripe Express Onboarding).
- `create_checkout(kind, price, fee, viewer, creator, …) → URL` — startet den
  Kauf; der Cut wird HIER aus dem econ-params-Knopf berechnet und als
  Application Fee mitgegeben.
- `parse_event(headers, body) → PaymentEvent` — verifiziert die
  Webhook-Signatur und normalisiert das Ereignis (bezahlt, Abo verlängert,
  Abo beendet, erstattet).

Stufe 1: Stripe-Impl. Stufe 2: Wallet/Smart-Contract-Impl hinter DEMSELBEN
Trait (eigenes Gate: Chain-Wahl-Owner-Frage, Contract-Audit — Zugänge, die
der Contract on-chain gewährt, melden sich über einen Indexer als
PaymentEvents). Tests laufen gegen einen Fake — CI braucht weder
Stripe-Schlüssel noch Chain. Implementierungsnotiz: bewusst KEIN
Stripe-SDK; die benötigte Fläche (Checkout Session, Account Link,
Webhook-HMAC) ist klein genug für einen handgeschriebenen, deny-geprüften
HTTP-Client.

### 4. Zugangsmodelle und Entitlements

`private_areas (creator_id PK, access_model ∈ free|one_time|subscription|
per_post, price_cents, currency='EUR', description, updated_at)` — das Modell
wählt der Creator, alle vier ab Tag 1 im Datenmodell; die Zahlungs-STUFEN
landen nacheinander (Einmalpreis → Abo → Einzelkauf), sichtbar wird ein
Kaufweg erst mit seiner funktionierenden Stufe (P-1-Prinzip: nichts
vortäuschen).

Zugang materialisiert sich als **Entitlement**, nie als Zahlungsabfrage zur
Lesezeit: `area_entitlements (viewer_id, creator_id, source ∈
purchase|subscription|operator, expires_at NULL = dauerhaft, granted_at)`;
Einzelkauf-Entitlements (per Post) kommen mit ihrer Stufe. Abos setzen
`expires_at` und werden von `invoice.paid`-Events verlängert — läuft es ab,
erlischt der Zugang von selbst, ohne Cron.

### 5. Die Sichtbarkeits-Invariante (fail closed)

`posts.area ∈ public|private` (Default public). Private Posts erscheinen in
KEINEM öffentlichen Lesepfad — Feed-Kandidaten, Post-Listen, Einzel-Read,
Kommentare, Ingestion-Backfill-Zählung öffentlicher Sichten — außer der
Betrachter ist entitled oder der Creator selbst. Durchgesetzt wird das wie
die `hidden_at`-Invariante: in den Repository-Queries, nicht in Handlern —
ein vergessener Pfad blendet zu viel AUS, nie zu viel EIN.

**Nachtrag (2026-07-09, Owner-Entscheidung — verbindliche Präzisierung des
kanonischen Prädikats für A4b–A4f):** Das implementierte Prädikat hat einen
DRITTEN Arm neben „entitled ODER Creator": ein Bereich mit
`access_model = 'free'` ist für JEDEN EINGELOGGTEN Nutzer sichtbar (nicht nur
für „entitled" im Sinne einer `area_entitlements`-Zeile). „Free" heißt also
Mitglieder-ohne-Bezahlung, **Login erforderlich** (nicht weltöffentlich) —
und diese Sichtbarkeit gilt in ALLEN Lesepfaden, auch in der globalen
Timeline, nicht nur auf dem Creator-Profil. Das kanonische Prädikat lautet
vollständig: `area = 'public' OR author_id = $V OR EXISTS(live
area_entitlements) OR (private_areas.access_model = 'free' AND $V IS NOT
NULL)`. Der `$V IS NOT NULL`-Guard steht auf den anonym-erreichbaren
Lesepfaden (posts get/list, comments list, post_visible_to) und entfällt
bewusst dort, wo der Betrachter strukturell authentifiziert ist (Feed,
comments create, Media-Rail). Per-Post-Read-Gating (`access_model='per_post'`)
ist bis zu seiner Zahlungsstufe (A9) auf Creator-Ebene zurückgestellt. Diese
Präzisierung ist auch für das Rechts-Gate relevant: Aktivierung setzt
Anwalt-Freigabe voraus, und der Kreis der Zuschauer eines free-Bereichs ist
größer als „entitled" — alle eingeloggten Nutzer.

**Der Media-Pfad ist ausdrücklich Teil der Invariante** — und der eine Pfad,
den das hidden_at-Muster strukturell NICHT erreicht: Media-Entitlement ist
heute per-Asset (`unlock_price <= 0` ⇒ jeder eingeloggte Nutzer bekommt die
presigned URL), ohne Join zu Posts — die Textmauer stünde, der eigentliche
Content (Bilder/Videos in Originalqualität, Asset-IDs sind enumerierbar)
wäre frei. Deshalb: Das Asset eines privaten Posts ist AREA-GATED —
`is_entitled` joint über den besitzenden Post (`posts.media_id`) auf dessen
`area` und verlangt bei `private` das Area-Entitlement (oder Ownership);
das deckt `GET /media/:id`, den HLS-Manifest-Pfad und jede presigned
Auslieferung. A4 implementiert Post- UND Media-Rail zusammen; die Semantik
unattachter Assets bleibt unverändert (außerhalb des P-4-Scopes).

**Die ökonomische Seite ist ENTSCHIEDEN, nicht dem Zufall überlassen:**
Interaktionen auf privaten Posts gehen NICHT in den Settlement-Graphen ein
(`edges_for_epoch` filtert `area = 'private'` genauso wie `hidden_at`).
Begründung: saubere Rail-Trennung — die Private Area verdient DIREKT über
den Provider (Rail 1, Marktplatz), Gems verdienen ausschließlich über
ÖFFENTLICHES Engagement (Rail 2). Alles andere würde den Payout-Graphen aus
Engagement speisen, das hinter einer Paywall für Report-getriebene
Moderation unsichtbar ist — eine Bot-Harvest-Fläche genau dort, wo das
Projekt sein härtestes ungelöstes Risiko verortet. (Owner kann das später
bewusst ändern; dann als econ-Entscheidung mit eigenem ADR.) Moderation
bleibt möglich: Entitled-Betrachter können private Posts REPORTEN, und der
Operator-Read-Pfad sieht gemeldete private Posts (Operator-Override wie
überall sonst).

Die AI-Ingestion analysiert private Posts in Stufe 1 NICHT (kein
Signal-Leak über den Feed; Aufnahme wäre eine spätere, bewusste
Entscheidung).

### 6. Ökonomie-Knopf und Flags

- `private_area_fee_bps = 1000` (10 %) in econ-params (Versions-Bump) — nie
  hardcoded; angewendet bei Checkout-Erzeugung (Integer-Cents, floor).
- Backend-Flag `GAMMA_PRIVATE_AREA` (default aus) gated Konfigurations-,
  Checkout- und Webhook-Endpoints; Frontend-Flag
  `NEXT_PUBLIC_FEATURE_PRIVATE_AREA` gated den Reiter. **Aktivierung erst
  nach Anwalt-Freigabe** (Go/No-Go-Box; Rechts-Gate per Owner-Entscheidung
  von „vor Bau" auf „vor Aktivierung" verschoben).

### 7. Der Webhook ist die einzige Wahrheitsquelle

Zugang wird ausschließlich aus signaturverifizierten Provider-Events gewährt
(idempotent per Event-Id; Redirect-/Success-URLs des Clients beweisen
nichts). Events, die wir nicht zuordnen können, werden geloggt und 200-quittiert
(Stripe-Retry-Semantik), aber gewähren nie Zugang.

## Consequences

- A2–A10 implementieren gegen diesen ADR (Stufenplan in MASTERPLAN §5/P-4).
- Die Wallet-Stufe erbt Seam, Entitlements und Invarianten unverändert —
  neu sind nur Provider-Impl + Gate.
- `purchases` gibt P-5 (Finance-Bereich) später eine fertige Kauf-Historie.
- Neue externe Abhängigkeit der PLATTFORM (nicht des Geldes): Stripe-Ausfall
  heißt „kein NEUER Kauf", nie „Zugang verloren" — Entitlements liegen bei uns.
