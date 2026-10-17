# Release su GitHub (repo + CI Windows + updater)

Questa guida copre, nell'ordine: (1) creare il repository GitHub mancante,
(2) come la CI compila l'eseguibile Windows, (3) come l'updater consuma
le release. Per il dettaglio del meccanismo di update vedi
[docs/UPDATER.MD](UPDATER.MD).

Stato attuale: il progetto **non ha ancora un remote GitHub**
(`updater.config.json` ha `owner = "REPLACE_ME_OWNER"`).

## Prerequisiti

- `git` e [GitHub CLI](https://cli.github.com/) installati;
- `gh auth login` (o `gh auth status`) con un account che puo' creare repo;
- in locale: Node LTS + Rust stable (stessi della CI).

## 1. Creare la repo GitHub e pushare

Esegui da `F:\altre_robe\cli-companion` (PowerShell).
Sostituisci `OWNER` con il tuo username GitHub (es. `necos98`):

```powershell
# 1a. (solo se non ancora fatto) inizializza il repo locale e committa
git init
git add .
git commit -m "Initial commit: cli-companion v0.1.0"

# 1b. crea la repo GitHub e collega il remote (nessun push forzato)
gh repo create OWNER/cli-companion --public --source=. --remote=origin

# 1c. push del branch principale
git branch -M main
git push -u origin main
```

Variante in un solo comando (se il commit locale esiste gia'):

```powershell
gh repo create OWNER/cli-companion --public --source=. --remote=origin --push
```

Verifica: `gh repo view OWNER/cli-companion --web`.

## 2. Prima release (attiva l'updater)

ATTENZIONE — ordine importante: `updater.config.json` e' compilato
**dentro** il binario Rust (`include_str!`), quindi il binario della
release deve essere compilato DOPO aver impostato il vero owner.

```powershell
# 2a. imposta il vero owner in updater.config.json
#     ("owner": "OWNER", "repo": "cli-companion"), poi committa e pusha:
git add updater.config.json
git commit -m "updater: set GitHub owner"
git push

# 2b. allinea le 3 versioni (devono essere identiche, vedi sotto) e tagga:
#     package.json -> version
#     src-tauri/Cargo.toml -> version
#     src-tauri/tauri.conf.json -> version
git tag v0.1.0
git push origin v0.1.0
```

Il push del tag `v0.1.0` fa partire la CI che compila e crea
automaticamente la Release `v0.1.0` con installer + checksum.
Controlla l'avanzamento con `gh run watch` oppure nella tab Actions.

Release successive: aggiorna le 3 versioni → commit → push →
`git tag vX.Y.Z` → `git push origin vX.Y.Z`.
Il `tag_name` deve corrispondere alla versione nei file (`v` iniziale
opzionale per l'updater, ma la CI crea la Release solo su tag `v*`).

## 3. Cosa fa la CI (`.github/workflows/build-windows.yml`)

Runner `windows-latest`, build system reale del progetto:

1. `npm ci` + `npm test` (`node --test tests/*.test.mjs`);
2. installa WiX 3.14 via choco (serve per l'MSI; NSIS e' scaricato
   da tauri-bundler automaticamente);
3. `npx tauri build --ci` → prima `npm run build` (`tsc && vite build`),
   poi backend Rust in release + bundle (`targets=all` = MSI + NSIS);
4. genera `<asset>.sha256` (singolo hash) per ogni asset;
5. carica exe + MSI + NSIS-setup + checksum come artifact CI;
6. **solo su tag `v*`**: `gh release create` con gli stessi file.

Asset pubblicati in ogni Release:

| Asset | Note |
|---|---|
| `cli-companion_<ver>_x64.msi` | Installer MSI (preferito dall'updater) |
| `cli-companion_<ver>_x64-setup.exe` | Installer NSIS |
| `cli-companion.exe` | Eseguibile portable (fallback) |
| `<asset>.sha256` ciascuno | Checksum verificato dall'updater |

## 4. Come l'updater consuma le release

Flusso (backend `src-tauri/src/updater.rs`, GUI `UpdateCheck.tsx`):

1. `GET https://api.github.com/repos/{owner}/{repo}/releases/latest`
   con `owner`/`repo` da `updater.config.json`
   (override: env `CLI_COMPANION_UPDATE_OWNER` / `_REPO` / `_API_URL`);
2. confronta `tag_name` (es. `v0.2.0`) con la versione locale
   (semver tollerante: `1.10 > 1.9`, `1.2 == 1.2.0`);
3. se il tag e' piu' nuovo, sceglie l'asset via `assetPatterns`
   (`.msi` prima di `setup.exe`, poi `.exe`; fallback: primo asset);
4. scarica in `%TEMP%`, verifica dimensione + SHA256 se `<asset>.sha256`
   (o `*SHA256SUMS*`) e' presente nella release; su mismatch elimina
   il file (rollback) e non tocca l'installazione;
5. installa avviando l'installer scaricato (detached), oppure ne mostra
   il percorso per installazione manuale.

Comandi utili senza GUI:

```powershell
cargo run -p cli-companion -- --check-update   # exit 0 ok / 2 disponibile / 1 errore
cargo run -p cli-companion -- --check-update --json
```

Finche' `owner` resta `REPLACE_ME_OWNER`, check e download rispondono
con un messaggio guida e non toccano la rete.

## 5. Checklist release

- [ ] `updater.config.json`: `owner`/`repo` reali (mai `REPLACE_ME_OWNER`)
- [ ] versioni sincrone: `package.json`, `src-tauri/Cargo.toml`,
      `src-tauri/tauri.conf.json` (il test `updater: versioni
      sincronizzate` fallisce se divergono)
- [ ] `npm test` verde in locale
- [ ] tag `vX.Y.Z` creato e pushato, Release apparsa su GitHub
- [ ] asset attesi presenti (MSI + NSIS + exe + `.sha256`)
- [ ] smoke test: `--check-update` dalla versione precedente rileva la nuova
