# Updater (GitHub Releases)

Updater leggero, senza nuove dipendenze Cargo: il backend interroga
`GET /repos/{owner}/{repo}/releases/latest` via `curl`, confronta il `tag_name`
con la versione locale e — se disponibile — scarica l'installer, ne verifica
dimensione ed eventuale checksum SHA256, poi lo avvia (o ne restituisce il
percorso per l'installazione manuale).

## File coinvolti

| File | Ruolo |
|---|---|
| `updater.config.json` | Single source of truth per `owner`/`repo`/`assetPatterns` |
| `src-tauri/src/updater.rs` | Check, download, verifica, install; comandi Tauri; unit test |
| `src-tauri/src/lib.rs` | Registra i 4 comandi updater |
| `src-tauri/src/main.rs` | Flag CLI `--check-update` / `--version` / `--help` |
| `src/lib/updater.ts` | Tipi + confronto versioni / scelta asset (parità col Rust) |
| `src/lib/tauri.ts` | `invoke` dei 4 comandi |
| `src/components/UpdateCheck.tsx` | Pannello GUI (footer sidebar + auto-check silenzioso) |
| `tests/updater.test.mjs` | Test node: config, sync versioni, integrazione, vettori |

## Repository non ancora esistente

Finché `owner` vale `REPLACE_ME_OWNER` l'updater è in attesa:

- `get_update_config` → `configured: false` + messaggio guida;
- `check_update` / `--check-update` → errore chiaro, nessun accesso rete;
- la GUI non mostra allarmi all'avvio; il messaggio compare solo premendo
  «Controlla aggiornamenti».

Per attivare: creare il repo GitHub, impostare `owner`/`repo` in
`updater.config.json` (o le env `CLI_COMPANION_UPDATE_OWNER` /
`CLI_COMPANION_UPDATE_REPO`), pubblicare una release con `tag_name` tipo
`v0.2.0` e allegare l'installer (`.msi` / `setup.exe` consigliati su Windows).

## Convenzioni release

- `tag_name` = versione (`v` iniziale opzionale); confronto semver tollerante
  (`1.10 > 1.9`, `1.2 == 1.2.0`, release > prerelease).
- Asset installer scelto via `assetPatterns` (substring, case-insensitive);
  fallback: primo asset della release.
- Checksum: se la release contiene `<asset>.sha256` (singolo hash) o un file
  `*SHA256SUMS*`, il digest viene verificato; su **mismatch il file è eliminato
  e l'installazione corrente resta intatta** (rollback).
- Su qualsiasi failure (rete, download, file vuoto/mancante) i parziali vengono
  rimossi e il messaggio spiega come riprovare; l'app in esecuzione non è mai
  toccata a caldo (su Windows l'exe running è lockato).

## Sicurezza e limiti noti

- Nessuna firma crittografica dell'installer (solo SHA256 se pubblicato):
  prima di distribuire pubblicamente valutare `tauri-plugin-updater` con chiavi
  di firma (updater firmato). Vedi note scout in `docs/DESIGN.MD` se presenti.
- Richiede `curl` nel PATH (`curl.exe` preinstallato su Windows 10+);
  override via `CLI_COMPANION_UPDATER_CURL`.
- Hook per test/mock: `CLI_COMPANION_UPDATE_LATEST_URL` punta a un JSON di
  release arbitrario (anche `file://`).
- CLI `--check-update`: exit `0` aggiornato, `2` update disponibile, `1` errore.
  Le build release Windows non hanno console: usare `cargo run -- --check-update`
  oppure la GUI.

## Versioni da tenere sincronizzate

La versione locale vive in 3 file (test `updater: versioni sincronizzate`
fallisce se divergono):

- `package.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/tauri.conf.json` → `version`
