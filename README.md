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

Versione corrente: **0.7.0**.

## Release e aggiornamenti

- [docs/RELEASE.MD](docs/RELEASE.MD) — creare la repo GitHub, CI Windows,
  pubblicare una release (tag `v*` → Release automatica con installer firmato, latest.json + SHA256).
- [docs/UPDATER.MD](docs/UPDATER.MD) — come funziona l'updater firmato (`tauri-plugin-updater`),
  `latest.json` e chiavi di firma.
- [docs/DESIGN.MD](docs/DESIGN.MD) — note di design del progetto.
- [docs/BOARD.MD](docs/BOARD.MD) — la Board dei task: modello dati, scheduler, worktree, tool MCP.
- [docs/SCHEDULE.MD](docs/SCHEDULE.MD) — finestre di esecuzione, orchestratore automatico e workflow per workspace.

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

Ogni workspace ha due avvii integrati, forniti dal registry dei coding agent:
**Claude Code** (`claude --dangerously-skip-permissions`) e **Orchestratore** (lo stesso
comando con la skill `/cli-companion-notify:orchestrate`; al ripristino il tab riprende la
conversazione senza rilanciare la skill). Non si modificano né si eliminano: si possono solo
scegliere come predefiniti. A questi si aggiungono gli avvii personalizzati del workspace.

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

## Sessioni automatiche (orchestratore)

Qualunque sessione Claude Code aperta in un tab di cli-companion può fare da
**orchestratore**: con i tool MCP di cli-companion avvia altre sessioni, i
**worker**, e ne segue il lavoro. Un worker è un tab marcato **AUTO** (badge e
stato nell'intestazione, tooltip con titolo del task, padre e ultimo report)
che comunica solo tramite tool: nessuno legge il suo terminale. Gli orchestratori
(quello automatico e un tab dell'avvio "Orchestratore") hanno il badge **ORCH**, e ce n'è
al massimo uno per workspace.

Setup: reinstalla il plugin da **Impostazioni → Coding agent** (per ogni
ambiente: Windows e ogni distro WSL). Il plugin ora dichiara anche il server MCP
`cli-companion` (`cli-companion --mcp`); fuori dai terminali di cli-companion, o
con l'app chiusa, non espone alcun tool.

Skill `orchestrate` (richiede la reinstallazione del plugin): il metodo
dell'orchestratore, caricato solo quando serve invece di occupare il contesto di ogni
sessione. Si invoca con `/cli-companion-notify:orchestrate` (o la carica il modello
quando gli chiedi di orchestrare dei task) e copre: pianificazione con l'utente, scelta
di task con dipendenze e aree toccate (`touches`), modello di task autosufficiente
(Goal, Context, Constraints, Done when, Verify, Report), invio del piano alla Board, fine turno,
gestione di ogni evento, integrazione dei risultati (merge uno alla volta, test di regressione,
`set_task_status done`), resoconto finale. È generica: le regole di un progetto (build, gate, roadmap) vanno nel suo
`CLAUDE.md`. Per il worker non c'è una skill: le sue istruzioni arrivano sempre da
`get_task`.

### Board dei task

Ogni workspace ha una **Board**: un piano di task con dipendenze, salvato su disco e
mostrato come grafo (React Flow). Si apre dal pulsante **Board** sulla card del workspace
(con il numero di task attivi) o con **Ctrl+Maiusc+B** sul workspace attivo; non esiste nella
vista Globale. Dal grafo vedi lo stato di ogni task, apri i dettagli (testo, report, test,
diffstat) e puoi trattenere, riprovare, annullare o segnare come fatto un task, regolare i
worker in parallelo (`max_parallel`) e mettere in pausa la board.

Chi fa cosa:

- l'**orchestratore** (skill `orchestrate`) pianifica con te, crea i task con `submit_plan`
  (dipendenze, `touches`, isolamento, priorità), poi **integra**: controlla il lavoro, fa il
  merge dei branch dei task uno alla volta, risolve i conflitti, lancia i test di regressione e
  segna il task come fatto (`set_task_status done`);
- l'**app** fa lo scheduler: avvia i worker (solo con un orchestratore disponibile nel
  workspace: se serve e nessuno è vivo, ne apre uno lei, "on demand") quando un task è pronto (dipendenze fatte, nessun
  conflitto di aree, entro `max_parallel`), crea e pulisce i **git worktree**
  (`.worktrees/<task>`, branch `task/<task>`), raccoglie i report strutturati e sveglia
  l'orchestratore solo quando serve una decisione.

Setup del worktree: in **Impostazioni → Workspace → Worker** puoi indicare per workspace un
comando (es. `npm ci`) che il worker esegue per primo in un worktree nuovo. I task con
`isolation: none` lavorano nella cartella del workspace (solo lettura o modifiche minime). Al
riavvio dell'app i task in corso risultano falliti e si possono rilanciare. Dettagli in
[docs/BOARD.MD](docs/BOARD.MD).

Tool dell'orchestratore:

- `submit_plan`, `get_board` (vista compatta; `columns` per le colonne `done`/`cancelled`),
  `get_task_details` (dettaglio completo di un task), `update_task`, `set_task_status`,
  `set_board` — la Board
  (vedi sopra e `docs/BOARD.MD`).
- `list_agents` — coding agent con supporto all'automazione.
- `spawn_session` — avvia un worker ad-hoc, fuori dalla board (`task`, opz. `title`, `agent`, `cwd`,
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
- `complete_task` — chiude il lavoro con `status` (`completed`, `failed`, `blocked`) e riassunto,
  più campi strutturati facoltativi (`commit`, `files_changed`, `tests`, `follow_ups`); per un task
  di board lo porta in `review`/`failed`/`blocked`. Il tab si chiude dopo ~1,5 s, salvo `keep_open`.

Eventi ricevuti dall'orchestratore (`kind`): `started`, `fact`, `help_request`,
`completed`, `needs_input` (il worker aspetta un permesso), `stopped_without_report`
(il worker ha finito il turno senza `complete_task`), `exited` (shell uscita o tab chiuso) e, per la
board, `start_failed` (avvio del task fallito), `plan_completed` (piano concluso) e `user_action`
(azione dell'utente dalla Board).
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
`completed`, `needs_input`, `stopped_without_report`, `exited`, `start_failed`, `plan_completed`) mentre è fermo al
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
Workspace -> Worker -> "Permessi dei worker automatici", default `accept-edits`).
L'orchestratore può solo restringerli: `permission_mode` di `spawn_session`, se
assente, vale l'impostazione; se più permissivo del massimo la chiamata fallisce.
Ordine dal più restrittivo: `plan` (sola lettura), `default` (chiede conferma per ogni azione: il worker resta fermo in
attesa dell'utente), `accept-edits` (approva le modifiche ai file), `bypass`
(nessun permesso: usalo con cautela). Con `bypass` il primo avvio interattivo di Claude Code mostra una
finestra di conferma, che non compare solo se in `~/.claude/settings.json` c'è
`skipDangerousModePermissionPrompt: true`.

Modello del worker: l'orchestratore sceglie il modello di ogni task (`model`), di norma
`sonnet` per il lavoro ben definito e `opus` per design, modifiche rischiose o trasversali e
bug difficili, salvo regole diverse nel workflow del workspace. Se il task non indica un
modello vale "Modello dei worker" del workspace (Impostazioni, sotto i permessi), altrimenti
il modello predefinito dell'agente. Finché un task non è partito (in coda, fallito, bloccato o
annullato) il modello si cambia dal dettaglio del task nella Board, o l'orchestratore lo
cambia con `update_task`.

Isolamento: per i task della Board i worktree git li crea e li pulisce l'app. Per le sessioni
ad-hoc (`spawn_session`) resta compito dell'orchestratore: per lavori paralleli sullo stesso
repository crea (ad esempio) un git worktree per worker e lo passa come `cwd`.

I tab dei worker non vengono salvati nella sessione né ripristinati al riavvio.

Limiti: 16 worker attivi per orchestratore, 32 terminali in totale, coda di 1000
eventi per orchestratore (i più vecchi vengono scartati).

### Esecuzione automatica e workflow

Per far lavorare i workspace da soli (ad esempio di notte, evitando le ore di punta) puoi
definire delle **fasce orarie** e un **orchestratore automatico** per workspace. Dettagli in
[docs/SCHEDULE.MD](docs/SCHEDULE.MD).

- **Fasce orarie**: in **Impostazioni → Generale → Fasce orarie** attivi la funzione e aggiungi
  una o più fasce (ora di inizio e fine, ora locale; una fascia come 20:00–13:00 attraversa la
  mezzanotte). Vedi se la finestra è aperta e quando cambia.
- **Orchestratore automatico**: nelle impostazioni del workspace lo attivi scegliendo agente e
  modello. I permessi sono il tetto dei worker del workspace: per lavorare senza nessuno
  davanti serve un livello che non apra dialoghi di conferma.
- **All'apertura della finestra** la board riparte e, se c'è lavoro (task in coda o da
  integrare) e nessun orchestratore è già attivo, l'app apre un orchestratore automatico in un
  nuovo tab. Non duplica quello aperto da te.
- **Alla chiusura** non parte più nessun task: i worker in corso finiscono, l'orchestratore
  integra quello che completa, scrive il resoconto e il suo tab si chiude. Un orchestratore
  aperto da te non viene mai chiuso. Aprire o chiudere la finestra non cambia la pausa della board.
- **Standby**: con "Impedisci lo standby" il PC non va in sospensione finché un workspace
  automatico ha la finestra aperta con lavoro da fare o task in corso.
- **Workflow**: ogni workspace può avere un `workflow.md` che dà all'orchestratore identità e
  metodo su misura (branch e merge, comandi di verifica, dimensione dei task, parallelismo e
  modelli, aree da non toccare, cosa decidere da solo, cosa fare senza utente, formato del
  report). L'orchestratore lo legge con `get_workflow` prima di pianificare e ha la precedenza
  sul suo metodo generico (mai su sicurezza: niente push o azioni distruttive non consentite).
  Lo crei con il wizard, `/cli-companion-notify:workflow` in un terminale del workspace oppure
  il pulsante **Crea con il wizard** nelle impostazioni del workspace (richiede Claude Code
  con il plugin): ti intervista nella tua lingua proponendo default ricavati dal repository. Dalle
  impostazioni puoi anche leggerlo e modificarlo a mano.

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
  radice) fa creare worktree e branch dentro quel repository; il branch base si
  sceglie per repository (`base_branches` della board, **Impostazioni →
  Repository**, `origin/HEAD`) e si parte da `origin/<base>` dopo un `git fetch`;
- **branch di integrazione**: con `integration_branch` (es. `jm-{date}`) l'app
  crea per ogni repository il worktree `.worktrees/integration` e i task partono
  da lì; il push resta all'orchestratore;
- **hook per repository** (**Impostazioni → Repository**): file ignorati da
  copiare, comandi di setup dopo la creazione del worktree e di pulizia prima
  della rimozione (anche con `--force`), con `$WORKTREE`, `$CONTAINER_WORKTREE`
  e le altre variabili;
- **comandi pre-approvati** per i worker Claude Code (**Impostazioni → Worker**),
  passati con `--settings`; quando un worker chiede un permesso l'evento dice
  quale comando aspetta;
- **regole per i worker**, inviate a ogni worker; **nota per l'orchestratore**
  mai inviata al worker; un task in coda può essere chiuso come completato
  fuori board (con una nota).

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
