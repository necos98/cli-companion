# Release su GitHub (repo + CI Windows + updater)

Questa guida copre, nell'ordine: (1) creare il repository GitHub mancante,
(2) come la CI compila l'eseguibile Windows, (3) come l'updater (firmato) consuma
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

Il push del tag `v0.1.0` fa partire la CI che compila, firma e crea
automaticamente la Release `v0.1.0` con installer firmato, `latest.json` e checksum.
Prima della prima release firmata servono i due secret GitHub
`TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
(vedi [docs/UPDATER.MD](UPDATER.MD)): la chiave privata non va mai committata.
Controlla l'avanzamento con `gh run watch` oppure nella tab Actions.

Release successive: aggiorna le 3 versioni → commit → push →
`git tag vX.Y.Z` → `git push origin vX.Y.Z`.
Il `tag_name` deve corrispondere alla versione nei file (`v` iniziale
opzionale per l'updater, ma la CI crea la Release solo su tag `v*`).

## 3. Cosa fa la CI (`.github/workflows/build-windows.yml`)

Runner `windows-latest`, build system reale del progetto:

0. cache Rust (`Swatinem/rust-cache`, `shared-key: release`): le build su tag
   la ripristinano ma non la salvano (`save-if` solo su `main`). Le cache dei
   run su tag non sono visibili ad altri tag, quindi la cache viene scaldata
   da `.github/workflows/cache-rust-main.yml` (push su `main` che tocca
   `Cargo.toml`/`Cargo.lock`/workflow, o avvio manuale): fa solo
   `cargo build --release` con dist segnaposto, senza Release ne' artifact;
1. `npm ci` + `npm test` (`node --test tests/*.test.mjs`);
2. `npx tauri build --ci` con i secret di firma → prima `npm run build`
   (`tsc && vite build`), poi backend Rust in release + bundle NSIS
   (`targets=["nsis"]`; NSIS e' scaricato da tauri-bundler automaticamente,
   niente piu' WiX/MSI) e `<setup>.exe.sig`;
3. genera `latest.json` (manifest del plugin updater, UTF-8 senza BOM; fallisce
   se manca la firma);
4. genera `<asset>.sha256` (singolo hash) per exe e setup;
5. carica exe + setup + `.sig` + `latest.json` + checksum come artifact CI;
6. **solo su tag `v*`**: `gh release create` con gli stessi file.

Asset pubblicati in ogni Release:

| Asset | Note |
|---|---|
| `cli-companion_<ver>_x64-setup.exe` | Installer NSIS (quello scaricato dall'updater) |
| `cli-companion_<ver>_x64-setup.exe.sig` | Firma minisign dell'installer |
| `latest.json` | Manifest letto da `tauri-plugin-updater` |
| `cli-companion.exe` | Eseguibile portable (fallback) |
| `<asset>.sha256` | Checksum per download manuali (non usato dall'updater) |

## 4. Come l'updater consuma le release

Flusso della GUI (`tauri-plugin-updater`, `UpdateCheck.tsx`):

1. `check()` scarica
   `https://github.com/necos98/cli-companion/releases/latest/download/latest.json`
   (endpoint in `src-tauri/tauri.conf.json`) e confronta `version` con
   quella installata;
2. se e' piu' nuova, `downloadAndInstall` scarica il setup NSIS e verifica la
   firma con la `pubkey` in `tauri.conf.json` (firma errata = update rifiutato);
3. l'installer parte in modalita' `passive`, l'app si chiude e si riapre da
   sola. I server MCP delle sessioni agent aperte vengono riavviati.

La CLI `--check-update` (`src-tauri/src/updater.rs`) fa solo un controllo
leggero su `releases/latest` via curl, senza scaricare nulla.

Comandi utili senza GUI:

```powershell
cargo run -p cli-companion -- --check-update   # exit 0 ok / 2 disponibile / 1 errore
cargo run -p cli-companion -- --check-update --json
```

Finche' `owner` resta `REPLACE_ME_OWNER`, la CLI risponde con un messaggio
guida e non tocca la rete.

## 5. Checklist release

- [ ] `updater.config.json`: `owner`/`repo` reali (mai `REPLACE_ME_OWNER`)
- [ ] versioni sincrone: `package.json`, `src-tauri/Cargo.toml`,
      `src-tauri/tauri.conf.json` (il test `updater: versioni
      sincronizzate` fallisce se divergono)
- [ ] `npm test` verde in locale
- [ ] tag `vX.Y.Z` creato e pushato, Release apparsa su GitHub
- [ ] secret `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` presenti su GitHub
- [ ] asset attesi presenti (setup + `.sig` + `latest.json` + exe + `.sha256`)
- [ ] `latest.json` raggiungibile da `releases/latest/download/latest.json`
- [ ] smoke test: da un'installazione precedente, «Controlla aggiornamenti»
      rileva la nuova versione e «Aggiorna e riavvia» la installa
