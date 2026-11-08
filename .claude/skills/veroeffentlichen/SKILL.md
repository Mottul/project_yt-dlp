---
name: veroeffentlichen
description: Eine neue Version des MottulVideoLoader veröffentlichen – Version in Cargo.toml anheben, Release-Workflow starten, Entwurf prüfen, Notizen schreiben, veröffentlichen. Verwenden, wenn ein Release, eine neue Version, ein Installer für Anwender oder „veröffentlichen“ gewünscht ist.
---

# Veröffentlichen

So durchlaufen bei 0.1.2, 0.1.3 und 0.1.4 (8.11.2026). Der Workflow baut rund 6½–7 Minuten.

**Öffentlich:** Einen geprüften Entwurf veröffentlichen (Schritt 5) hat der Nutzer dauerhaft
erlaubt (8.11.2026). Auf seinem Rechner steht dafür `Bash(gh release edit *)` in
`.claude/settings.local.json` (nur lokal, nicht im Repo). Entwürfe löschen weiter nur nach
Rückfrage. Ein Entwurf ist nicht öffentlich und darf jederzeit gebaut werden.

## 1. Version anheben

Die Version steht **nur** in `Cargo.toml` unter `[workspace.package]` – `tauri.conf.json` und
`package.json` tragen keine (Tauri liest sie aus Cargo.toml).

```bash
git switch main && git pull --ff-only
gh release list --limit 5          # letzte Version; Entwürfe zählen mit
# version = "x.y.z" in Cargo.toml setzen
cargo update --workspace           # zieht Cargo.lock nach, ohne Abhängigkeiten anzufassen
```

Patch für Fehlerbehebungen und kleine Funktionen (bisherige Praxis), Minor für Größeres. Das
Anheben kommt über einen PR nach `main`, wie jede andere Änderung. Probe vor dem Lauf:
`npm run app:build` → `target/release/bundle/nsis/MottulVideoLoader_<version>_x64-setup.exe`.

## 2. Workflow starten

Voraussetzung: Die Prüfungen des PRs, der die Version anhob, waren grün
(`gh pr view <nr> --json statusCheckRollup`). Die CI für den Merge-Commit auf `main` muss nicht
abgewartet werden – sie prüft denselben Stand, der Release baut ihn ohnehin selbst.

```bash
gh workflow run release.yml --ref main -f veroeffentlichen=true
gh run list --workflow release.yml --limit 1 --json databaseId,status,headSha
gh run watch <id> --exit-status --interval 30    # im Hintergrund laufen lassen
```

Ohne `-f veroeffentlichen=true` wird nur gebaut (Ergebnisse 7 Tage am Lauf, kein Entwurf).
Der Job **Version** leitet den Tag `v<version>` aus Cargo.toml ab und bricht ab, wenn

- es den Release schon gibt, auch als Entwurf → **Anheben vergessen**;
- ein gepushter Tag `v*` nicht zur Version passt.

## 3. Entwurf prüfen

```bash
gh release view v<version> --json isDraft,targetCommitish,assets \
  --jq '.targetCommitish, (.assets[] | "\(.name) \(.size)")'
```

Erwartet: `targetCommitish` = der Stand von `main`, und vier Dateien mit der Version im Namen:

| Datei | Für |
| --- | --- |
| `MottulVideoLoader_<v>_x64-setup.exe` | Windows |
| `MottulVideoLoader_<v>_universal.dmg` | macOS ab 11.3 (Intel + Apple Silicon) |
| `MottulVideoLoader_universal.app.tar.gz` | macOS, entpackte App |
| `MottulVideoLoader-Terminal-macos.zip` | Macs vor 11.3 (Terminal-Variante) |

## 4. Notizen

Was neu ist, aus Anwendersicht und auf Deutsch – aus `git log v<vorher>..main --oneline`
(Merge-Commits und PR-Texte). Den Hinweis auf die fehlende Signatur aus dem Workflow-Text
behalten. Oben eine Zeile, wie man von der Vorversion aktualisiert – gleichlautend mit
„Aktualisieren“ in `docs/INSTALL.md`, nichts Eigenes behaupten. Datei im Scratchpad schreiben,
dann:

```bash
gh release edit v<version> --notes-file <scratchpad>/notes.md
```

## 5. Veröffentlichen

```bash
gh release edit v<version> --draft=false --latest
git fetch --tags && git rev-parse --short v<version>^{commit}   # = Stand von main
curl -s -o /dev/null -w "%{redirect_url}\n" https://github.com/Mottul/project_yt-dlp/releases/latest
```

Der Tag entsteht erst jetzt, am `targetCommitish` des Entwurfs. `/releases/latest` muss auf
`…/tag/v<version>` zeigen – darauf baut der Update-Hinweis der App (`core::update`).
Die Adresse ist zwischengespeichert und zeigt nach dem Veröffentlichen unter Umständen noch eine
Weile die Vorversion (bei 0.1.3 länger als fünf Minuten, bei 0.1.4 sofort richtig). Frisch
fragen mit einem Anhang, z. B.
`…/releases/latest?x=1`. Die App fragt ohne Anhang und sieht die neue Version entsprechend
später – kein Fehler.
GitHub antwortet dort gelegentlich mit 504; dann später noch einmal fragen.

## Danach

- ROADMAP.md: den Punkt streichen, falls einer dafür stand.
- Alte Entwürfe, die nie veröffentlicht wurden, nur nach Rückfrage löschen
  (`gh release delete <tag> --yes` – nicht rückgängig zu machen, die Dateien sind weg).

## Fallen

- **Auto-Modus blockiert das Veröffentlichen** („Create Public Surface“), wenn die Erlaubnisregel
  fehlt, etwa auf einem anderen Rechner (so beim ersten Versuch mit 0.1.3). Nicht umgehen:
  Notizen an den Entwurf hängen (nicht öffentlich), dann den Nutzer bitten, die Regel
  einzutragen oder auf der Entwurfsseite „Publish release“ zu klicken. Den Befehl ohne
  vorangestelltes `cd …&&` ausführen: Bei einem zusammengesetzten Befehl muss jeder Teil
  erlaubt sein.
- **`gh run watch` endet manchmal vorzeitig mit Code 1**, während ein Job noch abschließt
  („Complete job“). Dann `gh run view <id> --json status,conclusion,jobs` erneut abfragen, bis
  `completed` dasteht – die Dateien hängen meist schon am Entwurf.
- Die Seite eines Entwurfs heißt `…/releases/tag/untagged-…`, nicht `…/tag/v<version>`. Das
  Anhängen der Notizen vergibt eine neue `untagged-…`-Adresse (so bei 0.1.4) – es bleibt
  derselbe Entwurf; `gh release list` zeigt, dass es nur einen gibt.
- `gh release edit` ohne `cd` aufrufen, mit `--repo Mottul/project_yt-dlp` – so greift die
  Erlaubnisregel.
- `gh release view --json isLatest` gibt es nicht; „latest“ zeigt die Weiterleitung oben.
- Ein Entwurf hat noch keinen Tag: `git tag` zeigt ihn erst nach dem Veröffentlichen.
- Der Workflow nimmt das neueste stabile Rust; ein roter Bau ohne eigene Änderung kann an einem
  neuen Clippy/rustfmt liegen (siehe CLAUDE.md) – erst `main` grün machen, dann neu starten.
