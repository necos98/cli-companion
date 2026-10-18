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

Versione corrente: **0.6.4**.

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

## Sessioni automatiche (orchestratore)

Qualunque sessione Claude Code aperta in un tab di cli-companion può fare da
**orchestratore**: con i tool MCP di cli-companion avvia altre sessioni, i
**worker**, e ne segue il lavoro. Un worker è un tab marcato **AUTO** (badge e
stato nell'intestazione, tooltip con titolo del task, padre e ultimo report)
che comunica solo tramite tool: nessuno legge il suo terminale.

Setup: reinstalla il plugin da **Impostazioni → Coding agent** (per ogni
ambiente: Windows e ogni distro WSL). Il plugin ora dichiara anche il server MCP
`cli-companion` (`cli-companion --mcp`); fuori dai terminali di cli-companion, o
con l'app chiusa, non espone alcun tool.

Skill `orchestrate` (richiede la reinstallazione del plugin): il metodo
dell'orchestratore, caricato solo quando serve invece di occupare il contesto di ogni
sessione. Si invoca con `/cli-companion-notify:orchestrate` (o la carica il modello
quando gli chiedi di orchestrare dei task) e copre: pianificazione con l'utente, scelta
di task indipendenti, isolamento con `.worktrees/<id>`, modello di task autosufficiente
(Goal, Context, Constraints, Done when, Verify, Report), fine turno dopo gli spawn,
gestione di ogni evento, verifica e merge dei risultati uno alla volta, resoconto
finale. È generica: le regole di un progetto (build, gate, roadmap) vanno nel suo
`CLAUDE.md`. Per il worker non c'è una skill: le sue istruzioni arrivano sempre da
`get_task`.

Tool dell'orchestratore:

- `list_agents` — coding agent con supporto all'automazione.
- `spawn_session` — avvia un worker (`task`, opz. `title`, `agent`, `cwd`,
  `permission_mode`, `model`, `keep_open`). L'orchestratore lavora solo nel proprio
  workspace: il worker si apre nel workspace del suo tab (non si sceglie) e `cwd`, se
  indicato, deve essere assoluto, esistente e dentro la cartella del workspace (niente
  `..`); per i git worktree usa una sottocartella, es. `.worktrees/`.
- `list_sessions` — i propri worker con stato e ultimo report.
- `send_message` — scrive un messaggio nel prompt di un worker (es. la risposta a una richiesta d'aiuto).
- `close_session` — chiude il tab di un worker.
- `get_events` — restituisce e svuota la coda degli eventi dei worker. Non attende
  mai: lista vuota se non è successo nulla.

Tool del worker:

- `get_task` — legge task e istruzioni (da chiamare per primo).
- `report_fact` — segnala all'orchestratore un fatto utile.
- `request_help` — pone una domanda all'orchestratore; la risposta arriva come messaggio nel prompt.
- `complete_task` — chiude il lavoro con `status` (`completed`, `failed`, `blocked`) e riassunto;
  il tab si chiude dopo ~1,5 s, salvo `keep_open`.

Eventi ricevuti dall'orchestratore (`kind`): `started`, `fact`, `help_request`,
`completed`, `needs_input` (il worker aspetta un permesso), `stopped_without_report`
(il worker ha finito il turno senza `complete_task`), `exited` (shell uscita o tab chiuso).
Stati di un worker: `starting`, `running`, `background` (turno chiuso in attesa di
un comando in background: Claude Code riprende da solo), `needs_input`, `idle`,
`completed`, `failed`, `blocked`, `closed`.

Fine turno del worker (hook `Stop`): se ci sono task in background o cron di sessione
ancora attivi il worker passa a `background` senza avvisare l'orchestratore. Se invece
ha chiuso il turno senza `complete_task`, cli-companion risponde all'hook con un blocco
(`{"decision":"block","reason":...}`): il worker riceve un promemoria in inglese
(chiama `complete_task`, o `request_help` se bloccato) e prosegue. Solo se anche dopo
il promemoria si ferma senza riferire diventa `idle` e l'orchestratore riceve
`stopped_without_report`. La notifica `idle_prompt` di un worker viene ignorata.

Risveglio: l'orchestratore non resta in attesa dei worker. Dopo gli spawn chiude il
turno (le istruzioni MCP gli vietano attese, `sleep` e polling), così il suo tab resta
libero e non consuma richieste. Se arrivano eventi *azionabili* (`help_request`,
`completed`, `needs_input`, `stopped_without_report`, `exited`) mentre è fermo al
prompt, cli-companion digita nel suo prompt il messaggio “New events from your
automated sessions: call get_events, handle them, then end your turn”. `started` e
`fact` restano in coda e arrivano con la prossima `get_events`. Il risveglio avviene
una sola volta per gruppo di eventi azionabili nuovi, e mai se l'orchestratore sta
aspettando una risposta a un permesso. La notifica `idle_prompt` dell'orchestratore
(~60 s dopo la fine del turno) conferma che è al prompt e non blocca il risveglio.

Lingua: tutto ciò che legge il modello (prompt di avvio, istruzioni del worker, messaggio
di risveglio, promemoria dello Stop, descrizioni dei tool MCP ed errori RPC) è in
inglese, e `spawn_session` chiede all'orchestratore di scrivere `task` e `send_message`
in inglese. Interfaccia e messaggi dell'app restano in italiano.

Permessi del worker: li decide un'impostazione per workspace (Impostazioni ->
Avvii personalizzati -> "Permessi dei worker automatici", default `accept-edits`).
L'orchestratore può solo restringerli: `permission_mode` di `spawn_session`, se
assente, vale l'impostazione; se più permissivo del massimo la chiamata fallisce.
Ordine dal più restrittivo: `plan` (sola lettura), `default` (chiede conferma per ogni azione: il worker resta fermo in
attesa dell'utente), `accept-edits` (approva le modifiche ai file), `bypass`
(nessun permesso: usalo con cautela). Con `bypass` il primo avvio interattivo di Claude Code mostra una
finestra di conferma, che non compare solo se in `~/.claude/settings.json` c'è
`skipDangerousModePermissionPrompt: true`.

L'isolamento è compito dell'orchestratore: cli-companion non crea worktree. Per
lavori paralleli sullo stesso repository l'orchestratore crea (ad esempio) un
git worktree per worker e lo passa come `cwd`.

I tab dei worker non vengono salvati nella sessione né ripristinati al riavvio.

Limiti: 16 worker attivi per orchestratore, 32 terminali in totale, coda di 1000
eventi per orchestratore (i più vecchi vengono scartati).

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
