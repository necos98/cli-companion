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

Versione corrente: **0.5.2**.

## Release e aggiornamenti

- [docs/RELEASE.MD](docs/RELEASE.MD) — creare la repo GitHub, CI Windows,
  pubblicare una release (tag `v*` → Release automatica con installer + SHA256).
- [docs/UPDATER.MD](docs/UPDATER.MD) — come l'updater interroga
  `releases/latest`, sceglie l'installer e verifica i checksum.
- [docs/DESIGN.MD](docs/DESIGN.MD) — note di design del progetto.

Stato: repo GitHub non ancora creata (`updater.config.json` ha
`owner = "REPLACE_ME_OWNER"`); segui `docs/RELEASE.MD` §1–2 per attivare
CI e updater.

## Terminali: gruppi, griglia e ripristino

I terminali sono raggruppati per workspace (una scheda per workspace in alto, con
numero di terminali e stato ❗/✓). I terminali di un gruppo stanno in una griglia
automatica: trascina l'intestazione di un pannello su un altro per scambiarli;
massimizza con il bottone, doppio click sull'intestazione o **Ctrl+Maiusc+M**.

Alla chiusura l'app salva schede, gruppi, layout e cronologia dei terminali
(`session.json` e `scrollback/` nella cartella dati) e al riavvio li ripristina:
i processi non sopravvivono, ma ogni terminale riparte con la cronologia di prima
e, per Claude Code, riprende la conversazione (`claude --resume`).

## Coding agent: notifiche di fine lavoro

cli-companion sa quando un agente avviato in uno dei suoi terminali ha finito
(ding + ✓ sul tab) o aspetta una risposta (suono diverso + ❗), e a finestra
non in primo piano mostra una notifica di Windows.

Per Claude Code: **Impostazioni → Coding agent**, poi “Installa” sulla riga
dell'ambiente (Windows e ogni distro WSL in cui usi `claude`: il plugin è
globale ma va installato per ambiente, perché ognuno ha la sua configurazione).
Il plugin (`cli-companion-notify@cli-companion-local`, scope utente) è generato
in una sottocartella `agent-plugins` della cartella dati dell'app (`%APPDATA%`) e non va cancellato a
mano: usa “Rimuovi”. “Prova notifica” verifica il canale. Gli agenti avviati
fuori da cli-companion vengono ignorati.

Aggiungere un nuovo agente: crea `src-tauri/src/agents/<nome>.rs` con una
struct che implementa `AgentProvider` (`detect`, `is_installed`, `install`,
`uninstall`; gli hook devono eseguire `cli-companion --notify <evento> <agente>`
con eventi `session-start`, `finished`, `needs-input`) e registrala in `PROVIDERS`
in `agents/mod.rs`. La UI non cambia.

Profilo di input (facoltativo): se l'agente vuole tasti propri nel terminale,
implementa `input_profile()` con regole dichiarative `InputRule { event, env_kind,
send }` (`env_kind` = `windows` | `wsl` | nessuno per tutti). Oggi l'unico evento è
`image-paste` (incolla con appunti senza testo): al posto di Ctrl+V il terminale
invia `send` (Claude Code: Alt+V = `ESC v`, su Windows e WSL). Nuovi eventi: una
variante di `InputEvent` e la sua gestione in `TerminalView`. Il frontend legge i
profili all'avvio (`agent_profiles`) e li risolve con `resolveInputOverride`
(`src/lib/agents.ts`).

## Migrazione da muse-companion (v0.4.0)

L'app si chiamava `muse-companion`. Con la rinomina in `cli-companion`:

- i workspace vengono copiati in automatico al primo avvio dalla
  vecchia cartella dati (`%APPDATA%\com.jmagarelli.muse-companion`);
- l'installer nuovo non sostituisce quello vecchio: disinstalla
  `muse-companion` da Impostazioni → App;
- l'integrazione Muse è stata rimossa: i vecchi plugin Muse già installati
  vengono ignorati senza errori (`--notify` con eventi sconosciuti esce con
  codice 0) e si possono togliere a mano con
  `muse plugins remove cli-companion-notify` (e `muse-companion-notify`);
- il comando di default globale non esiste più: gli avvii sono per workspace
  (menu “⋯” della card, Impostazioni → Avvii personalizzati);
- le variabili d'ambiente dell'updater ora iniziano con `CLI_COMPANION_`.
