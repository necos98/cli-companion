# Updater (tauri-plugin-updater)

L'app si aggiorna con il plugin ufficiale `tauri-plugin-updater` (Tauri v2):
aggiornamenti **firmati** (minisign), installer NSIS in modalità `passive` e
riavvio automatico. La CLI `--check-update` resta un controllo leggero a parte.

## Come funziona

1. All'avvio (e col pulsante «Controlla aggiornamenti» in Impostazioni →
   Aggiornamenti) la GUI chiama `check()`, che scarica
   `https://github.com/necos98/cli-companion/releases/latest/download/latest.json`
   e confronta la versione con quella installata.
2. Se c'è una versione più nuova compare il pallino su «Impostazioni» e il
   pannello mostra versione, data e note brevi.
3. «Aggiorna e riavvia» chiama `downloadAndInstall`: scarica il setup NSIS
   (barra di progresso), verifica la **firma** con la chiave pubblica in
   `tauri.conf.json` e lancia l'installer in modalità `passive` (solo barra di
   avanzamento, nessuna domanda). Su Windows l'app si chiude da sola quando
   parte l'installer e viene riaperta a fine installazione; `relaunch()` è il
   fallback se il processo è ancora vivo.
4. **MCP server**: al riavvio i server MCP delle sessioni agent aperte vengono
   riavviati (le sessioni perdono la connessione MCP finché l'app non riparte).
   La UI lo segnala accanto al pulsante.

## File coinvolti

| File | Ruolo |
|---|---|
| `src-tauri/tauri.conf.json` | `bundle.targets=["nsis"]`, `createUpdaterArtifacts`, `plugins.updater` (`pubkey`, `endpoints`, `windows.installMode=passive`) |
| `src-tauri/capabilities/default.json` | Permessi `updater:default` e `process:allow-restart` |
| `src-tauri/src/lib.rs` | Registra i plugin `updater` e `process` |
| `src/components/UpdateCheck.tsx` | Pannello GUI: check, progresso, installazione, riavvio |
| `src/App.tsx` | Check silenzioso all'avvio (pallino su Impostazioni) |
| `.github/workflows/build-windows.yml` | Build firmata + generazione `latest.json` + Release |
| `src-tauri/src/updater.rs` | Solo CLI `--check-update` (nessun download/installazione) |
| `updater.config.json` | `owner`/`repo` per la CLI (la GUI non lo usa) |
| `src/lib/updater.ts` | Confronto versioni, in parità con il Rust |
| `tests/updater.test.mjs` | Test: config, sync versioni, integrazione plugin, vettori |

## Chiave di firma

Gli update sono firmati con una chiave minisign generata con
`npx tauri signer generate`.

- **Chiave pubblica**: in `plugins.updater.pubkey` di `tauri.conf.json`
  (committata, è pubblica).
- **Chiave privata**: in locale `~/.tauri/cli-companion.key` (password in
  `~/.tauri/cli-companion.key.password`). **Non va mai committata** né copiata
  nel repository o nei log.
- **Secret GitHub** (Settings → Secrets and variables → Actions):
  - `TAURI_SIGNING_PRIVATE_KEY`: contenuto del file `.key`;
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: password della chiave.
- **Se la chiave privata va persa** (o la password) le installazioni esistenti
  non potranno più verificare nuovi aggiornamenti: servirebbe una nuova coppia
  di chiavi e un'installazione manuale una tantum su ogni macchina. Tenere un
  backup sicuro fuori dal repository.

## latest.json

Generato dalla CI (formato statico del plugin) e pubblicato come asset della
Release; l'endpoint `releases/latest/download/latest.json` punta sempre
all'ultima release.

```json
{
  "version": "0.8.1",
  "notes": "cli-companion v0.8.1",
  "pub_date": "2026-01-01T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contenuto di cli-companion_0.8.1_x64-setup.exe.sig>",
      "url": "https://github.com/necos98/cli-companion/releases/download/v0.8.1/cli-companion_0.8.1_x64-setup.exe"
    },
    "windows-x86_64-nsis": { "...": "stesso contenuto" }
  }
}
```

- `version` viene da `tauri.conf.json` (senza `v`); il tag della Release deve
  essere `v<version>`.
- Il file è UTF-8 **senza BOM**; se il `.sig` manca la CI fallisce.
- Asset di ogni Release: `*-setup.exe`, `*-setup.exe.sig`, `*-setup.exe.sha256`,
  `latest.json`, `cli-companion.exe` (+ `.sha256`). Niente più MSI: due tipi di
  installer creavano installazioni e collegamenti duplicati.

## CLI `--check-update` (leggera)

`cli-companion --check-update [--json]` interroga
`GET /repos/{owner}/{repo}/releases/latest` via `curl`, confronta il `tag_name`
con la versione locale e stampa l'esito. Non scarica né installa nulla.
Exit: `0` aggiornato, `2` update disponibile, `1` errore. Le build release
Windows non hanno console: usare `cargo run -- --check-update`.

- Config: `updater.config.json` (`owner`/`repo`/`apiUrl`), compilato nel binario;
  override via env `CLI_COMPANION_UPDATE_OWNER` / `_REPO` / `_API_URL`.
- Con `owner = REPLACE_ME_OWNER` il check risponde con un messaggio guida senza
  toccare la rete.
- Richiede `curl` nel PATH (`curl.exe` su Windows 10+); override
  `CLI_COMPANION_UPDATER_CURL`. Hook per test: `CLI_COMPANION_UPDATE_LATEST_URL`.

## Versioni da tenere sincronizzate

La versione locale vive in 3 file (il test `updater: versioni sincronizzate`
fallisce se divergono):

- `package.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/tauri.conf.json` → `version`
