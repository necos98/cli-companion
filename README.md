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
npx tauri build       # build completa + bundle Windows (NSIS, firmato: servono i secret TAURI_SIGNING_*)
cargo check           # da src-tauri/: solo backend
```

## Versioni

La versione vive in 3 file da tenere sincronizzati manualmente
(il test `updater: versioni sincronizzate` fallisce se divergono):

- `package.json` → `version`
- `src-tauri/Cargo.toml` → `version`
- `src-tauri/tauri.conf.json` → `version`

Versione corrente: **0.12.0**.

## Release e aggiornamenti

- [docs/RELEASE.MD](docs/RELEASE.MD) — creare la repo GitHub, CI Windows,
  pubblicare una release (tag `v*` → Release automatica con installer firmato, latest.json + SHA256).
- [docs/UPDATER.MD](docs/UPDATER.MD) — come funziona l'updater firmato (`tauri-plugin-updater`),
  `latest.json` e chiavi di firma.
- [docs/DESIGN.MD](docs/DESIGN.MD) — note di design del progetto.
- [docs/BOARD.MD](docs/BOARD.MD) — il Tasks Manager: coda di task, consegna, scheduler, fasce orarie, tool MCP.

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

## Avvii

Ogni workspace ha gli avvii integrati, forniti dal registry dei coding agent:
**Claude Code** (`claude --dangerously-skip-permissions`) e un avvio per ogni **preset** di
agente, oggi **Task Intake** (lo stesso comando con la skill `/cli-companion-notify:task-intake`,
che trasforma le richieste in specifiche di task da proporre alla coda; al ripristino il tab
riprende la conversazione senza rilanciare la skill; ogni apertura è un nuovo tab, così puoi
tenere più chat Intake in parallelo). Non si modificano né si eliminano: si
possono solo scegliere come predefiniti. A questi si aggiungono gli avvii personalizzati del
workspace. Un `builtin:orchestrator` salvato come predefinito diventa Task Intake.

L'avvio predefinito (quello di “+ Terminale”) si sceglie in **Impostazioni → Generale → Avvii**
per tutti i workspace (shell vuota o un integrato) e in **Impostazioni → Workspace → Avvii**
per il singolo workspace (eredita, shell vuota, un integrato o un avvio personalizzato): il
workspace vince sul globale.

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
in `agents/mod.rs`. La UI non cambia. Per supportare anche le sessioni
automatiche (vedi sotto) implementa `automation_command(env_kind, req)`, che
restituisce il comando da digitare nella shell per avviare l'agente con prompt
iniziale, modalità permessi e modello, e dichiara nel plugin dell'agente il
server MCP `cli-companion` (`cli-companion --mcp`).

Profilo di input (facoltativo): se l'agente vuole tasti propri nel terminale,
implementa `input_profile()` con regole dichiarative `InputRule { event, env_kind,
send }` (`env_kind` = `windows` | `wsl` | nessuno per tutti). Oggi l'unico evento è
`image-paste` (incolla con appunti senza testo): al posto di Ctrl+V il terminale
invia `send` (Claude Code: Alt+V = `ESC v`, su Windows e WSL). Nuovi eventi: una
variante di `InputEvent` e la sua gestione in `TerminalView`. Il frontend legge i
profili all'avvio (`agent_profiles`) e li risolve con `resolveInputOverride`
(`src/lib/agents.ts`).

## Tasks Manager (sessioni automatiche)

Ogni workspace ha una **coda di task**. Tre attori che non parlano tra loro (contratto
completo in [docs/BOARD.MD](docs/BOARD.MD)):

- **Task Intake**: un tab di Claude Code a cui descrivi il lavoro. Esplora il repository,
  scrive una specifica completa e la **propone** (`propose_tasks`); tu la accetti o la scarti
  dal pannello Task. Non riceve eventi e non viene mai svegliato.
- **App**: lo scheduler deterministico avvia i task "Da fare" in ordine, rispettando
  dipendenze, slot, fasce orarie e modalità di consegna; prepara e pulisce i worktree e
  raccoglie l'esito.
- **Worker**: un tab marcato **AUTO** che legge il task (`get_task`), lavora e **consegna da
  solo** secondo la modalità del workspace, poi chiude con `complete_task`. Se ha bisogno di te
  chiede nel suo terminale; quando richiede attenzione o fallisce arriva una notifica di sistema.

Setup: reinstalla il plugin da **Impostazioni → Coding agent** (per ogni ambiente: Windows e ogni
distro WSL). Il plugin dichiara il server MCP `cli-companion` (`cli-companion --mcp`); fuori dai
terminali di cli-companion, o con l'app chiusa, non espone alcun tool.

Stati di un task: **Da fare** (`queued`), **In corso** (`running`, con "Richiede attenzione" se il
worker aspetta te), **Fatto** (`done`, già consegnato) e **Fallito** (`failed`, "Riprova" lo rimette
in fondo alla coda riusando worktree e branch). Al riavvio dell'app i task in corso risultano falliti.

### Consegna

In **Impostazioni → Workspace** scegli come arriva il lavoro (vale per tutti i repository):

| Modalità | Cosa fa il worker |
|---|---|
| `pr` | branch `task/<id>`, push, PR e merge con `gh` (squash, merge o rebase) |
| `local_merge` | branch, rebase e fast-forward della base locale |
| `push` | commit sul branch corrente, `pull --rebase` e push |
| `commit` (default) | solo commit sul branch corrente |

Con `pr` e `local_merge` puoi attivare i **worker in parallelo**: un git worktree per task
(`.worktrees/<task>`, creato e ripulito dall'app, con gli hook per repository), fino a
`max_parallel`. Altrimenti i worker lavorano nel checkout del repository, uno per repository.
`pr` richiede `gh` autenticato; `local_merge` in parallelo richiede il checkout sul branch base.
Ogni worker riceve inoltre le regole del workspace e uno standard di ingegneria rigoroso
(`src-tauri/src/automation/worker_guardrails.md`).

### Tool MCP

Worker: `get_task` (task, istruzioni, ambiente, passi di consegna, `previous_attempt` dopo un
retry) e `complete_task` (`completed` | `failed`, riassunto, `commit`, `pr_url`,
`files_changed`, `tests`, `follow_ups`; il tab si chiude dopo ~1,5 s).

Sessioni normali (Intake compreso, nel workspace del tab): `propose_tasks`, `list_tasks`,
`get_task_details`, `update_task`, `delete_task`.

Fine turno del worker (hook `Stop`): con lavoro in background resta `background`; se chiude il
turno senza `complete_task`, cli-companion risponde con un promemoria in inglese e la seconda
volta il worker diventa `idle` ("Richiede attenzione"). Tutto ciò che legge il modello (istruzioni,
promemoria, descrizioni dei tool, errori RPC) è in inglese; interfaccia e messaggi dell'app restano
in italiano.

Permessi e modello: **Impostazioni → Workspace → Worker** ("Permessi dei worker automatici",
default `accept-edits`; `plan` sola lettura, `default` chiede conferma per ogni azione, `bypass` nessun
permesso, con il dialogo di conferma del primo avvio di Claude Code se manca
`skipDangerousModePermissionPrompt: true` in `~/.claude/settings.json`) e "Modello dei worker"
(usato quando il task non indica un modello). I tab dei worker non vengono salvati nella sessione.

### Fasce orarie

In **Impostazioni → Generale → Fasce orarie** definisci una o più fasce (ora locale; 20:00–13:00
attraversa la mezzanotte). Sono solo un gate: i workspace con **Rispetta le fasce orarie** non
avviano task fuori fascia (quelli in corso finiscono). Con "Impedisci lo standby" il PC resta
sveglio finché c'è almeno un task in corso. Vedi [docs/BOARD.MD](docs/BOARD.MD).

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
  (menu “⋯” della card, Impostazioni → Workspace → Avvii);
- le variabili d'ambiente dell'updater ora iniziano con `CLI_COMPANION_`.

## Cartella dati (v0.9.2)

L'identifier dell'app è ora `cli-companion`: i dati stanno in
`%APPDATA%\cli-companion` invece di `%APPDATA%\com.jmagarelli.cli-companion`.

- al primo avvio la vecchia cartella viene copiata per intero nella nuova
  (quella vecchia resta intatta);
- il plugin dell'agente già installato punta ancora alla vecchia cartella:
  reinstallalo da **Impostazioni → Coding agent** (“Rimuovi”, poi “Installa”)
  per ogni ambiente, dopodiché puoi cancellare
  `%APPDATA%\com.jmagarelli.cli-companion` e
  `%LOCALAPPDATA%\com.jmagarelli.cli-companion`.

## Workspace con più repository (v0.10.0)

La board funziona anche quando la radice del workspace non è un repository git
ma contiene più repository:

- **repository per task**: il campo `repo` di un task (percorso relativo alla
  radice) fa lavorare il worker (e creare worktree e branch) dentro quel repository; il
  branch base si sceglie per repository (**Impostazioni → Repository**, poi `origin/HEAD` per
  `pr`, poi il branch corrente) e con `pr` si parte da `origin/<base>` dopo un `git fetch`;
- **hook per repository** (**Impostazioni → Repository**): file ignorati da
  copiare, comandi di setup dopo la creazione del worktree e di pulizia prima
  della rimozione (anche con `--force`), con `$WORKTREE`, `$CONTAINER_WORKTREE`
  e le altre variabili;
- **comandi pre-approvati** per i worker Claude Code (**Impostazioni → Worker**),
  passati con `--settings`; quando un worker chiede un permesso l'evento dice
  quale comando aspetta;
- **regole per i worker**, inviate a ogni worker.

Il plugin dell'agente cambia (nuovo hook dei permessi): reinstallalo da
**Impostazioni → Coding agent**.

## Aggiornamento automatico del plugin (v0.10.1)

Il plugin dell'agente è versionato con la release dell'app (`<versione>-h<hash>`)
e ogni installazione annota la versione nella sua cartella (`.installed-version`).
All'avvio, in background, l'app controlla gli ambienti in cui il plugin è
installato: se qualcuno è rimasto indietro (ad esempio dopo un aggiornamento
dell'app) chiede se aggiornarlo. Con “Aggiorna” il plugin viene reinstallato in
tutti quegli ambienti e l'app si riavvia, riprendendo le sessioni aperte; con
“Non ora” non succede nulla e la domanda torna al prossimo avvio. Non serve più
reinstallare il plugin a mano dopo un aggiornamento.
