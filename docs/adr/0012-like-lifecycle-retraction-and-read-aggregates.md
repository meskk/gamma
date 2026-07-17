# ADR 0012 — Like-Lifecycle: Retraktion statt Löschung, Live-Aggregate, Kommentar-Likes

Status: accepted · Date: 2026-07-17

*(ADR 0010 bleibt für die Gewichtsformel-Entscheidung reserviert, §8.)*

## Context

Das Like-System war bis hierher nur halb integriert: Der Schreibpfad
(`POST /v1/interactions` → `interaction_events` → Settlement-Kanten → Gems) war
vollständig und getestet, aber es gab **kein Unlike**, **keine Like-Zähler**,
**kein `liked_by_me`** (die dokumentierte „UI-Lüge“: nach Reload zeigte ein
gelikter Post „ungeliked“), **keine Kommentar-Likes**, und Likes hatten
**keinen Einfluss aufs Feed-Ranking** (der Ranker las das tote
`posts.popularity_score`, das nie beschrieben wird).

Die Spannung: `interaction_events` ist bewusst ein **append-only, epochen-
gestempeltes Journal** (0001: „Missing this early is the one mistake you cannot
undo“), mit Anti-Inflations-Dedup pro Epoche (0009) und FK-Bestandsschutz
(0015). Ein klassisches DELETE-Unlike würde ökonomische Historie vernichten;
ein denormalisierter `like_count`-Zähler könnte gegenüber dem Journal driften.

## Decision

### 1. Unlike = Retraktion (Voiding), nie Löschung

`interaction_events` bekommt `retracted_at TIMESTAMPTZ` (Migration 0024). Ein
Unlike (`DELETE /v1/interactions`, gleicher Body wie der Like) setzt
`retracted_at = now()` auf **alle aktiven Zeilen des kanonischen Tupels über
alle Epochen** — die Zeile bleibt im Journal (Audit), zählt aber nirgends mehr:

- `edges_for_epoch` (die autoritative Settlement-Leseseite) überspringt
  retrahierte Zeilen. Ein Unlike **vor** dem Settlement der Epoche entfernt die
  Kante; **nach** dem Settlement ist es reine Anzeige — ausgezahlte Epochen
  werden nie wieder geöffnet (Settlement ist idempotent und liest zum
  Settle-Zeitpunkt; keine Rückforderung, bewusst).
- Re-Like **in derselben Epoche** ent-void die Originalzeile
  (`ON CONFLICT … DO UPDATE SET retracted_at = NULL`): gleiche id, gleiches
  eingefrorenes Gewicht. Like→Unlike→Like-Zyklen können das Dedup-Cap
  (eine gewichtete Zeile pro Tupel und Epoche) **nicht** umgehen.
- Retraktion ist **idempotent** (0 betroffene Zeilen = 204, kein 404): ein
  Toggle-Doppelklick darf nicht fehlschlagen, und eine von der Existenz des
  Ziels unabhängige Antwort öffnet kein Existenz-Orakel.
- Nur `like` ist retrahierbar (`only_like_retractable`): ein Comment-Event
  spiegelt eine weiterhin existierende Kommentar-Zeile, Follows haben ihren
  eigenen DELETE-Pfad, Dwell/Share haben keine Undo-Semantik.

### 2. Read-Aggregate LIVE aus dem Journal, nie denormalisiert

`Post`, `Comment` und `User` tragen jetzt `like_count`/`liked_by_me` bzw.
`likes_received` — **berechnet per Subquery aus `interaction_events`**
(`type = like AND retracted_at IS NULL`), nicht als gepflegter Zähler. Eine
Quelle der Wahrheit, kein Drift; partielle Indizes
(`ie_post_type_active`, `ie_comment_type_active`, Spalten-IS-NULL-Prädikat ist
immutable-safe) halten die Lookups index-only. Auf 1a-Skala (10k Nutzer) ist
das bewusst billiger als jede Cache-Invalidierung korrekt zu bauen.

Die Zähler zählen **distinkte LIKER, nicht Journal-Zeilen**
(`COUNT(DISTINCT actor_id)`; `likes_received`:
`COUNT(DISTINCT (actor_id, post_id))`). Das Journal hält absichtlich eine
gewichtete Zeile pro (Actor, Ziel, Epoche) — tägliches Re-Engagement ist eine
ökonomische Kante —, aber die Anzeige hat Per-User-Boolean-Semantik wie
`liked_by_me` und der Toggle. Zeilen statt Actors zu zählen hieße: ein Account
pumpt einen öffentlichen Zähler (und den Feed-Term) per rohem API-Call um
+1/Tag, und ein Unlike (voidet ALLE Epochen) ließe den Zähler um N fallen.
Self-Likes zählen in der Anzeige (der Autor kann den eigenen Post liken;
ökonomisch bleiben sie über den Self-Loop-Drop in `edges_for_epoch` inert).

`User.likes_received` zählt nur Likes auf **öffentlichen, nicht versteckten**
Posts des Nutzers: der öffentliche Profil-Stat darf weder moderierte noch
Paywall-Engagement-Volumina leaken (P-4). Kommentar-Likes und direkte
User-Likes zählen dort nicht — der Stat spiegelt, was das Profil-Grid zeigt.

### 3. Kommentar-Likes über dasselbe Event-Modell

`interaction_events.comment_id` (FK NO ACTION wie 0015) + Erweiterung des
Dedup-Index um die Spalte. Die Kante fließt zum **Kommentar-Autor**
(`COALESCE(target_id, post.author, comment.author)`), gegated durch die
Sichtbarkeit des **Posts des Kommentars** (ein Kommentar ist exakt so sichtbar
wie sein Post — A4f-Guard beim Schreiben, `edges_for_epoch`-Drop bei
private/hidden im Settlement). Die Normalisierung kollabiert die Id-Tripel auf
EINE kanonische Form (target > comment > post), damit redundante Ids keine
unterschiedlichen Dedup-Tupel für dieselbe Kante prägen können (die
0009-Umgehung, verallgemeinert).

### 4. Feed: log-gedämpfter Like-Term statt toter Spalte

Der Cold-Start-Ranker addiert `ln(1 + like_count)` (Gewicht 1.0) in den
Popularitätsterm; `popularity_score` bleibt additiv erhalten (heute 0). Die
Popularitäts-CTE der Kandidatenauswahl sortiert nach aktiven Likes statt nach
der toten Spalte. Das ist eine **Ranking-Heuristik, kein Ökonomie-Knopf** —
Auszahlungen lesen sie nie, daher lebt die Konstante bei den anderen
Ranker-Konstanten in `feed/service.rs`, nicht in `econ-params` (ADR 0003
unberührt). Bekannte, akzeptierte Randunschärfe: Der Cursor friert die
Ranking-Uhr ein, aber nicht den Like-Zähler — ein Like mitten in der
Pagination kann einen Post über die Cursor-Grenze schieben (seltene
Duplikat-/Skip-Möglichkeit an der Seitennaht; Klasse „jeder count-gerankte
Feed“).

## Consequences

- Das Produkt-Level-„liked“ ist epochenübergreifend (`liked_by_me` =
  irgendeine aktive Like-Zeile), das Ökonomie-Level bleibt epochenweise
  (tägliches Re-Engagement erzeugt weiter neue gewichtete Kanten — unverändert,
  aber durch die Toggle-UI faktisch auf Unlike→Re-Like-Zyklen beschränkt).
- Ein künftiger User-Erasure-Pfad muss `retracted_at`-Zeilen genauso behandeln
  wie aktive (anonymisieren/aggregieren, 0015 gilt unverändert).
- Harte Post-Löschung würde jetzt zusätzlich am `comment_id`-FK scheitern
  (Kommentare kaskadieren von Posts): gewollt laut — Posts werden soft-hidden,
  nie hart gelöscht.
- Frontend: `useLike` ist ein hydratisierter Toggle (Server-Zeile als Baseline,
  lokales Override, Single-Flight); die „UI-Lüge“-Kommentare sind Geschichte.
