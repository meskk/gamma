# Go/No-Go — Phase 1a-β (MASTERPLAN M4.8)

> Einmal komplett durchgehen, BEVOR echte Nutzer und echtes (gedeckeltes) Geld
> auf die Plattform kommen. Jede Zeile hat einen Verantwortlichen und einen
> Nachweis. Fast jede technische Zeile hat einen dokumentierten, bereits lokal
> geprobten Weg und muss nur auf der ECHTEN VM einmal passieren — die zwei
> Ausnahmen sind markiert: C3 braucht neue Engineering-Arbeit (der eine
> bekannte technische Blocker), C4 seine erste Probe überhaupt. Reihenfolge:
> A → B → C (C-interne Reihenfolge beachten, siehe C7); D ist die Liste
> dessen, was für die Beta bewusst akzeptiert bleibt.

## A. Owner-Entscheidungen (ohne sie startet nichts)

- [ ] **Domain + VM-Provider/Region** gewählt (z. B. Hetzner DE) — MASTERPLAN
      §8 Frage 8. *(Owner)*
- [ ] **Gems→€-Umrechnungsbasis** entschieden — die Kernfrage der Beta: Wie
      wird aus dem Punkte-Gem-Kontostand der geschuldete €-Betrag? (Das ist
      der offene „Gems ↔ Balance"-ADR aus P-5 — für die Beta reicht eine
      beta-scoped Festlegung, aber OHNE sie ist weder der Cap prüfbar noch
      irgendein Payout berechenbar.) Dazu gehört: econ-params-Review gegen den
      Cap (sobald Gems €-Wert haben, ist die Emission die Gelddruck-Rate —
      `emission_day0_pt` = 5.753 PT/Tag Default) und ein `payout`-Kind im
      `ledger_entries`-Journal (kleine Migration — heute existiert keins),
      damit Auszahlungen im Geldjournal nachvollziehbar sind.
      *(Owner + Session)*
- [ ] **Auszahlungs-Cap der Beta** beziffert (die 1a-β-Definition ist
      „gedeckelte Fiat-Auszahlung" — wie hoch ist der Deckel pro Nutzer/
      gesamt, in € über die entschiedene Umrechnungsbasis?). *(Owner)*
- [ ] **Payout-Drittanbieter** gewählt (P-3; §8 Frage 4 — z. B. Stripe
      Connect / PayPal Payouts / Wise) und der (halb-)manuelle
      Beta-Payout-Prozess in OPERATIONS.md dokumentiert (P-3-Akzeptanz).
      *(Owner + Operator)*
- [ ] **P-4/P-5-Scoping** entschieden: Private Area / Finance-Area
      launch-blockierend für die Beta oder danach? (§8 Frage 3; die Beta kann
      ohne beide starten — dann ist bezahlter Content ausgeblendet, P-1.)
      *(Owner)*
- [ ] **ADR 0010 (Gewichtsformel)** aufgelöst — §8: „vor 1a-β" ist die
      empfohlene Deadline; der Golden-Vector-Snapshot (M3/C-Gates) gehört VOR
      die Formel-Entscheidung. *(Owner + Session)*
- [ ] **Verifizierungs-Prozess (Bot-Gate `v_i`)** definiert: Wer wird nach
      welchen Kriterien verifiziert, wer bedient den operator-only Endpoint?
      Settlement zahlt NUR an Verifizierte — ohne Prozess verdient niemand
      etwas. *(Owner)*

## B. Rechtliches

- [ ] **Anwalt-Freigabe für die gedeckelte Beta-Auszahlung** — der
      vorbereitete Fragenkatalog (Monetarisierungsstrategie; Kern: geschuldete
      Balance „nur auszahlbar" als Handelsverbindlichkeit, keine Coins, kein
      In-Platform-Ausgeben der €-Balance) ist gestellt und beantwortet.
      *(Owner + Anwalt)*
- [ ] **Impressum, AGB, Datenschutzerklärung** für die Beta-Domain stehen
      (EU-VM, DSGVO-Basics; Moderations-/Report-Weg existiert technisch
      schon). *(Owner, ggf. Anwalt)*
- [ ] **Auskunfts-/Lösch-Prozess (DSGVO Art. 15/17)** definiert: Es gibt
      KEINEN Lösch- oder Export-Endpoint — für die geschlossene Beta reicht
      ein dokumentierter manueller SQL-Weg (welche Tabellen, in welcher
      Reihenfolge, was passiert mit Ledger-Zeilen — Aufbewahrungspflicht vs.
      Löschung klärt der Anwalt) mit benanntem Verantwortlichen und Frist.
      *(Owner + Operator, Anwalt für die Ledger-Frage)*

## C. Technische Gates (auf der echten VM, in dieser Reihenfolge)

- [ ] **C1 — VM-Bring-up von Null über TLS**: frische EU-VM kommt allein mit
      OPERATIONS.md §0–§4 auf „läuft über TLS". Das ist zugleich das
      Real-VM-Bein des M4-Done-Kriteriums (bisher nur lokal geprobt — es
      braucht A/Domain+Provider). Nachweis: `https://api.<domain>/health` =
      200 von außen. *(Operator)*
- [ ] **C2 — Deploy aus der Registry**: `GAMMA_IMAGE_TAG` = SHA eines GRÜNEN
      Publish-Laufs, `pull && up -d --no-build` (§3/§6); CI + Security auf dem
      SHA grün. Nachweis: `docker compose ps` zeigt die GHCR-Images, nicht
      `:selfbuilt`. *(Operator)*
- [ ] **C3 — Media-Endpoint öffentlich — NO-GO, solange offen**: Mit dem
      gebündelten MinIO zeigen presigned Upload-/Download-URLs auf das
      compose-interne `minio:9000` — aus dem Browser unerreichbar; Upload und
      Playback sind damit in Prod FUNKTIONSUNFÄHIG (im lokalen Drill maskiert
      der Smoke-Override das Problem bewusst via 127.0.0.1:9100). Zwei Wege:
      (a) **Managed S3** — S3_*-Variablen auf den Provider zeigen, MinIO aus
      dem Compose nehmen (Hinweis in compose.prod.yml), oder (b) **MinIO
      publik machen**: Port auf 127.0.0.1 publishen, Caddy-Block
      `media.<domain> → 127.0.0.1:9000`, `S3_ENDPOINT=https://media.<domain>`
      (core-api erreicht MinIO dann ebenfalls über die öffentliche URL —
      Hairpin über die eigene public IP funktioniert auf Cloud-VMs).
      Nachweis: Upload → Unlock → Playback aus einem echten Browser gegen die
      VM. *(Operator; Weg-Wahl Owner)*
- [ ] **C4 — Frontend erreichbar**: §5-Weg (npm build + systemd) hinter
      `app.<domain>`; Login/Feed/Compose aus dem Browser. *(Operator)*
- [ ] **C5 — Service-Account provisioniert**: RUNBOOK §2; Ingestion-Container
      `healthy`, verarbeitet einen Test-Post (Signals sichtbar). *(Operator)*
- [ ] **C6 — Backups scharf**: Cron installiert (§7), erster nächtlicher Dump
      liegt vor, **Off-VM-Kopie eingerichtet** (Snapshot ODER Pull), und der
      **Restore-Drill einmal AUF DER VM** durchgespielt (beide Skripte).
      *(Operator)*
- [ ] **C7 — Load-Smoke PASS auf der VM** (§9; VM-Variante der Kommandos).
      Schwellen aus §9 halten ohne Tuning. **Reihenfolge-Falle:** Der
      Smoke-Stack teilt das Compose-Projekt — sein abschließendes `down -v`
      löscht ALLE Volumes, auch den C5-Zustand und den C6-Erst-Dump. C7 also
      VOR dem endgültigen C5/C6-Zustand fahren (bzw. C5/C6 danach erneut) und
      NIEMALS, nachdem echte Nutzerdaten existieren. *(Operator)*
- [ ] **C8 — Schutzschichten an**: kein `GAMMA_RATE_LIMIT_DISABLED`-Rest in
      `.env.prod`; `GAMMA_TRUST_PROXY=true`; Stichprobe: 6 falsche
      Login-Versuche → 429 + Retry-After. *(Operator)*
- [ ] **C9 — Dauerbetrieb läuft**: settlement-scheduler settlet die
      Vortags-Epoche (Log-Sichtung nach 24 h), Housekeeping purged Sessions,
      `/metrics` antwortet. *(Operator)*
- [ ] **C10 — Moderation einsatzbereit**: Operator-Konto existiert (SQL-Weg
      dokumentiert), Report → Takedown → Restore einmal gegen die VM
      durchgespielt; Report-Eingang wird regelmäßig gesichtet (wer?).
      *(Operator + Owner)*
- [ ] **C11 — Monitoring minimal**: Uptime-Ping auf
      `https://api.<domain>/health` eingerichtet (§8-Default „Uptime-Ping +
      /metrics" gilt als angenommen); Alarmweg = E-Mail/Push an den Owner.
      *(Operator)*
- [ ] **C12 — Ein echter Test-Payout end-to-end**: Kleinstbetrag über den
      gewählten Anbieter (A) an ein Test-Konto ausgezahlt, nach der
      entschiedenen Gems→€-Basis berechnet und als `payout`-Journal-Eintrag
      nachvollziehbar — BEVOR der erste echte Nutzer-Payout ansteht. Der
      Geld-RAUS-Weg ist der einzige 1a-β-kritische Pfad ohne lokalen Drill;
      diese Box ist seine erste Probe. *(Operator + Owner)*

## D. Für die Beta bewusst akzeptiert (kein Blocker, aber gewusst)

- sessionStorage-Token mit CSP als Kompensation (HttpOnly-Cookie + CSRF ist
  1b-Eingangstor).
- Kein Secrets-Manager: `.env.prod` lebt nur auf der VM (1b-Punkt).
- Single-Bitrate-HLS (Ladder verschoben — §8: „bei gutem Netz muss es gut
  laufen").
- Frontend ohne Container-Image (systemd-Weg; OPERATIONS §10).
- Redis-Queue-/DLQ-Verlust verschmerzbar (OPERATIONS §7); Medien-Backup nur
  über VM-Snapshots.
- AI-Modell noch Heuristik-Platzhalter (M2.3–M2.7 offen) — Feed rankt ohne
  `content_signals`; Payouts sind davon by design unabhängig.
- X-Forwarded-For ist bei `GAMMA_TRUST_PROXY=true` spoofbar (klassische
  XFF-Schwäche; Limits sind Backstop, nicht Sicherheitsgrenze — Login-Schutz
  hat zusätzlich den Per-Account-Backoff).
- **Keine Konto-Sperre**: Moderation wirkt auf Inhalte (Takedown), nicht auf
  Konten — ein missbräuchlicher Nutzer kann nicht gebannt oder ausgeloggt
  werden. Not-Behelf für die Beta (dokumentieren, wer ihn ausführen darf):
  Verifizierung entziehen (stoppt künftige Payouts) + Sessions des Kontos per
  SQL löschen (`DELETE FROM sessions WHERE user_id = …`). Echtes Ban-Feature
  vor dem öffentlichen Launch.
- **Kein Passwort-Reset** (es gibt keinen Mailer): Ein verlorenes Passwort
  strandet das Konto samt geschuldeter Balance. Beta-Behelf: Identität des
  bekannten Beta-Teilnehmers manuell prüfen, dann Payout der gestrandeten
  Balance über den A/C12-Weg; echte Recovery vor dem öffentlichen Launch.

**GO** = alle A-, B-, C-Boxen abgehakt. Jedes offene Kästchen ist ein
dokumentiertes NO-GO mit benanntem Verantwortlichen.
