# cli-companion

Workspace companion con terminali integrati Windows + WSL.
Frontend TypeScript + React 18 + Vite + Tailwind, backend Rust su Tauri v2.

## Sviluppo

Prerequisiti: Node LTS, Rust stable.

```powershell
npm ci
npm run tauri dev     # app + hot reload (frontend su http://localhost:1420)
```

Altri comandi:

```powershell
npm test              # node --test tests/*.test.mjs
npm run build         # tsc && vite build (solo frontend, in dist/)
npx tauri build       # build completa + bundle Windows (MSI + NSIS)
cargo check           # da src-tauri/: solo backend
```

## Versioni

La versione vive in 3 file da tenere sincronizzati manualmente
(il test `updater: versioni sincronizzate` fallisce se divergono):

- `package.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/tauri.conf.json` → `version`

Versione corrente: **0.1.1**.

## Release e aggiornamenti

- [docs/RELEASE.MD](docs/RELEASE.MD) — creare la repo GitHub, CI Windows,
  pubblicare una release (tag `v*` → Release automatica con installer + SHA256).
- [docs/UPDATER.MD](docs/UPDATER.MD) — come l'updater interroga
  `releases/latest`, sceglie l'installer e verifica i checksum.
- [docs/DESIGN.MD](docs/DESIGN.MD) — note di design del progetto.

Stato: repo GitHub non ancora creata (`updater.config.json` ha
`owner = "REPLACE_ME_OWNER"`); segui `docs/RELEASE.MD` §1–2 per attivare
CI e updater.

## Migrazione da muse-companion (v0.4.0)

L'app si chiamava `muse-companion`. Con la rinomina in `cli-companion`:

- workspace e impostazioni vengono copiati in automatico al primo avvio dalla
  vecchia cartella dati (`%APPDATA%\com.jmagarelli.muse-companion`);
- l'installer nuovo non sostituisce quello vecchio: disinstalla
  `muse-companion` da Impostazioni → App;
- il plugin Muse va reinstallato dal tab Muse (rimuove anche quello vecchio,
  i cui hook puntano a `muse-companion.exe`);
- le variabili d'ambiente dell'updater ora iniziano con `CLI_COMPANION_`.
