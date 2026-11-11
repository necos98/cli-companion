# cli-companion — Design

App desktop (Windows) per gestire cartelle di lavoro ("workspace") locali e WSL2,
aprirci dentro terminali integrati a tab e far lavorare dei coding agent su una coda di task.

Documenti collegati: [BOARD.MD](BOARD.MD) (contratto del Tasks Manager: board, consegna, strumenti MCP,
E2E), [RELEASE.MD](RELEASE.MD), [UPDATER.MD](UPDATER.MD), la guida
[wiki/integrare-un-coding-agent.md](wiki/integrare-un-coding-agent.md) e, per chi sviluppa sul
progetto, [CONTRIBUTING.md](../CONTRIBUTING.md).

## Stack

- **Tauri v2** + **Rust** (backend), **React 18 + TypeScript + Vite** (frontend)
- UI: **shadcn/ui** (componenti in `src/components/ui/`, stile new-york) +
  **Tailwind CSS v4** (tema dark in `src/index.css`, alias `@/` -> `src/`).
  Nota storica: la prima versione usava gluestack-ui v1 + react-native-web,
  rimossi perché incompatibili con il dev server Vite 8 (sorgenti .jsx/.tsx
  grezzi non pre-bundlati -> schermata bianca). Lo vieta la regola ESLint `no-restricted-imports`
  (`eslint.config.js`).
- Terminali: **xterm.js** (`@xterm/xterm` + `@xterm/addon-fit`) nel frontend,
  **portable-pty** (ConPTY su Windows) nel backend. Il ConPTY è quello distribuito con l'app
  (`conpty.dll` + `OpenConsole.exe` accanto all'eseguibile, vedi `src-tauri/windows/conpty/README.md`), non quello
  di sistema: su Windows 10 quest'ultimo ridisegna male le interfacce a schermo intero dei coding agent
- Persistenza: **JSON** nella app-data dir di Tauri. Tutti i file strutturati (`workspaces.json`,
  `launch_settings.json`, `bots.json`, `hub_settings.json`, `schedule.json`, `boards/<workspace_id>.json`,
  `threads/<workspace_id>.json`, `threads/general.json`) passano da `store/`
  (busta `{schema_version, data}`, migrazioni, backup datati, avvisi). Fanno eccezione `session.json` e
  `scrollback/<tab_id>.txt` (blob del frontend, `session.rs`) e l'archivio dei task conclusi
  (`boards/archive/<workspace_id>.jsonl`, una riga per task), scritti con `write_atomic` / in coda al file.
- Log: **tauri-plugin-log** (file nella cartella dei log dell'app, vedi `store/` più sotto)
- Board dei task: **React Flow** (`@xyflow/react`) + **dagre** (`@dagrejs/dagre`) per il layout a grafo
- Contratto Rust ↔ TypeScript: **ts-rs** (vedi "Tipi TypeScript generati")

## Architettura

```text
+----------------------+  invoke (comandi)    +---------------------------+
|  React (WebView)     | -------------------> |  Rust (Tauri)             |
|  - stores (stato)    | <-- Channel<PtyEvent>|  - workspaces/  board/    |
|  - app/ (azioni)     |     uno per sessione |  - automation/  agents/   |
|  - hooks/ componenti |     pty              |  - pty/  ipc/  store/     |
|  - xterm.js          | <-- eventi (emit) ---|  - platform/  validate    |
+----------------------+                      +-------------+-------------+
                                                            |
                                          +-----------------+-----------------+
                                          |                                   |
                                  ConPTY: powershell/cmd              wsl.exe -d <distro>
                                  cwd = cartella                      --cd <linux-path>
```

Frontend e backend comunicano solo via comandi Tauri (`invoke`) per le azioni, un `Channel` per lo
stream di output di ogni terminale ed eventi Tauri (`emit`/`listen`) per il resto (`board-update`,
`agent-event`, `automation-*`, `store-warning`, ...; l'elenco tipizzato è `TauriEvents` in
`src/lib/tauri.ts`). Lo stesso eseguibile fa anche da client degli hook dei coding agent
(`cli-companion --notify`) e da server MCP (`cli-companion --mcp`): vedi `ipc/` e `mcp`.

### Tipi TypeScript generati (ts-rs)

I tipi dei DTO che attraversano `invoke`/`emit` sono generati dal Rust con la crate `ts-rs` e
committati in `src/bindings/` (un file per tipo, cartella piatta). Non si modificano a mano.

- Rigenerare: `npm run bindings` (`scripts/generate-bindings.mjs`): svuota `src/bindings/` e
  lancia `cargo test export_bindings`; ts-rs scrive un file per ogni tipo esportato. La cartella
  e il tipo degli interi a 64 bit (`number`) sono in `src-tauri/.cargo/config.toml`.
- Aggiungere un DTO: `use ts_rs::TS;` e sul tipo `#[derive(TS)]` + `#[ts(export)]` (accanto a
  `Serialize`/`Deserialize`, con gli stessi attributi `serde`), poi `npm run bindings` e commit
  dei file generati.
- La CI (passo "Bindings TypeScript aggiornati") rigenera e fallisce se `git diff src/bindings`
  non e' vuoto: DTO Rust e bindings devono andare nello stesso commit.
- I file generati sono esclusi da Prettier (`.prettierignore`) e da ESLint, e hanno `eol=lf`
  (`.gitattributes`).
- I moduli di `src/lib` importano i bindings **solo** con `import type`: l'import sparisce alla
  compilazione e i moduli restano importabili da `node --test` senza passare da un bundler
  (lo verifica `tests/bindings.test.mjs`, insieme al divieto di copie scritte a mano di un DTO).
- `src/lib/tauri.ts` riesporta i bindings con i nomi usati dall'interfaccia e contiene l'oggetto `api`,
  un wrapper tipizzato per ogni comando (`invoke<T>("nome_comando", { chiaviCamelCase })`).
  `tests/tauri-args.test.mjs` confronta gli argomenti di ogni comando registrato in `lib.rs` con le chiavi
  inviate da `api`.

## Mappa dei moduli del backend (`src-tauri/src/`)

Il crate è una libreria (`cli_companion_lib`, `lib.rs`) più un binario sottile (`main.rs`) che, prima di
avviare la GUI, smista i rami da riga di comando: `--help`, `--version`, `--notify <evento> [agente]`
(client degli hook, `ipc::run_notify`), `--mcp` (server MCP su stdio, `mcp::run_stdio`) e
`--check-update` (`updater`). Ogni ramo esce senza aprire finestre.

### Livelli e dipendenze

```text
 5  lib.rs · main.rs      composizione: plugin, stati gestiti, `setup`, comandi, rami da riga di comando
 4  automation   mcp   connectors   orchestrazione dei worker (registro, scheduler, git, E2E) / shim MCP verso l'app / connettori esterni dell'HUB
 3  pty          ipc      canali con terminali e agenti
 2  board  workspaces  hub     dominio: coda dei task (pura) / cartelle di lavoro / thread dell'Agents HUB
 1  agents                integrazioni dei coding agent: contratti + registro + `claude_code/`
 0  platform  validate  store  secrets  wsl  net  ttl_cache  schedule  logging  ...   fondamenta
```

I numeri non descrivono ogni `use`, ma l'ordine in cui conviene pensare ai moduli: una freccia verso
l'alto è l'eccezione e va giustificata. Le dipendenze reali (imports `crate::…` del codice di produzione,
test esclusi) sono queste:

| Modulo | Dipende da | Non dipende da |
|---|---|---|
| `platform`, `validate`, `store`, `secrets` | nessun altro modulo dell'app (`store` solo da Tauri per `AppHandle` e dagli eventi; `secrets` solo dal crate `keyring`) | tutto il resto |
| `agents` | `platform`, `validate`, `wsl`, `net`, `ttl_cache` (+ `launch_settings` nei soli comandi Tauri) | `workspaces`, `board`, `automation`, `ipc` |
| `workspaces` | `agents` (tipi dei contratti), `validate`, `platform`, `store`, `wsl` (+ `automation` nei soli comandi, per riallineare lo scheduler) | `board`, `ipc`, `pty` |
| `board` | `workspaces` (`Delivery`, `Workspace`, `SettingsPatch`), `agents` (agente predefinito), `validate`, `store` | `automation`, `ipc`, `pty`, `mcp`, `tauri` |
| `ipc` | `agents` (contratto `HookEvents`, tipo `HookEventKind` e ricerca nel registro con `registry::find`), `platform` | `automation`, `board`, `workspaces` |
| `pty` | `ipc` (nomi delle variabili e `is_valid_tab_id`), `platform`, `wsl`, `workspaces`, `automation` (credenziale di sessione e `user_input`) | `board` |
| `automation` | `board`, `workspaces`, `agents`, `ipc` (tipi e trait), `platform`, `validate`, `wsl`, `schedule`, `user_skills`, `store`, `awake`, `pty` (solo `workspace_of_tab`, in `TauriHost`), `hub` (solo `board_published`, in `TauriHost`; `lib.rs` registra `board_views` come sorgente di `hub::set_board_source` e `queue_hub_plan_task` / `decide_hub_proposal` / `answer_question` come azioni di `hub::set_board_actions`) | — |
| `mcp` | `automation` (`Role`, `lock`, nomi dei metodi), `agents`, `workspaces`, `ipc` (client RPC) | `tauri` |
| `hub` | `store`, `secrets` (API key del provider nel portachiavi), `bots` (validazione delle immagini dei segretari), `board` (`valid_id` e `BoardView` per gli eventi della board), `validate` (+ `workspaces` nei comandi e negli eventi della board, per sapere se il canale di un workspace esiste, anche nei canali di un segretario; le viste correnti delle board all'accensione e per il tool `board_status` dei segretari arrivano da una sorgente `BoardSource` registrata da `lib.rs`, e le azioni dei segretari sulla board, task di pianificazione, decisione di una proposta e risposta a una domanda di un worker, da `BoardActions` registrate allo stesso modo); crate esterni `async-openai` e `reqwest` solo nel client del modello (`hub/engine/openai.rs`) | `automation`, `agents`, `ipc`, `pty`, `mcp` |
| `connectors` | `hub` (solo la sua API pubblica: `ingress`, `send`, `attachments`, i tipi del modello, `HubNotice`, `HubLifecycle`, `thread_exists`, `Channel`), `store`, `secrets` (i segreti di ogni istanza nel portachiavi), `workspaces` (solo `WorkspaceStore`, nei comandi delle regole, per sapere se il canale esiste); crate esterni `reqwest` (solo il client del contesto, `connectors/runtime/http.rs`) e `tokio` (`time`, `sync`, `macros`) | `automation`, `agents`, `board`, `ipc`, `pty`, `mcp`; `hub` non dipende da `connectors` (gli arrivano solo il `NoticeSink` e il `HubLifecycle` registrati da `lib.rs`) |

Le regole che questa tabella racchiude, e come sono garantite:

1. **Il core non nomina un coding agent.** Fuori da `agents/<integrazione>/` un agente si raggiunge solo
   dai contratti (`agents/contracts/`) e dal registro (`agents/registry.rs`, l'unico punto che lo nomina).
   Lo impongono `agents/architecture.rs` (Rust) e `tests/architecture.test.mjs` (`src/`): vedi
   [wiki/integrare-un-coding-agent.md](wiki/integrare-un-coding-agent.md).
2. **`board` non dipende da `automation`.** `board/` è pura: nessun file importa `tauri`, `AppHandle`,
   `automation` o `ipc`; tipi, validazione, operazioni sulla coda, scheduler e vista, con il tempo
   passato dal chiamante. È `automation` che la possiede, la salva su disco (`automation/board_files.rs`)
   e ne esegue gli effetti. Oggi nessun test lo impone: è una proprietà del codice da mantenere.
3. **`ipc` delega tramite trait.** Il canale non sa cosa fa il core degli eventi: riceve un `IpcHandler`
   (`authenticate`, `probe_credential`, `rpc`, `hook_event`) e `automation::IpcBridge` lo implementa
   (`ipc::start_listener(app, Arc::new(IpcBridge(..)))` in `lib.rs`). Il formato dello stdin degli hook
   è il contratto `HookEvents` dell'integrazione. Allo stesso modo `automation` parla con l'esterno solo
   attraverso il trait `Host` (`TauriHost` in produzione, un host finto nei test) e `mcp` attraverso il
   trait `Backend`.
4. **Parti pure.** Hanno tempo e I/O iniettati e si provano senza finestra: `board/`, `automation::Registry`
   (`registry.rs`, `boards.rs`, `worker.rs`), `schedule.rs` (logica), `ipc/gate.rs`, `ipc/wire.rs`,
   `pty/env.rs` (`env_vars`, `merge_wslenv`), `user_skills`, `validate/`, `workspaces/settings_patch` e
   `workspaces/validate`, `hub/threads` e gli eventi della board di `hub/events` (`snapshot`, `diff`, `route`).
5. **Input validato al confine.** Ogni valore che finisce in una riga di comando (branch, modello,
   repository, regole, nomi) passa da `validate/` o dal modulo che lo possiede; i messaggi sono errori
   tipizzati (`ValidationError`), in italiano per l'interfaccia e in inglese per gli strumenti letti dal
   modello.

Cicli che esistono davvero (e perché): `workspaces/commands.rs` e `pty/mod.rs` chiamano `AutomationState`
perché cambiare consegna, fasce o archiviazione di un workspace, o aprire una shell, riguarda anche
lo scheduler e i segreti di sessione; `automation::TauriHost` chiede a `pty` il workspace di un tab.
Restano confinati ai comandi Tauri e all'host reale; la logica pura non li attraversa. Dello stesso tipo è
`workspaces/commands.rs` → `hub::forget_workspace` (la rimozione di un workspace cancella i suoi thread) e
`automation::TauriHost::board_published` → `hub::board_published` (le viste pubblicate diventano eventi nei
thread); `board` non dipende da `hub` e la logica di `automation` lo raggiunge solo dal trait `Host`.
Nel verso opposto non c'è un ciclo: all'accensione l'HUB deve fissare la base degli eventi con la vista corrente
di ogni board, ma `hub` non nomina `automation`. `lib.rs`, che compone già entrambi, registra in `setup`
`automation::board_views` come `hub::BoardSource` (`hub::set_board_source`), e `hub_set_enabled` la chiama
solo al passaggio da spento ad acceso. Una sorgente iniettata invece di un comando nel livello di composizione
lascia `hub_set_enabled` dov'è (con l'impostazione) e invece di una ripubblicazione forzata di tutte le board non
emette `board-update` né salva file. Allo stesso modo le **azioni** dei segretari sulla board (`hub::BoardActions`:
creare un task `plan`, decidere una proposta, rispondere a una domanda) sono puntatori a funzione che `lib.rs`
registra con `hub::set_board_actions` (`automation::queue_hub_plan_task`, `automation::decide_hub_proposal`,
`automation::answer_question`, lo stesso codice dei pulsanti del pannello Task e delle risposte nel terminale):
`hub` continua a non nominare `automation`. La terza azione ha due utilizzatori: la **risposta diretta** a una
domanda (`hub/direct_answer.rs`, senza passare dal modello) e il tool `relay_answer` dei segretari (`hub/engine/relay.rs`).

### Dove sta cosa

| Modulo | Responsabilità |
|---|---|
| `platform/` | `Target` (Windows o distro WSL) e `TargetKind`; costruzione dei comandi per target (`cmd /c`, `wsl.exe`, finestra nascosta); `output_with_timeout` (output limitato a 8 MiB per stream); conversioni di path Windows/WSL |
| `wsl.rs`, `wsl/home.rs` | ciò che è davvero WSL: distro installate, esistenza di una cartella, `$HOME` della distro |
| `validate/` | predicati sui valori che finiscono in comandi esterni (`valid_branch`, `valid_model`, `normalize_repo`), `ValidationError` con codici stabili e messaggi bilingui |
| `store/` | persistenza JSON comune: busta versionata, migrazioni, backup, avvisi (`StoreWarning`), `write_atomic` |
| `workspaces/` | il `Workspace` (target, avvii, impostazioni dei worker, consegna, repository, archiviazione), il suo store in memoria e `workspaces.json`, i comandi Tauri, le patch di impostazioni proposte dagli agenti |
| `agents/` | contratti (`contracts/`), registro, rilevamento, pacchetti/plugin, preset, regole dei worker; `claude_code/` è l'unica integrazione |
| `board/` | la coda dei task di un workspace: modello, ciclo di vita, scheduler, vista, formato su disco, recupero, archivio (pura) |
| `automation/` | registro di worker e board, scheduler a timer, avvio e pulizia dei worktree, E2E, RPC dei tool MCP, autenticazione per sessione, salvataggio delle board |
| `pty/` | una shell per tab in un PTY, con output su un `Channel` |
| `ipc/` | listener TCP locale, client `--notify`, protocollo delle righe JSON, cancello degli eventi |
| `mcp.rs`, `mcp/` | server MCP su stdio (`--mcp`): inoltra le `tools/call` come RPC all'app |
| `session.rs` | `session.json` e `scrollback/` |
| `schedule.rs`, `awake.rs` | fasce orarie (logica pura + `schedule.json`) e blocco dello standby |
| `launch_settings.rs`, `bots.rs` | impostazioni globali degli avvii e nomi/avatar dei bot (su `store/`) |
| `launch_migration/` | migrazione dei vecchi id `builtin:preset:*` verso i Bot: tabella congelata (`ids.rs`), riconciliazione pura (`reconcile.rs`), passo all'avvio (`mod.rs`) |
| `hub.rs`, `hub/` | Agents HUB (spento di default): impostazioni globali (provider del modello, lingua, sessioni, segretari), canali, thread e messaggi su `store/`, comandi ed evento `hub-update`, eventi della board nei thread, motore dei segretari (`hub/engine/`: menzioni, sessioni, tool calling verso un endpoint compatibile OpenAI, evento `hub-secretary-status`) |
| `connectors.rs`, `connectors/` | connettori dell'Agents HUB, Fonti (`source`, es. Gmail) e Canali di contatto (`contact`, es. Telegram), compilati dietro contratti con registro, supervisori in background, comandi Tauri e configurazione in `connectors.json`; il registro di produzione è ancora vuoto (nessun connettore reale prima delle fasi 4 e 5); progetto e fasi in [CONNECTORS.MD](CONNECTORS.MD) |
| `secrets.rs` | segreti nel portachiavi del sistema operativo (crate `keyring`) dietro il trait `SecretStore`; un finto in memoria per i test |
| `user_skills.rs`, `user_skills/` | skill globali dell'utente: archivio e sincronizzazione nelle cartelle degli agenti |
| `toast.rs`, `updater.rs`, `net.rs`, `ttl_cache.rs`, `logging.rs` | toast cliccabile (Windows), controllo aggiornamenti da CLI, client HTTP via `curl`, cache con scadenza, log su file |

## Flussi principali

### Avvio

1. `main.rs` esce subito per i rami da riga di comando; altrimenti `run()` (`lib.rs`) costruisce
   l'app: prima il plugin di log (`logging::plugin()`), poi opener, dialog, clipboard, notification,
   updater, process, e gli stati gestiti (`WorkspaceStore`, `PtyState`, store delle impostazioni degli avvii, dei bot e
   dell'HUB, `hub::ApiKey` sul portachiavi del sistema, `HubState`, `HubFeed`, `hub::HubBoardActions`, `hub::SecretaryEngine` con le sessioni dei segretari, `AutomationState`).
2. `setup`, in quest'ordine: `store::install_emitter` (da qui un avviso dello store diventa un evento
   `store-warning`), `store::legacy::migrate_legacy_data` (copia dei vecchi identifier), caricamento di
   `workspaces.json`, `launch_settings.json`, `bots.json`, `hub_settings.json` (i thread dell'HUB no: si leggono alla
   prima richiesta), `launch_migration::reconcile_at_startup` (dopo workspace, impostazioni degli avvii, bot e HUB:
   crea una volta sola, in modo idempotente, il “Bot derivato” `<bot>-<agente>` a cui punta un avvio predefinito
   migrato, copia del Bot del preset con l'agente fissato e nome tipo “Intake (OpenCode)”, adattato a 24 caratteri e
   reso unico, `preset` = id del preset di base, costruito da `bots/derived.rs::derived_bot`; con un agente
   sconosciuto ripiega sul Bot del preset, e riscrive ogni id legacy rimasto), delle board (`automation::load_boards`: migra i file
   vecchi e fa fallire come interrotti i task `running`), `automation::auto::init` (carica `schedule.json` e
   lancia il thread del timer, che aspetta `frontend_ready`), il ripristino in background delle skill utente
   e infine `ipc::start_listener` (apre il socket in modo sincrono: la porta è nota prima del primo terminale).
3. Il frontend (`App.tsx`) avvia gli hook di fondo: `useInitialData` legge workspace, profili degli agenti, avvii
   integrati e impostazioni; `useBoardsSync` carica le board e ascolta `board-update`; `useSessionPersistence`
   ripristina i tab quando workspace e profili sono pronti; `useAppEvents` registra i listener degli eventi.
4. Quando sessione ripristinata e listener sono pronti, `useFrontendReady` chiama `frontend_ready`. Solo allora
   il timer di `automation::auto` fa il primo giro (con un tetto di 30 s se il frontend non risponde): rivaluta
   tutte le board (i task in coda ripartono) e riallinea le worktree con le board; da lì ogni 30 s. Così i primi
   `automation-spawn` trovano i listener già registrati.
5. All'uscita (`RunEvent::Exit`) si annullano le esecuzioni E2E, si salvano le board con un salvataggio ancora in
   coda (`automation::flush_on_exit`) e si chiudono tutti i terminali (`pty::close_all_on_exit`).

### Sessione di terminale

1. La UI crea il tab (`app/actions/tabs.ts`: chiave UUID = `tab_id`, bot, gruppo del layout) e monta un
   `TerminalView`, che chiama `pty_spawn(workspace_id, tab_id, cols, rows, cwd?, channel)`.
2. `pty_spawn` (async, lavoro su `crate::blocking`) valida il `tab_id` (`ipc::is_valid_tab_id`), risolve il
   workspace e il suo `Target`, **prenota** lo slot nel `Registry` (al massimo 32 sessioni, un solo PTY per tab,
   sotto lo stesso lock), controlla la cartella di partenza, chiede ad `AutomationState` il **segreto di sessione**,
   costruisce il comando (`session_command`: PowerShell/shell custom su Windows, `wsl.exe -d <distro> --cd <path>`
   su WSL, con `CLI_COMPANION_SESSION`, `CLI_COMPANION_TOKEN`, `CLI_COMPANION_PORT`, `TERM`, `COLORTERM` e, su
   WSL, `WSLENV`) e apre il PTY. Se un passo fallisce, la prenotazione e la credenziale si liberano da sole (drop).
3. Un thread legge il PTY e inoltra i chunk (decodifica UTF-8 incrementale) sul `Channel`; un altro scrive i byte
   accodati da `pty_write`. Un Invio scritto nel terminale di un worker in attesa avvisa l'automazione
   (`automation::user_input`).
4. Alla fine del flusso (shell uscita o `pty_close`) la sessione esce dalla tabella, il figlio viene atteso, la
   credenziale viene revocata e il canale riceve `exit`; il frontend chiude il tab.

### Hook → evento

1. Un coding agent esegue il suo hook, che lancia `cli-companion --notify <evento> <agente>` (nel plugin generato
   da `agents/<integrazione>/package`). Il processo eredita da quella shell `CLI_COMPANION_SESSION`,
   `CLI_COMPANION_TOKEN` e `CLI_COMPANION_PORT`.
2. `ipc::run_notify` (`ipc/client.rs`): se l'evento non è noto, o manca il `tab_id`, esce con 0 senza fare nulla;
   altrimenti legge lo stdin dell'hook (tetto 1 MB), lo fa normalizzare dal contratto `HookEvents` dell'agente e
   spedisce una riga JSON a `127.0.0.1:<porta>` con il segreto del tab.
3. Il listener (`ipc/server.rs`, al massimo 32 connessioni) passa la riga al **cancello** (`ipc/gate.rs`, puro):
   senza `tab_id` valido e senza il segreto di quel tab (`IpcHandler::authenticate`) l'evento è scartato; un
   `permission-request` diventa `needs-input` e i duplicati vengono eliminati.
4. L'evento ammesso va al core (`IpcHandler::hook_event` → `automation::on_hook_event` → `Registry::on_hook`), che
   aggiorna il worker del tab (attività, Stop bloccato, fine sessione) e può rispondere con un motivo di blocco.
   La risposta è una riga `{}` o `{"block":"…"}`: solo `--notify finished` la legge e, se c'è `block`, stampa su
   stdout la riga che l'agente si aspetta (`HookEvents::stop_block_output`) così che il turno prosegua.
5. Dopo che il core ha elaborato l'hook, il server emette l'evento Tauri `agent-event` verso il frontend, se può
   arrivarci (`forwards_to_frontend`: non uno `finished` bloccato o con lavoro in background); il segreto è già stato
   tolto dal cancello. Lì `app/events.ts` (`onAgentEvent`) aggiorna lo stato del tab in
   `tabRuntime`, suona e, a finestra senza focus, mostra la notifica di sistema. Con l'Agents HUB acceso e le sue
   «Notifiche di sistema» attive (`hubCoversAgentEvent`, letto all'arrivo dell'evento) suono e notifica di sistema
   non partono: restano lo stato del tab (✓/❗) e i contatori, e le notifiche sono solo quelle dell'HUB.

### Task: dalla proposta alla consegna

1. **Proposta.** In un tab normale l'Intake (un preset del plugin) chiama il tool MCP `propose_tasks`. Il server MCP
   (`--mcp`) inoltra la chiamata come RPC (`ipc::rpc_call`) col segreto del tab; `ipc/server.rs` la autentica,
   `automation::rpc::dispatch` risolve il ruolo del chiamante (`automation/roles.rs`) e verifica che il metodo gli
   sia consentito; `automation/boards/session.rs` valida le specifiche con `board::parse_proposal` e aggiunge una `Proposal`
   alla board. Ogni modifica della board produce `Effect::Publish`: evento `board-update` e salvataggio coalescente.
2. **Accettazione.** L'utente sceglie dalla UI (`proposal_decide`): le specifiche vengono rivalidate ed entrano in
   coda come `queued`. Ogni specifica proposta nomina un Bot (`bot`, obbligatorio in `propose_tasks`): all'accettazione
   `Board::check_specs` lo risolve e copia agente, modello, effort e skill nel task (snapshot; cambiare il Bot dopo non
   tocca i task in coda). I task senza `bot` si comportano come prima.
3. **Scheduler.** Dopo ogni modifica (e a ogni giro del timer) `board::schedule` (pura) sceglie i task avviabili:
   coda non in pausa, fascia aperta, dipendenze `done`, agente del task pronto (`AgentGate`), slot libero. Ogni scelto
   passa a `running` e il registro emette `Effect::StartTask`.
4. **Avvio.** Fuori dal lock (`automation/worktrees/start.rs`, thread dell'host): base del branch, `git fetch`, worktree
   `task/<id>` in `<repo>/.worktrees/<id>` (solo con consegna a worktree), file da copiare e comandi `setup` del
   repository, poi `emit_spawn` (`automation-spawn`) con il comando di avvio costruito dal contratto
   `WorkerAutomation` dell'agente **del task** (modello e regole di permesso per agente: vedi “Integrazioni: stato,
   risolutore e prontezza”). Un guardiano (`watchdog`) fallisce gli avvii bloccati dopo 5 minuti.
5. **Lavoro.** Il frontend apre il tab del worker in secondo piano (`app/events.ts`, `onAutomationSpawn`) e quel tab
   fa `pty_spawn` come ogni altro. Il worker chiama `get_task` (istruzioni, ambiente e passi di consegna
   generati da `board/texts.rs`), lavora, consegna secondo la modalità del workspace e chiama `complete_task`. Gli
   hook ne tengono aggiornato lo stato (`running`, `needs_input`, `idle`, ...).
6. **Esito.** `complete_task` costruisce il `TaskReport` (con il diffstat calcolato dall'app e, per i task marcati E2E,
   il blocco sull'ultima esecuzione) e porta il task a `done` o `failed`. Dopo 1,5 s il registro emette
   `automation-close`; chiuso il tab, dopo 2 s parte la pulizia (`cleanup` del repository, `git worktree remove`,
   cancellazione del branch). Un `failed` conserva worktree e branch per il "Riprova".
7. **Dopo.** I task conclusi oltre i 200 più recenti escono dalla board nell'archivio `.jsonl`; a ogni avvio e ogni 5
   minuti il riallineamento delle worktree ripulisce ciò che la pulizia normale ha perso. Gli stati e le transizioni sono
   in [BOARD.MD](BOARD.MD), "Ciclo di vita".

### Persistenza e migrazioni

1. Ogni file strutturato è descritto da un `JsonFile<T>` statico (nome, catena di migrazioni, controllo semantico
   opzionale `validated`, lettura voce per voce opzionale `decoded_with`); `JsonFile::named` ne crea uno il cui nome è
   noto solo a runtime (una board per workspace). Lo stato in memoria sta in un `JsonStore<T>` gestito da Tauri
   (`workspaces/store.rs`, `launch_settings.rs`, `bots.rs`) oppure, per le board, nel `Registry` di `automation`.
2. Al caricamento: file assente → default; file senza busta → versione 0; si applicano i passi `steps[versione..]`
   (funzioni pure `Value -> Value`), poi la deserializzazione, `decoded_with` (le voci illeggibili si escludono e il file
   originale va in backup) e `validated`. Dopo la lettura le board applicano anche `Board::sanitize` e
   `recover_after_restart` (che non è una migrazione: si esegue a ogni caricamento).
3. Un file corrotto, non migrabile o scritto da una versione più recente non viene mai cancellato né sovrascritto:
   va in `<nome>.bak-<AAAAMMGG-OOMMSSmmm>` (ultimi 5) e produce uno `StoreWarning` (log, comando `store_warnings`,
   evento `store-warning`, avviso nel frontend). Un file illeggibile resta dov'è e i suoi salvataggi sono rifiutati.
4. Al salvataggio: `write_atomic` (file temporaneo, `sync_all`, rename con ripetizioni su Windows). Le board salvano al
   massimo una volta al secondo per workspace; l'archivio dei task usciti si scrive **prima** della board che non li
   contiene più.
5. Aggiungere una migrazione = aggiungere un passo in coda alla catena del file (la versione è il numero dei passi) e
   un test sul passo. Nessun passo si modifica dopo il rilascio. Dettagli del formato in "`store/`".

## Backend: i moduli

Le sezioni seguenti descrivono ciascun modulo. I contratti con il frontend (comandi, eventi, DTO) sono
dettagliati qui o, per la coda dei task, in [BOARD.MD](BOARD.MD).

### `platform/`, `wsl.rs`, `validate/` — fondamenta

- `platform/` è l'unico posto che conosce `cmd /c`, `wsl.exe` e il flag di finestra nascosta
  (`CREATE_NO_WINDOW`). `Target { Windows, Wsl { distro } }` costruisce i comandi (`command`, `git_command`) e li
  esegue (`run`, `run_timeout`, con `output_with_timeout`: i flussi sono letti da thread dedicati e limitati a
  8 MiB ciascuno). `TargetKind` (`"windows"` / `"wsl"`) è la famiglia, persistita nei workspace e vista dal
  frontend. Le conversioni di path (`windows_to_wsl_path`: `C:\a\b` → `/mnt/c/a/b`, `\\wsl$\Distro\home\u` →
  `/home/u`; `wsl_to_windows_path`) stanno in `paths.rs`.
- `wsl.rs`: `installed_distros()` esegue `wsl.exe -l -q` (l'output è **UTF-16LE con BOM**: la decodifica gestisce
  anche UTF-16BE e UTF-8, con filtro difensivo sulle righe spurie), `wsl_dir_exists` / `ensure_wsl_dir` verificano
  una cartella nella distro, `wsl/home.rs` risolve `$HOME` (una volta per processo). Comandi Tauri:
  `list_wsl_distros`, `check_wsl_path` (verifica live del form di aggiunta).
- `validate/`: predicati puri e `ValidationError` (vedi regola 5 della mappa). Un `ErrorCode` porta nella stessa riga
  della tabella il modello italiano e quello inglese: un codice senza i due messaggi non compila.
- `net.rs` (HTTP via un sottoprocesso `curl`, senza un crate di rete in più: l'unica eccezione è il client del
  modello dei segretari, `async-openai` sul reqwest 0.13 già presente per l'updater, vedi HUB.MD) e `ttl_cache.rs` (cache con scadenza per
  risultati di processi lenti, come il rilevamento degli agenti) sono helper condivisi. I connettori fanno eccezione per la rete: il loro
  client è un `reqwest` condiviso che passa solo da `connectors/runtime/http.rs` (allowlist degli host dichiarati).

### `workspaces/` — i workspace

Un workspace è una cartella di lavoro (Windows o distro WSL) con le sue impostazioni. File: `<app_data>/workspaces.json`
(su `store/`, schema versione 1; l'elenco è letto voce per voce, quindi un workspace illeggibile viene escluso e
riportato senza far perdere gli altri).

- `model.rs`: `Workspace { id, name, kind, path, distro?, shell?, created_at, launches, default_launch?, agent?, delivery,
  use_schedule, repos, archived, + WorkerSettings }`. Il JSON è piatto: `WorkerSettings` ci entra con
  `#[serde(flatten)]` e conserva i nomi `worker_permission`, `worker_rules`, `worker_agents`
  (`worker_settings.rs`).
  - `agent`: l'agente di worker e revisori del workspace; assente = il predefinito globale
    (`registry::resolve_agent`).
  - `kind`: `"windows"` o `"wsl"`; `path`: percorso Windows (`C:\...`) o Linux assoluto; `distro` solo per WSL;
    `shell`: override opzionale. `target()` (in `target.rs`) lo traduce in un `platform::Target`.
  - `launches`: `Launch { id, name, command }` (id `ln-<millis>-<n>`), rimossi insieme al workspace.
  - `default_launch`: assente = eredita il predefinito globale (`launch_settings.json`, stessi valori); `"shell"` =
    shell vuota; altrimenti l'id di un avvio del workspace, di un integrato (`builtin:<agent_id>` per ogni
    integrazione abilitata, da `agents::builtin_launch_list()`) o di un Bot abilitato (`bot:<id>`, da
    `bots::enabled_launch_ids`). La validazione è `check_default_launch(launches, id, shared)`, con `shared =
    workspaces::shared_launch_ids(bots::enabled_launch_ids(app))` (id integrati + id degli avvii dei Bot abilitati);
    `get_workspace_settings.allowed.default_launch` elenca shell, avvii del workspace, integrati e Bot abilitati.
    Un id salvato che punta a un'integrazione disabilitata non si cancella: l'avvio ripiega sulla shell con un avviso.
  - `worker_permission`: tetto dei permessi dei worker, `"plan" | "default" | "accept-edits" | "bypass"`
    (vocabolario di `agents::PermissionMode`); assente o sconosciuto → `"accept-edits"`, così i vecchi file non
    diventano più permissivi.
  - `worker_agents[<agente>].allow` / `.deny`: regole di permesso pre-approvate / vietate, nella sintassi dell'agente
    (`Bash(git:*)`, `Edit`, ...), validate dal contratto `WorkerPermissions` (`agents/worker_rules.rs`). All'avvio
    di un worker la risorsa `<app_data>/worker-settings/<workspace_id>.json` viene scritta (solo se cambiata) e il
    comando la riceve con `--settings` (con l'Agents HUB acceso il file c'è sempre: contiene anche la sola lettura
    degli allegati del canale del workspace, vedi il punto 4 dell'avvio dei worker più sotto). Il rifiuto dell'agente prevale anche in `bypass`; le regole `deny` non sono
    un confine di sicurezza (altre forme di invocazione possono aggirarle): la UI lo avvisa.
  - `worker_agents[<agente>].model`: modello di default dei worker di quell'agente, usato quando il task non indica
    `model` (poi vale quello del catalogo dell'agente).
  - `worker_rules`: testo accodato da `get_task` alle istruzioni di ogni worker.
  - `delivery`: `Delivery { mode: pr|local_merge|push|commit (default commit), parallel (default false, solo
    pr/local_merge), merge_method: squash|merge|rebase (default squash) }`.
  - `use_schedule`: i task partono solo nelle fasce orarie.
  - `repos`: `RepoConfig` per repository (branch base, file da copiare, setup e pulizia dei worktree, `e2e`
    opzionale: `E2eConfig { command, junit, timeout_min }` in `workspaces/e2e.rs`; vedi BOARD.MD, "Test E2E").
  - `archived`: workspace congelato; la board non avvia task e quelli in corso tornano in coda.
- `migrations.rs`: passo 0 → 1 (`migrate_v0`) sul JSON: porta alla busta il formato storico e converte
  `auto_orchestrate.enabled` → `use_schedule`, `worktree_setup` → `repos["."].setup` (se vuoto), il vecchio
  `is_default` degli avvii → `default_launch` e `builtin:orchestrator` → `bot:intake`; i campi vecchi spariscono.
  Il passo 2 → 3 (`migrate_v2`) riscrive i `default_launch` rimasti sugli id dei preset: `builtin:preset:task-intake`
  → `bot:intake`, `builtin:preset:project-setup` → `bot:setup`, `builtin:preset:skill-wizard` → `bot:skill-wizard`,
  `builtin:preset:<preset>@<agente>` → `bot:<bot>-<agente>`. Una versione precedente dell'app vede un
  `workspaces.json` v3 come più recente e lo mette da parte (vedi "File su disco").
- `store.rs`: `WorkspaceStore` (un `JsonStore<Vec<Workspace>>`). Ogni modifica passa da `update` / `modify`: si
  applica a una copia, si salva e solo poi si pubblica.
- `validate.rs`, `delivery.rs`, `repo_config.rs`, `e2e.rs`: validazione e normalizzazione dei valori (errori
  tipizzati). `settings_patch.rs` (+ `settings_patch/`) applica in modo tutto-o-niente le modifiche proposte dagli
  agenti (`propose_settings`), con le stesse funzioni di validazione dei comandi dell'interfaccia
  (`SettingsPatch::normalized(ws, shared)` / `apply(ws, shared, new_id)`; il trait `Host` dell'automazione espone
  `shared_launch_ids()`). Una proposta in attesa salvata prima dell'aggiornamento con un `default_launch`
  `builtin:preset:*` viene rimappata al volo (`launch_migration::current_launch_id`: sul Bot derivato se esiste, altrimenti
  sul Bot del preset).
- Comandi (`commands.rs`, `launches.rs`, `explorer.rs`): `list_workspaces`, `add_workspace`, `rename_workspace`,
  `reorder_workspaces`, `set_workspace_archived`, `remove_workspace`, `set_worker_permission`,
  `set_workspace_agent`, `set_worker_permission_rules`, `set_worker_model`, `set_worker_rules`, `set_delivery`, `set_use_schedule`,
  `set_repo_config`, `add_launch`, `update_launch`, `remove_launch`, `set_default_launch`,
  `open_workspace_folder` (`explorer.exe`; per WSL il percorso UNC `\\wsl.localhost\<distro>\...`). I comandi
  tornano il `Workspace` aggiornato; quelli che toccano consegna, fasce, archiviazione o rimozione avvisano poi
  `AutomationState`, fuori dal lock dei workspace (lo scheduler li rilegge).
- `add_workspace` valida tutto prima di salvare: Windows, il path deve esistere ed essere una cartella; WSL, distro
  non vuota, path assoluto (accetta anche `C:\...` o `\\wsl$\...` convertendoli) e cartella esistente nella distro;
  niente duplicati a parità di `kind + path + distro`; nome vuoto → ultimo segmento del percorso.

### `pty/` — sessioni terminale

- Moduli: `mod.rs` (comandi Tauri), `registry.rs` (tabella delle sessioni e prenotazione degli slot), `spawn.rs`
  (cartella, comando per target, apertura del PTY), `env.rs` (variabili di sessione e `WSLENV`, puro), `io.rs`
  (thread di lettura e scrittura), `exit.rs` (chiusura all'uscita), `tests.rs`.
- Ogni tab del frontend corrisponde a una sessione PTY numerata (`u32`) tenuta in `Registry` (limite: 32 sessioni,
  un solo PTY per `tab_id`). Limite, unicità del tab e inserimento avvengono sotto lo stesso lock: `Reservation`
  prenota lo slot prima dello spawn e lo libera da sola (drop) se lo spawn fallisce; l'indice tab → sessione rende
  deterministica `workspace_of_tab`.
- All'uscita (`RunEvent::Exit`) `close_all_on_exit` impedisce nuove sessioni e termina quelle vive in parallelo,
  attendendo al massimo 3 s.
- `pty_spawn(workspace_id, tab_id, cols, rows, cwd?, on_event)` (`tab_id` = chiave UUID del tab, validata: ASCII
  alfanumerico/`-`/`_`, max 64):
  - Windows: `powershell.exe` (o la shell custom del workspace; per PowerShell/pwsh con `-NoLogo -NoExit -Command`
    e un piccolo hook che aggiorna il titolo del terminale) con cwd = cartella; WSL: `wsl.exe -d <distro> --cd <path>
    [-e <shell>]`;
  - `env_vars` (pura) imposta per entrambi `TERM=xterm-256color` + `COLORTERM=truecolor` (senza questi hint molti CLI
    disabilitano i colori; niente `FORCE_COLOR`, per rispettare `NO_COLOR`), `CLI_COMPANION_SESSION=<tab_id>` (gli
    hook degli agenti ereditano la variabile e così l'evento si lega al tab), `CLI_COMPANION_TOKEN` (il segreto
    della sessione, emesso da `AutomationState::issue_credential` e revocato quando la shell finisce) e
    `CLI_COMPANION_PORT` (porta del listener, se in ascolto; senza listener la variabile ereditata viene rimossa);
  - su WSL le stesse variabili si aggiungono a `WSLENV` con il suffisso `/u` (`merge_wslenv`: il valore del processo
    padre è preservato, senza duplicati) perché attraversino il confine verso la distro e, via interop, tornino
    all'exe Windows dell'hook. `env_vars` riceve il `WSLENV` ereditato come argomento: i test non dipendono dalla
    shell da cui girano;
  - `cwd` opzionale (usato dalle sessioni automatiche) al posto della cartella del workspace, validato come questa
    (Windows `is_dir`, WSL `wsl_dir_exists`); la cartella si ricontrolla allo spawn.
- `pty_write` è async (non gira sul main thread): accoda i byte e, quando l'input contiene Invio, avvisa
  `automation::user_input` su un thread bloccante (il worker che aspettava l'utente torna `running`).
- Un thread di lettura per sessione inoltra i chunk al `Channel<PtyEvent>` passato a `pty_spawn` (`{kind:"output",
  data}`; alla chiusura `{kind:"exit"}`): ogni tab riceve solo il proprio output, anche quello prima del ritorno
  dello spawn. La decodifica UTF-8 è **incrementale**: i byte incompleti di un carattere multibyte restano in coda
  fino al chunk dopo (niente `�` su accenti/box-drawing).
- Comandi: `pty_spawn`, `pty_write`, `pty_resize`, `pty_close`. `pty_close` uccide il processo figlio; il thread di
  lettura invia comunque `exit`, così frontend e backend non divergono.

### `ipc/` — canale di notifica degli agenti

Modulo generico: non sa nulla di Claude Code. Il formato dello stdin degli hook e della
decisione di blocco e' il contratto `HookEvents` dell'integrazione (`agents/contracts/hook_events.rs`,
implementato in `agents/claude_code/hook_events.rs`); cosa farne lo decide il core tramite il trait
`IpcHandler` (implementato da `automation::IpcBridge`), quindi `ipc` non importa `automation`.

- File: `wire.rs` (tipi del protocollo, costanti, parsing di una riga), `client.rs` (`--notify` e
  `rpc_call`), `gate.rs` (cancello puro: quali eventi ammettere), `server.rs` (listener, limite di
  connessioni, stato, comandi Tauri). I test lunghi stanno in `client/tests.rs` e `server/tests.rs`.
- Gli hook dei coding agent chiamano questo stesso exe:
  `cli-companion --notify <evento> [<agente>]` (ramo con early-return in
  `main.rs`, come `--check-update`).
- Eventi (`KNOWN_EVENTS`): `test` (self-test), `session-start`, `session-end`, `finished`,
  `needs-input`, `permission-request`. Gli sconosciuti (es. i vecchi `stop` dei plugin
  Muse) sono ignorati in silenzio con exit 0. Gli eventi diversi da `test` corrispondono a un
  `HookEventKind` (`hook_kind`).
- Il tab di appartenenza arriva dalla variabile `CLI_COMPANION_SESSION`: un
  evento diverso da `test` senza (o con valore non valido) e' ignorato con exit 0,
  perche' la sessione dell'agente gira fuori da cli-companion
  (`should_send(event, tab_id)`, pura). Solo l'invio fallito di un evento da
  inviare da' exit 1.
- `run_notify` e' solo il wrapper che legge l'ambiente reale (`CLI_COMPANION_SESSION`,
  `CLI_COMPANION_TOKEN`, `CLI_COMPANION_PORT`) e cerca il contratto dell'agente nel registro
  (`agents::registry::find(agente)` -> `capabilities().hook_events`); la logica sta in
  `notify_target`, che riceve tab, token, porta, lettura di stdin, contratto e stdout espliciti.
  Agente sconosciuto o senza contratto: l'evento parte senza dettagli e non c'e' mai un output di
  blocco. I test usano solo `notify_target` con porte effimere e un contratto finto: la suite gira
  anche dal terminale di un worker, dove quelle variabili sono vere, e un evento partito da un test
  arriverebbe all'app a nome del worker (un `session-end` gli faceva fallire il task). Protezione:
  notifiche e RPC si connettono solo da `connect_local`, che nei build di test (`cfg(test)`) va in
  panic se la porta e' `DEFAULT_PORT` o quella dichiarata in `CLI_COMPANION_PORT`, prima di
  connettersi; due test `should_panic` lo verificano.
- Se stdin non e' un terminale (`IsTerminal`) viene letta (tetto 1 MB, solo dopo
  la decisione di inviare) e passata a `HookEvents::normalize(kind, raw)`, che la parsa con
  tolleranza ai fallimenti e restituisce un `HookEvent` neutro (id sessione dell'agente,
  `wait` (tipo di attesa neutro `WaitKind`: `Permission`, `Idle`, `Question`, `Other`), `message`
  troncato, `start_source`, `end_reason`, `already_continuing`,
  `background_work`, `transcript_path`, `tool_name`, `tool_summary`). Per Claude Code i nomi dei
  campi dello stdin (`session_id`, `background_tasks`, `session_crons`, `tool_input`...) stanno solo
  in `agents/claude_code/hook_events.rs`, come la traduzione dei suoi `notification_type`
  (`permission_prompt`, `idle_prompt`, `elicitation_dialog`, altro) in `WaitKind`. Il self-test
  lancia l'exe con stdin nulla.
- Payload (una riga JSON su TCP `127.0.0.1:<porta>`, poi uguale nell'evento Tauri
  `agent-event`):
  `{event, agent, tab_id?, agent_session_id?, notification_type?, message?, token?,
  stop_hook_active?, background_work?, source?, reason?, transcript_path?, tool_name?}` (campi
  opzionali: i client vecchi restano validi; i nomi sono quelli del protocollo e non si rinominano).
  `token` viene da `CLI_COMPANION_TOKEN` ed e' il segreto del tab `tab_id`. Per `permission-request` il messaggio e'
  "Permesso richiesto: <tool> - <riassunto>", costruito dal client. `notification_type` porta il
  `WaitKind` con un nome scelto dal protocollo (`permission`, `idle`, `question`, `other`;
  `NotifyPayload::wait_kind` lo rilegge, un nome sconosciuto non e' nessun tipo): il core non vede
  mai i valori grezzi di un agente.
- Porta: il listener (aperto in modo sincrono nel `setup`, prima di ogni pty) prova `DEFAULT_PORT`
  (17421); se e' occupata (altra istanza) ripiega su una porta libera e lo logga. La porta in ascolto
  arriva ai terminali in `CLI_COMPANION_PORT` (anche in `WSLENV`, solo se impostata); `--notify` e
  `--mcp` la leggono con `app_port()` (assente o non valida = `DEFAULT_PORT`). Il comando `ipc_status`
  ritorna `{port, preferred_port, error}`.
- Cancello (`gate.rs`, puro, tempo iniettato): un evento senza `tab_id` valido e senza il segreto di QUEL tab (`IpcHandler::authenticate`) e' scartato (non va ne'
  all'handler ne' al frontend); gli altri perdono il token; `permission-request` diventa `needs-input`
  con l'attesa `Permission`; un `needs-input` di attesa `Permission` per un tab con un
  `permission-request` negli ultimi 60 s e' un duplicato e si scarta (memoria limitata a 256 tab).
  Uno `finished` bloccato o con lavoro in background non va al frontend (`forwards_to_frontend`).
- Connessioni: un thread per connessione, al massimo `MAX_CONNECTIONS` (32) insieme; oltre, la
  connessione si scarta con un warning.
- Sullo stesso socket passano due tipi di riga, distinti dalla chiave `rpc`:
  - **notify**: il payload sopra, limite 64 KB. Il listener risponde con una riga JSON,
    `{}` oppure `{"block":"<motivo>"}`, scritta dopo che il core (`IpcHandler::hook_event`) ha
    elaborato l'hook (scrittura best-effort: un client che ha gia' chiuso non e' un errore). Solo
    `--notify finished` con token legge la riga (timeout 4 s; timeout, chiusura o
    risposta illeggibile = exit 0 senza output): se contiene `block` stampa su stdout
    esattamente `HookEvents::stop_block_output(motivo)` (per Claude Code
    `{"decision":"block","reason":"<motivo>"}`, che fa proseguire il turno) ed esce con 0.
    Nient'altro viene mai scritto su stdout da `--notify`;
  - **RPC**: `{"rpc":1,"token","session","method","params"}`, limite 1 MB,
    risposta di una riga `{"ok":true,"result":..}` o `{"ok":false,"error":".."}`.
    Segreto errato, sessione sconosciuta o non valida (`is_valid_tab_id`) -> `ok:false` con un unico messaggio (`invalid session credentials`).
    `IpcHandler::rpc` (-> `automation::handle_rpc` -> `automation::rpc::dispatch`) la esegue. Il
    client `rpc_call(port, line, timeout)` e' usato da `mcp.rs`; il timeout di lettura della
    richiesta e' 2 s. Nessuna RPC resta in attesa: rispondono tutte subito.
- Da WSL l'exe gira sull'host via interop: il TCP resta locale, nessun
  networking cross-VM.
- Comandi: `test_agent_notify` (self-test via --notify, con una credenziale di prova usa e getta e la porta dell'app) e `ipc_status`.

### `toast.rs` — notifica cliccabile (Windows)

- `show_agent_notification(tab_id, title, body)`: toast con
  `tauri-winrt-notification` (il plugin notification non espone il click su
  desktop). `tab_id` validato con `is_valid_tab_id`; audio della toast
  disattivato (suona gia' l'app).
- AUMID: l'`identifier` quando installata; da `target\debug|release` ripiega
  su `Toast::POWERSHELL_APP_ID` (come tauri-plugin-notification), quindi in dev
  la toast risulta di PowerShell ma il click funziona.
- `on_activated`: `unminimize` + `show` + `set_focus` sulla finestra `main`,
  poi evento `notification-activated` `{tab_id}`. Il frontend applica
  `focusTab` (layout.ts): workspace, pannello attivo, de-massimizzazione.
  Fuori da Windows il comando da' errore e `systemNotify.ts` usa il plugin.

### `agents/` — plugin dei coding agent

Il core non sa come funziona un agente: offre dei **contratti** (uno per capacità) e ogni integrazione implementa quelli che
le servono.

- `mod.rs`: dichiarazioni e riesportazioni (e la documentazione su come aggiungere un agente). `contracts/` ha un file per
  contratto: `integration` (`AgentIntegration`: id, nome, rilevamento, comando interattivo, `capabilities()`), `capabilities`
  (`Capabilities`: un campo `Option<&dyn …>` per ogni contratto, senza `Default`, quindi un nuovo contratto obbliga ogni
  integrazione a decidere), `package` (`IntegrationPackage`), `hook_events` (`HookEvents`), `session_resume`
  (`SessionResume`), `automation` (`WorkerAutomation`), `worker_permissions` (`WorkerPermissions`), `plugin_skills`
  (`PluginSkills`), `user_skills` (`UserSkills`), `usage_limits` (`UsageLimits`), `session_usage`
  (`SessionUsageReader`), `input_profile` (`InputProfileSource`), `models` (`ModelCatalog`: modelli consigliati per
  fascia e regola di validita' del nome; il core sceglie per fascia, non nomina modelli).
- `registry.rs`: il registro `INTEGRATIONS`, l'unico punto in cui il core nomina i singoli agenti; il registro dice solo quali agenti esistono:
  quali sono abilitate e quale è il predefinito (worker e preset) lo sceglie l'utente (`integrations.json`:
  `enablement.rs` regole pure, `integrations_store.rs` persistenza, `resolve.rs` + `registry::resolve_agent` l'unico punto
  in cui si sceglie l'agente di un uso, `readiness.rs` quanto è pronta una coppia agente × target); finché non sceglie
  vale solo la prima voce. `detection.rs` rileva i target (Windows + ogni distro,
  escluse `docker-desktop*`); `package.rs` è il ciclo di vita dei pacchetti (lock per agente × target); `launches.rs`
  produce gli avvii integrati delle sole integrazioni abilitate (`builtin:<agent_id>`); `presets.rs` +
  `presets/*.md` le skill del plugin usate dai Bot;
  `worker_rules.rs` la validazione delle regole di permesso dei worker tramite il contratto.
- `commands.rs`: comandi Tauri generici, che non nominano nessun agente: `agent_env_status` (sonda in parallelo ogni
  coppia agente × target), `agent_profiles`, `agent_usage_limits`, `agent_session_usage`, `install_agent_plugin(agent_id,
  env_key)`, `remove_agent_plugin(agent_id, env_key)`, `agent_plugins_outdated`, `agent_plugins_update`, `builtin_launches`.
- Test: `architecture.rs` (il core non nomina un agente: vedi la mappa dei moduli), `conformance.rs` (verifica da solo
  ogni integrazione registrata e ogni contratto dichiarato), `testing.rs` (supporti).
- Guida completa, con OpenCode come seconda integrazione di riferimento: [wiki/integrare-un-coding-agent.md](wiki/integrare-un-coding-agent.md).
- Aggiungere un agente: una cartella `agents/<nome>/` che implementa `AgentIntegration` + i contratti dichiarati in
  `capabilities()` + una riga in `registry.rs` (e una nei due test architetturali). La UI (Impostazioni → Generale → Integrazioni)
  raggruppa per agente da sola.
- Skill del plugin (preset): `Preset { id, name, skill }` in `presets.rs`, con lo SKILL.md in inglese incluso con
  `include_str!`. Oggi tre (`task-intake`, `skill-wizard`, `project-setup`, descritte sotto e in [BOARD.MD](BOARD.MD));
  aggiungere una skill = una voce in `PRESETS` + un file md. Il pacchetto di ogni integrazione che dichiara
  `plugin-skills` (contratto `PluginSkills`, solo `skill_path`) ne spedisce le skill. Non esistono più voci rapide
  dei preset: una sessione guidata da una skill del plugin parte solo come Bot (`bot:<id>`, vedi `bots/`).
- Automazione (facoltativa): `WorkerAutomation::command(env_kind, &AutomationRequest)
  -> Option<AutomationLaunch>` (`{ command, env }`) costruisce il comando da digitare nel tab, e le variabili d'ambiente da impostare sul tab, per
  avviare una sessione automatica; ritorna `None` se la richiesta non è rappresentabile in modo sicuro (tutto ciò che
  finisce in una riga di comando è non fidato). `AutomationRequest { bootstrap_prompt,
  permission_mode: PermissionMode (Default | AcceptEdits | Plan | Bypass), model, permissions_file }`.
  Helper pubblico `automation_command(agent_id, env_kind, req)`.
  Claude Code: `claude "<bootstrap>" --permission-mode <m> [--settings "<file>"] --allowedTools
  mcp__plugin_cli-companion-notify_cli-companion [--model <m>]` (prompt prima dei flag:
  `--allowedTools` e' variadico, quindi `--settings` lo precede; il file è quello delle regole allow/deny del workspace,
  accettato solo se il percorso è sicuro per la shell; la regola senza nome tool abilita tutti i tool del server,
  necessario perche' `acceptEdits` non approva i tool MCP). `bypassPermissions` al primo
  avvio interattivo mostra un dialogo di conferma (salta con
  `skipDangerousModePermissionPrompt: true` in `~/.claude/settings.json`).
- Profili di input: `InputProfileSource::input_profile() -> InputProfile` (vuoto se non dichiarato)
  con regole dichiarative `InputRule { event: InputEvent, env_kind: Option<"windows"|"wsl">,
  send }`. `InputEvent` (kebab-case) e' il punto di estensione; oggi solo
  `ImagePaste`. Claude Code: `ESC v` (Alt+V) per windows e wsl. Il comando
  `agent_profiles` li espone (nessun processo); l'App li carica all'avvio e
  `resolveInputOverride(profiles, agentId, envKind, event)` (`lib/agents.ts`) sceglie
  la sequenza. L'agente del tab e' `tabAgentId`: stato del plugin (`session-start`..
  `session-end`), altrimenti il primo token del comando di avvio/ripristino
  (`claude`/`claude.exe`). `TerminalView` riceve la sequenza gia' risolta
  (`imagePasteSeq`, via ref) e la invia al posto di `Ctrl+V` quando gli appunti non
  hanno testo; senza regola il comportamento resta invariato.
- `claude_code/` (un file per contratto):
  - rilevamento: Windows `cmd /c claude --version`; WSL: path assoluto via
    `bash -lc 'command -v claude'` (ripiego `bash -ic`, ultima riga che inizia con
    `/`), poi `<abs> --version`; il path assoluto serve per tutti i comandi
    successivi nella distro. Parsing in funzioni pure testate.
  - installazione: marketplace locale generato in
    `<app_data>/agent-plugins/claude-code/<env>/` (`cli-companion-local`, plugin
    `cli-companion-notify`), hook in forma exec (`command` = exe corrente con slash
    dritti, o `/mnt/c/...` su WSL; `args` = `["--notify", evento, "claude-code"]`,
    `timeout` 10): `SessionStart` -> `session-start`, `Stop` -> `finished`,
    `SessionEnd` -> `session-end`, `Notification` (matcher `permission_prompt|idle_prompt|elicitation_dialog`) ->
    `needs-input`. `PermissionRequest` (no matcher, no output, exit 0: it never decides, the
    normal dialog still shows) -> `permission-request`: stdin `tool_name` + `tool_input` are
    summarised (Bash command / file path / URL, max 200 chars) into
    `Permesso richiesto: Bash — <command>`; the backend turns it into a `needs-input`
    (wait kind `Permission`) carrying that text, and drops the generic permission
    Notification that follows for the same tab within 60 s (no duplicate event/sound).
    The hook fires only when a prompt is actually shown (not in bypass, not when a rule
    already allows). Il plugin contiene anche `.mcp.json` con il server `cli-companion`
    (`command` = exe come sopra, `args` = `["--mcp"]`) e una skill per ogni preset di
    `agents/presets.rs` (`Preset { id, name, skill }`): `skills/<id>/SKILL.md`, sorgente
    `agents/presets/<id>.md` inclusa con `include_str!` e normalizzata a LF, in inglese,
    invocabile come `/cli-companion-notify:<id>`. Oggi tre preset: `task-intake` (il
    metodo dell'agente Intake: vedi [BOARD.MD](BOARD.MD)), `skill-wizard` (intervista l'utente
    e crea una skill globale con i tool MCP `save_skill`, `list_skills`, `delete_skill`; le skill
    utente sono archiviate in `<app data>/user-skills` e sincronizzate come destinazione in
    `~/.claude/skills`, dove le cartelle gestite portano il marcatore `.cli-companion-managed`) e
    `project-setup` (esplora il repository, legge la configurazione con `get_workspace_settings` e
    la propone con `propose_settings`: vedi [BOARD.MD](BOARD.MD));
    aggiungere un preset = una voce in `PRESETS` + un file md. Le skill `orchestrate` e `workflow` non esistono piu': a ogni
    installazione `remove_obsolete_skills` toglie da `skills/` le cartelle che non sono
    preset. Passi: `plugin validate`,
    pulizia best-effort, `plugin marketplace add`, `plugin install --scope user`. La
    versione del plugin (`<release>-h<hash>`) deriva da hook + `.mcp.json` + le skill dei preset
    (Claude cache per versione: exe spostato o skill cambiata = versione nuova; i plugin con
    una versione diversa da quella attesa sono segnalati da `agent_plugins_outdated` e
    reinstallati da `agent_plugins_update`). La cartella e' caricata sul posto e deve restare su disco.
  - stato installato: `claude plugin list --json` contiene `id` ==
    `cli-companion-notify@cli-companion-local` (array o oggetto con `plugins`).
  - rimozione: `plugin uninstall` + `marketplace remove` + cartella generata.
- `opencode/` (OpenCode 2.x, un file per contratto più `plugin.ts`, `plugin.rs`, `config_dir.rs`, `mode_rules.rs`,
  `version.rs`): seconda integrazione reale, scritta senza `if` nel core. Dettagli e prove in
  [wiki/integrare-un-coding-agent.md](wiki/integrare-un-coding-agent.md).
  - rilevamento: `opencode --version` (`opencode v2.0.30`); su WSL path assoluto con `resolve_wsl_command_cached`
    (lo shim Windows nel `PATH` di WSL è scartato). Solo la 2.x: `detect` mostra qualsiasi versione, `install` rifiuta
    le altre (la v2 ha un'altra API dei plugin e un altro formato di configurazione).
  - `--standalone` ovunque: di norma il TUI parla con un servizio condiviso per utente, il cui ambiente è quello di chi
    lo ha avviato, quindi plugin e server MCP non vedono le `CLI_COMPANION_*` del tab. Con `--standalone` il server è
    privato, figlio del TUI, e le eredita. Interattivo `opencode --standalone`; ripresa
    `{command} --standalone --session {id}` (toglie anche `--standalone`, `--session/-s`, `--continue/-c`, `--fork`).
    Il plugin è inerte senza `CLI_COMPANION_SESSION` o con `--service` in argv. Un `opencode` digitato a mano funziona
    senza notifiche né MCP.
  - pacchetto: una cartella nostra, `<config>/plugins/cli-companion-notify/` (`<config>` da `opencode debug paths`,
    `XDG_CONFIG_HOME` rispettata), con `index.ts` e `skills/<preset>/SKILL.md`; nessun file dell'utente toccato e nessun
    merge in `opencode.json`. Il plugin registra a runtime il server MCP `cli-companion` (`ctx.mcp.transform`) e le skill
    (`ctx.skill.transform`). Riga di intestazione fissa in `index.ts` come marcatore; una cartella omonima senza
    marcatore è rifiutata; `uninstall` toglie solo la nostra. Copia in `plugin_root` con `destination.txt`. `version` =
    `<release>-h<fnv32>` (helper condiviso con Claude Code).
  - eventi: il plugin ascolta `ctx.event.subscribe` (filtra da sé: il filtro `types` non vale nella 2.0.30) ed esegue
    `<exe> --notify <evento> opencode` con uno stdin JSON nostro (`session_id`, `source`, `continuing`, `wait`,
    `message`, `tool`, `summary`, `reason`). `session.created` (o il primo evento di una sessione mai vista, `resume`) →
    `session-start`; `session.execution.succeeded` → `finished`; `permission.asked` → `permission-request` +
    `needs-input` (permission); `form.created` → `needs-input` (question); `session.execution.failed` → `needs-input`
    (other); `session.deleted` → `session-end`. Subagenti (con `parentID`) ignorati. Blocco del fine turno:
    `{"continue":"<motivo>"}` su stdout, letto dal plugin che chiama `ctx.session.prompt`.
  - automazione: `opencode run` esce al primo fine turno, quindi i worker usano il TUI:
    `opencode --standalone --prompt "<bootstrap>" [--auto]` (`--auto` solo per `Bypass`). Il TUI non ha `--model` né
    `--agent`: modello, `default_agent` (`plan`/`build`) e regole della modalità (`mode_rules.rs`) vanno in
    `OPENCODE_CONFIG_CONTENT`, le regole del workspace nel file di `OPENCODE_CONFIG`
    (`agents.build/plan.permissions`, dopo le regole di primo livello: l'ultima che corrisponde vince, quindi battono
    quelle di modalità; con `--auto` i `deny` valgono ancora). Il server MCP è usabile senza conferme con
    `cli-companion_*` e `execute` in `allow` (il modello lo chiama dal "Code Mode").
  - permessi: sintassi `azione(risorsa)` o `azione`; `prepare` scrive `<dati app>/worker-permissions/opencode/<ws>.json`.
  - preset: skill registrate dal plugin, avviate con `--prompt "Use the skill tool to load the skill <id>, ..."`.
  - skill utente in `<config>/skills`; modelli `provider/modello` (`is_valid` = `valid_model_shape` + provider valido),
    consigliati `opencode/gpt-5.4-mini`, `opencode/gpt-5.5`, `opencode/big-pickle`, nessun predefinito.
  - non dichiarati: `usage_limits`, `session_usage` (sessioni in SQLite, nessun transcript), `input_profile`.
  - limiti noti: chiudere il TUI non emette `session-end` (resta `running` fino alla chiusura del tab); solo la 2.x;
    WSL coperto solo da test unitari.
- Estensioni del core nate da OpenCode: (1) **`AutomationLaunch { command, env }`**: `WorkerAutomation::command` ritorna
  anche variabili d'ambiente, validate da `is_valid_env_name` / `is_valid_env_value` / `first_invalid_env_name` (nome
  `[A-Z_][A-Z0-9_]{0,63}`, riservati `CLI_COMPANION_*`, `WSLENV`, `PATH`, `TERM`, `COLORTERM`; valore ≤ 32 KiB senza
  caratteri di controllo; niente doppioni; `automation_command` rifiuta le non valide). Il backend le prepara per il
  tab del worker (`pty/staged_env.rs`, dentro `PtyState`, consumate una volta da `pty_spawn`, scartate alla chiusura
  del worker) e `pty/env.rs` le aggiunge a `WSLENV` con `/u`: sul tab la shell parte già con l'ambiente giusto, in
  PowerShell, cmd e bash, senza sintassi di shell. (2) **`validate::valid_model_shape`** (e `MODEL_SHAPE_PATTERN`):
  la forma generica di un modello ammette `/` e `:`; la regola vera la dà `ModelCatalog::is_valid` dell'agente.

### Integrazioni: stato, risolutore e prontezza

Il registro dice quali agenti **esistono**; quali sono **in uso** e quale agente serve a un uso lo decidono tre pezzi
separati, tutti in `agents/`. Il vocabolario per l'utente è nel [README](../README.md), “Integrazioni dei coding agent”.

- **Stato** (`enablement.rs` puro, `integrations_store.rs` persistenza). `IntegrationsState { enabled, default_agent }`
  sta in `integrations.json` (`JsonFile` con `adopt_envelope`, vedi `store/`) ed è uno stato di processo, non un `State`
  di Tauri: lo leggono anche punti senza `AppHandle` (sanificazione delle board, regole dei worker, avvii).
  `view` lo riporta sempre alle invarianti: solo id registrati, nell'ordine del registro e senza doppioni, almeno
  un'integrazione abilitata (altrimenti la prima del registro), predefinito fra le abilitate (altrimenti la prima abilitata).
  `normalize_loaded` gira una volta all'avvio (`load_integrations`, prima di workspace e board): scarta gli id
  sconosciuti con un avviso nel log e, se non resta nulla (file assente o corrotto), abilita le integrazioni con un
  pacchetto già installato in qualche ambiente (`package::installed_somewhere`), altrimenti la prima; poi salva.
  `set_enabled` rifiuta di spegnere l'ultima abilitata e quella predefinita; `set_default` richiede un'integrazione
  abilitata. I comandi Tauri sono `integrations_list` (ogni integrazione con contratti dichiarati, modelli, modello
  predefinito e, per ogni ambiente, stato rilevato e `Readiness`), `set_integration_enabled`,
  `set_default_integration`; ogni cambio emette `integrations-changed` col nuovo stato.
- **Risolutore** (`resolve.rs` puro + `registry::resolve_agent`). L'unico punto in cui si sceglie l'agente di un uso:
  `resolve_agent(explicit, workspace_agent, AgentUse)` prende il primo `Some` fra `explicit` (l'agente del task o
  del Bot), `workspace_agent` (`Workspace.agent`) e il predefinito globale. `AgentUse::Worker` (l'unico uso) pretende il
  contratto `automation`; il predefinito globale deve dichiararlo come gli altri. Una scelta presente ma non utilizzabile è un `AgentUnavailable` (`Unknown`, `Disabled`, `MissingContract`), mai
  un ripiego su un altro agente; il testo è in inglese perché arriva al modello via RPC (“<nome> is disabled: enable it
  in the integrations settings”, “<nome> does not support automated sessions”, “unknown agent: <id>”).
  `registry::find` invece cerca fra **tutte** le integrazioni, anche disabilitate: serve a hook, regole e comandi di un
  agente il cui pacchetto è ancora installato, e non sostituisce `resolve_agent` per scegliere. Chi risolve: la board
  (`board/agent.rs`, `resolve_spec_agent`, a proposta, accettazione, creazione e aggiornamento di un task),
  `workspaces/validate.rs` (`normalize_workspace_agent`, per `Workspace.agent`), `registry::automation_command`
  (comando di avvio del worker, `AgentUse::Worker`).
- **Modelli** (`models.rs`). Il core ragiona per fascia: `model_is_valid(integration, nome)` usa
  `ModelCatalog::is_valid` se l'integrazione dichiara `models`, altrimenti la forma sicura di `validate::valid_model`;
  `tier_models` dà il primo consigliato di `Standard` e `Strong` ed è ciò che finisce nelle descrizioni del campo `model`
  dei tool MCP. Nessun nome di modello fuori dalla cartella dell'integrazione.
- **Prontezza** (`readiness.rs`). `Readiness` = `Ready | NotFound | NotInstalled | Outdated | Disabled`, decisa da
  `classify` con questa precedenza: non abilitata, agente non trovato nel target, pacchetto mancante, pacchetto di una
  versione diversa da quella attesa (solo se l'integrazione dichiara `package`), pronta. Un'integrazione disabilitata
  risponde `Disabled` senza rilevare nulla. Il sondaggio (`probe`: rilevamento + versione del pacchetto) lancia processi,
  quindi sta fuori dal main thread e passa da una cache di 30 s per coppia agente × target, azzerata prima e dopo
  ogni operazione sui pacchetti (installazione, aggiornamento, rimozione). `agent_readiness(app, id, target)` è la versione per chi ha solo l'id (l'automazione).

**Avvii integrati** (`agents/launches.rs`). `builtin_launch_list()` (comando Tauri `builtin_launches`, senza argomenti)
produce una sola voce interattiva `builtin:<agent_id>` per ogni integrazione abilitata (`BuiltinLaunch` non ha più i campi
`preset` e `follows_workspace_agent`). Le sessioni guidate da una skill del plugin (Task Intake, Skill Wizard, Project Setup)
partono solo come Bot (`bot:<id>`, `bots/launches.rs`), che porta agente, modello, effort e skill nella configurazione
(`bots.json`); un Bot con una skill del plugin richiede un agente che dichiara `plugin-skills`
(`bots/resolve.rs` avvisa “does not ship the plugin skills”). Tutte le voci integrate portano `restore_command`
(il tab riprende la conversazione, non rilancia la skill). Un id salvato
(avvio predefinito, tab) che punta a un'integrazione disabilitata non si cancella: l'avvio parte come shell con un avviso.

**Avvio di un task con l'agente.**

1. L'agente del task è sempre valorizzato: `resolve_spec_agent` lo risolve alla creazione (task → workspace → globale) e
   controlla anche il modello con `model_is_valid`; non cambia se poi cambiano i predefiniti.
2. Lo scheduler (puro) non considera candidato un task in coda il cui agente è nel *gate* del workspace
   (`board/agent_gate.rs`, `AgentGate`): agenti non `Ready` oppure ancora in controllo. Il task non fallisce e non occupa
   uno slot; la vista espone `agent_unavailable { agent, reason }`.
3. La prontezza del gate non blocca mai sotto il lock del registro: `automation/agent_probe.rs` ricorda l'ultimo valore
   noto per target × agente, lo rinnova su un thread bloccante dopo 15 s e, se cambia, fa rivalutare le board
   (`recheck_agents`), come a ogni giro del timer.
4. All'avvio (`automation/worktrees/start.rs`) `AutomationRequest.effort` è `task.effort` (un livello non supportato
   è scartato con una nota sul task) e, se il task ha una skill, `get_task` la fa caricare per prima al worker (una skill
   utente mancante blocca l'avvio con una nota). Il modello è `task.model`, poi `Workspace.worker_agents[<agente>].model`,
   poi il predefinito del catalogo dell'agente (`None` = non si passa il modello); le regole allow/deny sono quelle di
   `worker_agents[<agente>]`, preparate dal contratto `WorkerPermissions` *dello stesso agente*
   (`worker_permissions_file`), e il comando viene da `automation_command(agente del task, …)`. Con l'Agents HUB
   acceso lo stesso contratto riceve anche le cartelle che il worker legge senza conferme (`readable`: solo
   `hub_attachments/channels/<workspace_id>/`, da `hub::attachments::workers_readable_dir`), e ne concede la sola
   lettura: Claude Code `Read(//<cartella>/**)` tra i consentiti e `Edit(//<cartella>/**)` tra i vietati del file
   `--settings` (forma POSIX `/c/...` su Windows, `/mnt/c/...` in WSL; non `--add-dir`, che consentirebbe anche di
   modificare e carica la configurazione della cartella); OpenCode `external_directory(<cartella>/*)` consentita
   prima e `edit(<cartella>/*)` vietata dopo tutte le regole. Una cartella non esprimibile in modo sicuro (caratteri
   jolly, percorso non su un drive) si salta con un avviso nel log. Con l'HUB spento il comando resta quello di prima.
5. Il worker nasce con `agent_id` del task; il resto (hook, `get_task`, `complete_task`) non dipende dall'agente.

### `board/` — modello, ciclo di vita e scheduler della coda

Parte **pura**: nessun I/O, nessun `AppHandle`, tempo iniettato. Ogni workspace ha una `Board` (coda di `Task` con
dipendenze, proposte di task e di impostazioni pendenti); l'integrazione con worker, git e disco sta in `automation/`.
Il contratto con il frontend e con i tool MCP è [BOARD.MD](BOARD.MD). File:

| File | Contenuto |
|---|---|
| `model.rs` | `Board`, `Task`, `Proposal`, `TaskStatus` (`queued|running|done|failed`), `WorktreeInfo`, costanti (limiti, note standard) |
| `state.rs` | `TaskState`: `Queued`, `Running(Run)` con fase `Preparing` (l'app prepara worktree e setup, nessun worker) o `Live` (il tab del worker e la sua attività), `Done(Finished)`, `Failed { finished, cause }` |
| `lifecycle.rs`, `lifecycle/` | **l'unico posto che scrive lo stato del task**: `TaskEvent` applicato con `Task::apply`; `actions.rs` ha la tabella `permits(stato, operazione)` e `TaskAction`; `ops.rs` i metodi di `Board` (ferma, riprova, segna come fatto, archiviazione del workspace) |
| `queue.rs`, `cascade.rs` | specifiche, proposte (accetta/scarta), modifica, riordino, eliminazione anche a cascata (indice delle dipendenze inverse) |
| `spec.rs` | `TaskSpec`/`TaskPatch`, id (anche generati dal titolo), repository, grafo delle dipendenze, parsing di proposte e limiti |
| `scheduler.rs` | `schedule(board, ctx)`: quali task avviare adesso (funzione pura) |
| `agent.rs`, `agent_gate.rs` | `resolve_spec_agent` (l'agente e il modello di una specifica, via `registry::resolve_agent`) e `AgentGate`/`AgentUnavailableView` (i task in coda con l'agente non pronto, che lo scheduler salta e la vista spiega) |
| `view.rs` | `BoardView` / `TaskView` per il frontend (`waiting_on`, `attention`, `allowed_actions`, `can_edit`, `can_reorder`, `cascade`, `limits`) e righe compatte di `list_tasks` |
| `worker.rs` | `WorkerStatus`/`WorkerActivity`/`WorkerEnd` e la tabella di transizione pura applicata agli hook |
| `report.rs`, `review.rs`, `e2e.rs` | `TaskReport`, task di review (verdetto, task rivisti), tipi E2E e `check_gate` (il blocco della consegna) |
| `plan.rs` | regole dei task di pianificazione (`kind: "plan"`): tipo riservato all'app, niente `e2e`, report senza consegna, `TaskKind::isolated` |
| `settings.rs` | proposte di impostazioni (`SettingsProposal`) |
| `texts.rs`, `prompts/*.md` | istruzioni, ambiente e passi di consegna dati al worker; standard di ingegneria e istruzioni del reviewer e del pianificatore (inglese, `include_str!`) |
| `persist.rs`, `legacy.rs`, `recovery.rs`, `archive.rs` | formato su disco (busta, migrazioni, riga dell'archivio), migrazione dei file v0.11, recupero dopo un riavvio, selezione dei task da archiviare |

- **Scheduler.** `schedule` non fa nulla con la coda in pausa o con il gate chiuso (workspace archiviato, oppure con
  fasce attive e fuori fascia); sceglie i `queued` con tutte le dipendenze `done`, nell'ordine dell'array. In modalità
  parallela (`pr`/`local_merge` con `parallel`) fino a `max_parallel` running, altrimenti uno per repository, comunque
  entro `max_parallel` (un task `plan`, nel suo worktree, non occupa il repository). Salta anche i task il cui agente è nel `AgentGate` del contesto (non pronto nel target del
  workspace): restano `queued` e non occupano slot (vedi “Integrazioni: stato, risolutore e prontezza”).
- **Azioni.** `permits` è la sola tabella di cosa si può fare a un task per stato; la usano le guardie delle operazioni,
  i messaggi di errore e `allowed_actions` della vista (BOARD.MD, "Azioni consentite").
- **Persistenza.** `<app_data>/boards/<workspace_id>.json` su `store/` (`board_store`, una `JsonFile` per workspace;
  `BOARD_MIGRATIONS`, oggi un passo: `v0_to_v1`). Al caricamento `Board::from_json` migra, applica `sanitize` (limiti di
  `max_parallel`, un agente per ogni task) e `recover_after_restart` (i `running` diventano `failed`). Archivio dei
  conclusi: `MAX_FINISHED_KEPT` = 200, i più vecchi vanno in `boards/archive/<workspace_id>.jsonl`. Il tutto è
  descritto in BOARD.MD ("File su disco" e "Archivio dei task conclusi").

### `automation/` — Task Manager: coda, scheduler e worker

Ogni workspace ha una coda di task; l'app la esegue con uno scheduler deterministico e un
worker (tab PTY marcato automatico) per task. Il worker consegna da solo e riferisce con
`complete_task`; l'Intake (una sessione normale) propone task. Contratto completo (modello
dati, consegna, scheduler, tool, UI): [BOARD.MD](BOARD.MD). Struttura a strati, in `automation/`:

- `mod.rs`: API del modulo e re-export dei comandi Tauri (`commands.rs`); `bridge.rs`: punti
  d'ingresso dall'app reale (RPC autenticate, eventi hook, avvio e chiusura).
- `registry.rs`: `Registry`, nucleo **puro** (niente `AppHandle`, tempo iniettato): worker
  (`get_task`, `complete_task`), board e `Effect` da eseguire. Tetto di 256 worker tracciati.
- `worker.rs`: tipi dei worker e macchina a stati degli hook (attività, Stop bloccato, fine
  sessione, chiusura del tab).
- `auth.rs`: segreto per sessione (CSPRNG) emesso all'apertura di ogni terminale, legato all'id
  del tab e revocato alla fine della shell; ogni RPC ed evento hook verifica la coppia.
- `roles.rs`: tabella unica ruolo → metodi RPC, usata dal dispatch (`rpc.rs`) e dalla lista dei
  tool MCP (un test verifica che coincidano).
- `state.rs`: `AutomationState { Mutex<Registry> }` (managed state). `run` modifica sotto lock,
  schedula le board toccate (`flush_boards`) e applica gli effetti **fuori** dal lock;
  pubblicazione ordinata per `rev` (evento `board-update` + salvataggio coalescente, max 1/s).
- `host.rs`: trait `Host` (eventi Tauri, git, file, timer, workspace) e `TauriHost`; i test usano
  un host finto con i timer in linea.
- `boards.rs`: parte del `Registry` che tiene le board (proposte, modifiche dei task, scheduler).
  `boards/lifecycle.rs`: avvio dei task, guardiano degli avvii, archiviazione del workspace e pulizia
  nel `Registry`. `boards/commands.rs`: logica dei comandi Tauri della UI. `boards/session.rs`: tool
  MCP delle sessioni (Intake, revisore e pianificatore).
- `worktrees.rs`: tipi e helper condivisi di git (percorsi, `git`, verifica dei repository) e file
  della board. `worktrees/start.rs`: avvio di un task (base, `git fetch`, worktree, setup, spawn).
  `worktrees/cleanup.rs`: pulizia a consegna avvenuta. `worktrees/real.rs`: lato reale di git (timeout 120 s, WSL via `wsl.exe -e`,
  sempre tramite `process.rs`), rilevamento dei repository, file di esclusione.
  `worktrees/reconcile.rs`: riallineamento delle worktree con le board (task `done` rimasti,
  worktree `task/*` orfane) all'avvio e ogni 5 minuti, dal tick dello scheduler. `hooks.rs`: setup e pulizia per repository (`RepoConfig`), più lettura e
  cancellazione di file di testo nell'ambiente del workspace (`Host::read_text_file`/`remove_file`).
  `check_e2e_configured` (in `worktrees.rs`) rifiuta un task `e2e` senza configurazione E2E del
  repository.
- `process.rs`: runner dei processi dell'automazione (git, hook `setup`/`cleanup`, E2E): output limitato
  (ultimi 64 KiB per stream, letti da thread), timeout e annullamento (`CancelFlag`) con
  uccisione dell'**intero albero** di processi: `taskkill /T /F` su Windows, in WSL anche i
  processi Linux marcati con `CLI_COMPANION_RUN` nell'ambiente (`wsl.exe` non li porta con sé).
- `e2e.rs` + `e2e/`: test E2E eseguiti dall'app, contratto in BOARD.MD, "Test E2E". `job`
  (esecuzione in background via `Host::defer`, mai sotto il lock del registro), `verdict` +
  `junit` (esito e parser, puri), `runs` (`E2eRuns`: una esecuzione per cartella, porte
  riservate, ultimi esiti manuali), `tools` (`run_e2e`, `get_e2e_result`), `manual` (comandi
  Tauri). Il blocco di `complete_task` è puro (`board/e2e.rs`, `check_gate`); l'impronta del
  codice (git) si calcola prima di prendere il lock e `Effect::CancelE2e` annulla le esecuzioni
  fuori dal lock. **Ordine dei lock**: il mutex di `E2eRuns` è una **foglia**: non si prende con
  `AutomationState::inner` in mano e sotto di esso non si prende nessun altro lock; l'attesa di
  `get_e2e_result` avviene fuori dal mutex, su una `Condvar` per esecuzione.
- `rpc.rs`: `dispatch` autentica la richiesta (id del tab + segreto), risolve il ruolo del chiamante e smista
  i metodi che la tabella di `roles.rs` gli consente (`Method::roles`, un `match` esaustivo: aggiungere un metodo
  obbliga a decidere chi lo può chiamare):

  | Metodo | Worker | Reviewer | Planner | Sessione |
  |---|:-:|:-:|:-:|:-:|
  | `whoami` (handshake, non è un tool) | sì | sì | sì | sì |
  | `get_task`, `complete_task` | sì | sì | sì | no |
  | `ask_user` (solo con l'Agents HUB acceso) | sì | no | sì | no |
  | `run_e2e`, `get_e2e_result` | sì | sì | no | no |
  | `propose_tasks`, `get_task_details` | no | sì | sì | sì |
  | `list_tasks` | no | no | sì | sì |
  | `get_workspace_settings` | no | sì | sì | sì |
  | `update_task`, `delete_task`, `propose_settings` | no | no | no | sì |
  | `save_skill`, `list_skills`, `delete_skill` | no | no | no | sì |

  Il ruolo viene dal tab: il worker di un task `work` è `worker`, quello di un task `review` è `reviewer`, quello di
  un task `plan` è `planner`, ogni altro tab di cli-companion (Intake compreso, workspace = quello del tab) è
  `session`. Un tab di un workspace archiviato è
  congelato. `ask_user` richiede l'HUB (`Method::requires_hub`): `whoami` riporta `hub` e senza HUB il tool non è
  nemmeno elencato (contratto in BOARD.MD, "Domanda all'utente"). `settings.rs`: patch validata in `workspaces/settings_patch.rs`, proposte in `board/settings.rs`.
  `review.rs` ha ciò che il revisore legge in `get_task`; `planner.rs` il task di pianificazione (creazione
  dall'app con `create_plan_task`, `get_task` e tool del pianificatore, proposta unica); `skills.rs` i tool delle skill utente (la logica su file è in
  `user_skills`).
- `board_files.rs`: le board su disco (`<app data>/boards/<workspace>.json`) e il loro archivio `.jsonl`; un
  `JsonFile` per workspace che vive per tutta l'esecuzione, così la protezione contro la riscrittura di un file
  illeggibile vale anche per i salvataggi successivi.
- `auto.rs`: fasce orarie e standby (sotto).
- `agent_session.rs`: legame worker <-> sessione dell'agente e riapertura dopo un
  `session-end` (sotto).

Stati del worker: `starting` -> `running` (`get_task`) -> `completed` | `failed`
(`complete_task`) -> `closed` (tab chiuso). In mezzo, dagli hook di `--notify` (solo con segreto
valido): `needs_input` (hook needs-input, tranne l'attesa `Idle`), `background`, `idle` e
`awaiting_answer`. `Registry::on_hook` su `finished`: con `background_work > 0` -> `background`;
con una domanda pendente (`ask_user`) -> `awaiting_answer`, senza bloccare lo Stop; altrimenti, con
`stop_hook_active != true`, risposta `block` con `STOP_BLOCK_REASON` (promemoria in inglese);
con `stop_hook_active == true` -> `idle`. `needs_input` e `idle` = "richiede attenzione" e
mandano la notifica. `awaiting_answer` (worker fermo dopo `ask_user`) **non** richiede attenzione,
perché l'utente risponde dall'HUB: la notifica è "<titolo> ha una domanda" (kind `question`).
Nessun hook segnala la risposta dell'utente: `pty_write` avvisa il backend
quando nel terminale di un worker in attesa si preme Invio (`Registry::user_input`) e il worker
torna `running`; in `awaiting_answer` Invio chiude anche la domanda, senza testo. Un permesso richiesto
durante la domanda porta a `needs_input`: lì Invio approva il permesso e la domanda resta aperta.

`automation::answer_question` (`automation/questions.rs`, azione dell'HUB e non un comando né un tool) consegna
la risposta dell'utente: la segna sulla board sotto il lock e la scrive nel tab del worker **fuori dai lock**
(`automation/answer_input.rs`: testo ripulito da caratteri di controllo ed ESC, una riga digitata com'è, più righe
in bracketed paste, Invio differito di 150 ms). Rifiuta la risposta con un permesso in attesa o mentre il worker
lavora; se il terminale non è raggiungibile la domanda torna in attesa. Errori e codifica in BOARD.MD.

Sessione dell'agente (`agent_session.rs`). Un processo secondario avviato dal worker (un
`claude` figlio, un subagent) eredita `CLI_COMPANION_SESSION` e gli hook del plugin: i suoi
eventi arrivano col tab del worker ma con un altro `session_id`. Il worker lega a se'
(`AgentSession`) il primo `agent_session_id` che vede (di norma dal suo `session-start`). Gli
eventi con un id diverso (`session-end`, `finished`, `needs-input`, ...) sono ignorati: niente
fallimento, niente promemoria dello Stop, niente attenzione. Un `session-start` con un altro id
rilega il worker solo se `source` e' `clear`, `resume` o `compact` (la stessa sessione utente che
continua); `startup` o `fork` di un altro id sono processi secondari. Un `session-end` della
sessione legata con `reason` `clear` o `resume` non chiude il worker: l'agente prosegue nello
stesso processo con una nuova sessione (che poi rilega col suo `session-start`). Eventi senza
id: ammessi finche' il worker non e' legato (plugin vecchi, comportamento invariato), estranei
dopo il legame, per tutti gli eventi. Ogni hook di Claude Code (`SessionStart`, `Stop`,
`SessionEnd`, `Notification`, `PermissionRequest`) porta `session_id` nello stdin, quindi un
evento senza id accanto a un id legato non viene dal worker (es. `--notify` lanciato a mano o da
un test nel suo terminale); estendere la regola anche a `finished` e `needs-input` evita
promemoria dello Stop e stati di attenzione falsi. Un `session-end` ignorato lascia una traccia
nella nota del task (`ignored a session-end: ...`, con l'id estraneo, `it carried no agent
session id` o il `reason`).

Riapertura. Se un `session-end` ha comunque chiuso il worker (stato `closed`, task `failed` con
la nota `the session ended without a complete_task report`) ma il suo tab e' ancora aperto,
il `complete_task` di quel worker riapre il task (`Board::reopen_failed`: solo se e' ancora
`failed` con quella nota e lo stesso `attempts` del worker, cioe' nessun retry l'ha ripreso),
poi lo conclude come sempre con il report (`done`/`failed`, pulizia, chiusura del tab). La
nota diventa `reopened: ...`. Tab chiuso davvero (`tab_closed`) o task ripreso da un retry:
`the session is closed`. La notifica `failed` gia' partita non si ritira; dopo la riapertura
vale la regola normale (notifica solo se il report e' `failed`, nessuna per `done`).

Notifiche di sistema: evento `task-notify` `{workspace_id, task_id, tab_id?, kind:
"attention"|"failed"|"question", title, body}` (testo in italiano); il frontend mostra la notifica.

Tutto cio' che legge il modello e' in inglese (`BOOTSTRAP_PROMPT`, `STOP_BLOCK_REASON`,
istruzioni, ambiente e passi di consegna in `board/texts.rs`, `board/prompts/worker_guardrails.md`,
descrizioni dei tool MCP, errori delle RPC). UI e commenti restano in italiano.

```text
Intake: propose_tasks --RPC--> board.proposals --board-update--> UI: Accetta --> coda (queued)
scheduler (dopo ogni modifica) --> running --> thread: base, worktree, setup --> emit automation-spawn
W: claude "<bootstrap>" --> get_task --> lavora, consegna (commit/push/PR/merge) --> complete_task
                                                          dopo 1,5 s: automation-close; chiuso il tab: pulizia
```

Pulizia: a `complete_task completed` il tab si chiude dopo 1,5 s; quando `automation_tab_closed`
lo conferma (e dopo 2 s, perche' Windows rilasci la cartella) l'app esegue i `cleanup` del
repository, `git worktree remove` e `git branch -D` (`pr`) / `-d` (`local_merge`). Gli errori
diventano una nota sul task. `failed` conserva worktree e branch per il retry. Se la pulizia
si perde (crash, chiusura dell'app) la rifa il riallineamento delle worktree (BOARD.MD, "Pulizia").

Eventi Tauri verso il frontend: `automation-spawn`, `automation-close`, `automation-update`,
`board-update`, `task-notify`, `schedule-state`. Comandi: `automation_tab_closed`,
`automation_sessions`, `board_get`, `board_set`, `task_create`, `task_update`, `task_action`,
`task_move`, `proposal_decide`, `settings_proposal_decide`, `workspace_detect_repos`, `e2e_run_repo`,
`e2e_last_result`, `e2e_open_report`, `schedule_get`, `schedule_set`, `frontend_ready`. Comando di avvio del worker:
`agents::automation_command`; permessi = `Workspace.worker_permission`, modello = `task.model`
poi `worker_agents[<agente del task>].model`, poi il predefinito dell'agente.

### Fasce orarie e standby (`schedule.rs`, `automation/auto.rs`)

`<app_data>/schedule.json` (`{enabled, windows:[{start,end}], keep_awake}`, ora locale `HH:MM`,
finestra `[start, end)` che puo' attraversare la mezzanotte, max 12). Logica pura testata
(`window_open`, `next_change`). Sono solo un **gate dello scheduler**: con schedule abilitato,
un workspace con `use_schedule` non avvia task fuori fascia (quelli in corso finiscono);
`BoardView.schedule_blocked` lo espone. Un thread dell'app fa un giro ogni 30 s (guardiano degli
avvii, `schedule-state` se cambia, le board il cui gate e' cambiato ripartono). `keep_awake`: con
il flag attivo l'app impedisce lo standby (Windows `SetThreadExecutionState`, thread dedicato)
finche' c'e' un task `running` in un workspace non archiviato. Comandi: `schedule_get`,
`schedule_set`.

### `mcp.rs`, `mcp/` — server MCP stdio (`cli-companion --mcp`)

- Ramo con early-return in `main.rs` come `--notify`. JSON-RPC 2.0 newline-delimited su stdin/stdout: `initialize`,
  `notifications/initialized`, `ping`, `tools/list`, `tools/call`; metodo ignoto con id → errore -32601. Su stdout passano
  solo messaggi MCP (una riga JSON ciascuno).
- È uno **shim**: ogni `tools/call` apre una connessione TCP a `127.0.0.1:<porta>` (`CLI_COMPANION_PORT`, default 17421;
  `ipc::rpc_call`), invia la riga RPC con `CLI_COMPANION_TOKEN` e `CLI_COMPANION_SESSION` dall'ambiente e restituisce il
  risultato come testo JSON (errore RPC → `isError: true`). Read timeout 30 s (`RPC_TIMEOUT`).
- Il ruolo si risolve con la RPC `whoami` e decide i tool esposti (`worker`, `reviewer`, `planner`, `session`: tabella unica in
  `automation/roles.rs`, che un test confronta con le definizioni di `mcp/tools.rs` e `mcp/tools/planner.rs`). Senza le variabili d'ambiente la lista
  è vuota, così il plugin non fa rumore fuori da cli-companion.
- **Handshake non bloccante.** `initialize` e `tools/list` non aspettano `whoami` oltre `ROLE_TIMEOUT` (3 s): Claude Code dà
  poco tempo all'avvio di un server. Se il ruolo non arriva, il server risponde con la lista vuota e un thread
  (`mcp/retry.rs`) riprova `whoami` con intervallo crescente (da 1 s fino a 10 s, per 5 minuti al massimo); appena riesce
  manda `notifications/tools/list_changed` e i tool compaiono. Lo stato condiviso fra ciclo di stdin e thread è in
  `mcp/state.rs`, il ciclo di vita in `mcp/session.rs`.
- Logica pura (`handle_line` con `trait Backend`), con orologio e attesa iniettabili (`Clock`): i test non dormono né
  aprono socket.

### `store/` — persistenza JSON comune

Componente unico per tutti i file strutturati dell'app: `workspaces.json`, `launch_settings.json`,
`bots.json`, `hub_settings.json`, `schedule.json`, una board per workspace (`boards/<workspace_id>.json`) e i
thread dell'Agents HUB (`threads/<workspace_id>.json`, `threads/general.json`). Un file e'
descritto da un `JsonFile<T>` statico (nome + catena di migrazioni, e opzionalmente
`.validated(check)` per il controllo semantico e `.decoded_with(f)` per leggere un elenco voce per
voce); `JsonFile::named` ne crea uno il cui nome e' noto solo a runtime (una istanza per file, con
il proprio blocco delle scritture) e `JsonFile::parse` applica busta, migrazioni e validazione a un
testo in memoria, senza I/O. `JsonStore<T>` tiene il dato in memoria come stato Tauri (`get`,
`load`, `update`). Un file e' una unita' di codice per responsabilita': `atomic`, `backup`,
`envelope`, `file`, `json_store`, `legacy`, `migrate`, `warnings`.

- **Formato**: `{ "schema_version": N, "data": ... }`, JSON indentato. Un file senza busta
  (cioe' qualunque JSON che non abbia ESATTAMENTE le chiavi `schema_version` intero non
  negativo e `data`) e' il formato storico: versione 0, l'intero valore e' il dato.
- **Migrazioni**: lista ordinata di passi puri `Value -> Value` (`Migration`); la versione
  corrente di un file e' il numero dei passi, quindi aggiungere un passo = incrementare la
  versione. Il passo 0 -> 1 introduce la busta (`adopt_envelope`, identita'; `launch_settings`
  ne usa uno proprio che migra anche `builtin:orchestrator`; il passo 1 -> 2 di `launch_settings`, `migrate_v1`,
  riscrive gli id `builtin:preset:*` in id `bot:*` come `workspaces::migrate_v2`; i due passi usano la tabella congelata
  di `launch_migration/ids.rs`). Al caricamento si applicano
  `steps[versione_del_file..]`; un passo che fallisce nomina il proprio indice nell'errore.
- **File corrotto** (non UTF-8, non JSON, migrazione o deserializzazione fallita, controllo
  semantico `validated` fallito): mai cancellato. Viene rinominato
  `<nome>.bak-<AAAAMMGG-OOMMSSmmm>` (UTC, ordinabile) e si riparte dal default; di ogni file
  si tengono gli ultimi 5 backup (i vecchi `*.json.bak` del comportamento precedente non
  vengono toccati). Il file nuovo nasce solo dal primo salvataggio esplicito.
- **Versione piu' recente** dell'app (`schema_version` > passi noti): trattata come corrotta
  (backup, warning `newer_version`), cosi' un salvataggio successivo non puo' distruggerla.
- **File illeggibile** (errore di I/O diverso da "non trovato": permessi, file bloccato,
  percorso che e' una cartella): il file non viene toccato, si usa il default, i salvataggi di
  quel file sono rifiutati con un errore esplicito finche' non si sistema e si riavvia l'app.
  Lo stesso vale se nemmeno il rename in backup riesce.
- **Avvisi**: ogni anomalia produce uno `StoreWarning { file, kind (corrupt | newer_version |
  unreadable | partially_read), message, backup, at }` (`partially_read`: alcune voci di un elenco
  erano inutilizzabili e sono state escluse, le altre caricate, il file originale copiato in un
  backup), scritto nel log, conservato in memoria (ultimi 100) e
  letto dal frontend con il comando `store_warnings`; l'evento Tauri `store-warning` lo
  notifica appena viene prodotto.
- **`write_atomic`**: file temporaneo unico, `write_all` + `sync_all`, poi rename; il
  temporaneo e' rimosso se qualcosa fallisce. Su Windows il rename riprova fino a 5 volte con
  pausa crescente (antivirus/indicizzatore possono bloccare il file per qualche ms).
- **Log**: `logging.rs` configura `tauri-plugin-log`, registrato per primo. File
  `cli-companion.log` nella cartella dei log dell'app (`LogDir`), rotazione a 1 MB con 5 file
  conservati; livello `Info` in release e `Debug` in debug (che scrive anche su stdout).
- **Tempi dell'avvio** (`startup.rs`): righe `startup: <passo> at +N ms` dall'inizio di `run`
  per ogni passo del setup, inizio e fine di ogni caricamento della pagina e `frontend_ready`
  (sessione ripristinata). La finestra di config nasce prima del setup, che gira sul main thread:
  lavoro lento li' ritarda il primo paint. Lo sfondo della finestra (`backgroundColor` in
  `tauri.conf.json`) e' il colore del tema, cosi' durante il caricamento non c'e' un flash bianco.
- **Migrazione dai vecchi identifier** (`store/legacy.rs`): al primo avvio la cartella dati di
  `com.jmagarelli.cli-companion` viene copiata per intero se quella nuova non esiste, e
  `workspaces.json` di `com.jmagarelli.muse-companion` se manca; le cartelle vecchie restano
  intatte.
- **Downgrade non supportato**: un file salvato con la busta, letto da una versione
  precedente dell'app (che si aspetta il formato senza busta), viene visto come
  corrotto/incompleto e messo da parte nel backup. Non si torna a una versione precedente
  senza ripristinare a mano i dati.

### `bots.rs` — nomi dei bot (impostazioni globali)

Ogni sessione (tab) è un "bot" con un nome e un avatar: tocco puramente visivo.
File `bots.json` nella cartella dati: `{ enabled, names, images }`, con `images` =
`{ nome: { png, color } }` (`#[serde(default)]`: i file vecchi restano validi). File assente o campo
mancante = `enabled: true` e la pool predefinita (`DEFAULT_NAMES`, unica fonte: il
frontend la riceve da `get_bot_settings`). Stesso schema di `launch_settings.rs`: un
`JsonFile` statico (`store/`) e un `JsonStore` gestito da Tauri; un file corrotto non viene
cancellato ma messo da parte come `bots.json.bak-<timestamp>`, `update()` salva (scrittura
atomica) prima di aggiornare la memoria. Comandi `get_bot_settings`, `set_bot_settings(enabled,
names)` (ritorna le impostazioni salvate), `reset_bot_names`, `set_bot_image(name, png,
color)` e `remove_bot_image(name)` (ritornano anch'essi le impostazioni salvate). Validazione
dei nomi: trim, 1-24 caratteri, niente caratteri di controllo, niente duplicati (senza
maiuscole), al massimo 200 nomi; la lista vuota è ammessa. Validazione delle immagini:
il nome deve essere nella pool (senza maiuscole; si salva con l'ortografia della pool),
`png` base64 valido con firma PNG e IHDR al massimo 64x64, al massimo 32 KB, `color` nel
formato `#rrggbb`. Le immagini dei nomi che escono dalla pool sono eliminate da
`set_bot_settings` e `reset_bot_names`.

**Bot-agente** (`agents`, schema v2). Nello stesso file stanno i Bot come agenti specializzati e
globali: `AgentBot { id, name, task, agent, model, effort, skill, enabled, image, preset }`
(`bots/agent.rs`). `agent: null` = l'agente del workspace; `model` è `ModelChoice` (`default`: decide
la CLI; `tier`: `standard|strong`, risolto sull'agente effettivo con `tier_models`; `id`: un modello
concreto, ammesso solo con l'agente fissato); `skill` è `SkillRef` (`plugin`: id di
`agents::presets::PRESETS`; `user`: nome di una skill globale, la cui esistenza si verifica al
lancio). I predefiniti (`bots/presets.rs`: `general`, `intake`, `setup`, `skill-wizard`, tutti con
l'agente del workspace) arrivano con passi di migrazione additivi e congelati (`bots/migrations.rs`,
v1→v2): saltano gli id presenti e girano una volta sola, quindi un Bot cancellato non ricompare. Il
Bot `general` è configurabile ma non si può eliminare né disattivare; se manca da un file scritto a
mano lo rimette la lettura (`decode_settings`). Gli id dei Bot dell'utente cominciano con `custom-`.
Validazione al salvataggio (`bots/validate.rs`, pura, con gli agenti da `AgentCatalog`): nome come
la pool e unico fra i Bot e i nomi dei segretari dell'HUB, compito di 1-300 caratteri su una riga,
agente fissato registrato e abilitato, modello `id` valido per l'agente fissato, effort supportato
dall'agente fissato (con l'agente del workspace si ammette), skill del plugin esistente e nome di
skill utente valido. `bots/resolve.rs::resolve_bot` (pura) traduce un Bot in agente, modello,
effort e skill di un lancio: ciò che l'agente effettivo non sa applicare si toglie con un avviso
in inglese. Comandi (`bots/commands.rs`): `list_agent_bots`, `create_agent_bot(draft)`,
`update_agent_bot(id, draft)`, `delete_agent_bot(id)`, `restore_agent_bot_preset(id)` (compito,
modello, effort e skill tornano quelli del predefinito), `restore_agent_bot_presets` (ricrea i
mancanti) e `list_bot_skills` (skill del plugin e dell'utente). Una versione precedente dell'app
vede un `bots.json` v2 come più recente e lo mette da parte.

### `hub/` — Agents HUB

Modalità opzionale, spenta di default (`hub_settings.json`: `enabled`, `provider`, `language`, `session`,
`secretaries`): un canale per workspace più il
Generale, e in ogni canale thread di messaggi. Spenta, non crea file e non cambia nulla nel resto dell'app. Un
file per responsabilità: `channel` (`general` o l'id di un workspace, stringa semplice sul filo), `model` (DTO
con prefisso `Hub` nei bindings), `text` (validazione, messaggi in italiano), `threads` (operazioni pure e
limiti; `threads/lifecycle.rs` per chiusura, collegamenti e ricerca del thread di un task o di una proposta),
`waiting` (`waiting_user` calcolato dagli eventi), `files` (`threads/<canale>.json` su `store/`), `state`
(`HubState`: caricamento pigro, un `JsonFile` vivo per canale), `settings` (con `settings/`: `provider`, `language`,
`session`, `secretary`, `secretaries` puri, `tolerant` per leggere i valori di una versione futura senza perdere il
file, `commands`), `api_key` (la API key del provider nel portachiavi, tramite `secrets::SecretStore`; il frontend
ne vede solo la presenza), `commands` ed `events` (eventi della
board nei thread: istantanea compatta, diff e instradamento puri, `HubFeed` con un thread dedicato che riceve le
viste da `Host::board_published` e scrive fuori dai lock della board; all'accensione fissa come base le viste
correnti di tutte le board, chieste alla sorgente registrata da `lib.rs`; l'instradamento porta la proposta di un
piano nel thread del piano e indica quali eventi svegliano il segretario titolare del thread), `actions` (le azioni
sulla board registrate da `lib.rs`), `append` (un messaggio di testo con
salvataggio ed evento, per `hub_send` e per il motore), `attachments` (gli allegati dei messaggi dell'utente: metadati
nel messaggio, file in `hub_attachments/`, caricamento in due fasi con lo staging, pulizia dei file di messaggi e
thread potati calcolata da `HubState::update_when` con una funzione pura, comandi) ed `engine` (il motore dei segretari: menzioni e titolare,
risvegli da evento della board, sessioni in memoria con una coda per thread e uno schedulatore unico delle scadenze,
ciclo di tool calling con i tool `board_status`, `thread_read`, `post_message`, `open_plan_task` e `decide_proposal`
(`engine/planning.rs`: la proposta si lega solo alla risposta dell'utente al suo evento), riassunto del thread alla
chiusura; logica pura o generica sui trait `ChatModel`, `TurnHost` e `ProposalBoard`, con un modello e una board
finti nei test; client reale `async-openai`). Comandi
`hub_get_settings`, `hub_set_enabled`, `hub_set_provider`, `hub_set_api_key`, `hub_clear_api_key`,
`hub_set_language`, `hub_set_session`, `hub_create_secretary`, `hub_update_secretary`, `hub_delete_secretary`,
`hub_set_secretary_image`, `hub_remove_secretary_image`, `hub_list_threads`, `hub_get_thread`, `hub_create_thread`, `hub_send`,
`hub_stage_attachment_file`, `hub_stage_attachment_bytes`, `hub_read_attachment`, `hub_attachment_path`,
`hub_secretary_status`; eventi `hub-update` (riassunto del thread toccato) e `hub-secretary-status` (segretari svegli).
Spegnere l'HUB chiude le sessioni dei segretari e annulla i loro lavori. La rimozione di un workspace cancella il suo file dei thread e i suoi allegati
(`hub::forget_workspace`, chiamato da `remove_workspace` dopo la board), l'istantanea della sua board e le sessioni dei
segretari nel suo canale. Modello, file, limiti e scelte:
[HUB.MD](HUB.MD).

### `connectors/` — connettori dell'Agents HUB

Estende l'HUB con due tipi di connettori: le **Fonti** (`source`), che portano elementi esterni (es. email di Gmail
filtrate da una regola) nei thread di un canale e possono svegliare un segretario, e i **Canali di contatto**
(`contact`, es. Telegram), che portano fuori le notifiche dei thread e riportano dentro i messaggi dell'utente.
Consegnati le fondamenta nell'HUB e il runtime (fasi 1 e 2); i connettori reali e l'interfaccia generica seguono.
Livello 4 accanto ad `automation` e `mcp`: dipende da `hub` solo attraverso la sua API pubblica (`hub::ingress`,
`hub::send`, `HubLifecycle`, `thread_exists`) e riceve le notifiche come `NoticeSink` registrato da `lib.rs` in `setup`
(`connectors::start`); `hub` non dipende da `connectors`. Come per gli agenti, nessun file fuori da
`connectors/<id>/` nomina un connettore, salvo il registro (`connectors/architecture.rs` e `tests/architecture.test.mjs`).

- **Un supervisore per istanza** (`runtime/supervisor.rs`): un task su `tauri::async_runtime`, solo con l'HUB acceso e
  l'istanza abilitata, fermato con `JoinHandle::abort`. `ConnectorsRuntime` (`runtime/manager.rs`, stato gestito)
  tiene `connectors.json` in memoria, gli stati di salute e i task; il suo `reconcile` allinea i task al file.
- **File**: `connectors.json` (istanze, regole, collegamenti) e `connector_state/<istanza>.json` (cursori, id visti,
  mappe, tutto limitato) su `store/`; i segreti solo nel portachiavi, mai nei file né nei DTO.
- **Pura e I/O separate**: backoff, stato, validazione, decisioni dei comandi (`commands/checks.rs`), un giro di
  una fonte (`runtime/round.rs`) sono generici su `HubPort`, `Clock` e `Sleeper`; i test li guidano con finti
  (`connectors/testing.rs`), senza rete né `AppHandle`.
- **`tokio` come dipendenza diretta** (`default-features = false`, `time`, `sync`, `macros`): serve a `sleep` e
  `timeout` dei supervisori, ai canali limitati delle notifiche e a `select!`. È già nel grafo via tauri e reqwest:
  non aggiunge nessun crate.
- **Comandi Tauri** in `connectors/commands/` (istanze, segreti, azioni, regole, collegamenti, stato) ed evento
  `connector-status`; frontend in `src/stores/connectors.ts`.

Contratti, file, affidabilità, comandi, sicurezza, alternative valutate e fasi: [CONNECTORS.MD](CONNECTORS.MD);
ricetta per un connettore nuovo: [wiki/integrare-un-connettore.md](wiki/integrare-un-connettore.md).

### Altri moduli

- `session.rs`: file nella cartella dati dell'app, scritti con `write_atomic`.
  - `session.json`: blob JSON opaco (lo schema è del frontend, campo `version`), tetto 2 MB. Comandi `load_session` /
    `save_session`.
  - `scrollback/<tab_id>.txt`: cronologia serializzata di un terminale, tetto 4 MB. `save_scrollback`,
    `load_scrollback`, `delete_scrollback`; il `tab_id` è validato con `ipc::is_valid_tab_id`.
    `prune_scrollback(keep)` elimina i file dei tab non elencati (chiamato dopo il ripristino).
- `launch_settings.rs`: avvio predefinito globale dei nuovi terminali (`"shell"`, un integrato o un Bot abilitato) e flag
  `show_usage`; schema 2 (`migrate_v1`: gli id `builtin:preset:*` diventano `bot:*`; una versione precedente dell'app
  vede il file come più recente e lo mette da parte); stesso schema
  di `bots.rs` (un `JsonFile` statico + un `JsonStore` gestito da Tauri). Comandi `get_launch_settings`,
  `set_global_default_launch`, `set_show_usage`.
- `user_skills.rs` (+ `user_skills/`): logica pura sulle skill globali dell'utente (`<app data>/user-skills/<nome>/`,
  sincronizzate in `<skills dir>/<nome>/` con il marcatore `.cli-companion-managed`; una cartella senza marcatore è scritta a
  mano e non viene mai sovrascritta né cancellata). Le destinazioni sono quelle dichiarate dal contratto `UserSkills`
  delle integrazioni (Windows e distro WSL). Errori in inglese perché li legge il modello via RPC. Il ripristino all'avvio
  gira in un thread (`automation::skills::restore_in_background`).
- `awake.rs`: blocco dello standby (Windows `SetThreadExecutionState` su un thread dedicato).
- `updater.rs`: solo `--check-update` da riga di comando; l'aggiornamento vero lo fa la GUI con `tauri-plugin-updater`
  (vedi [UPDATER.MD](UPDATER.MD)).
- `logging.rs`: vedi "`store/`", voce Log.

### `lib.rs`

Registra i plugin (`log` per primo, poi `opener`, `dialog`, `clipboard-manager`, `notification`, `updater`, `process`), gli
stati condivisi, il caricamento in `setup` (vedi "Avvio") e tutti i comandi in un solo `generate_handler!`. Contiene
anche `blocking`: esegue lavoro bloccante (processi esterni, rete, filesystem) con `spawn_blocking`, perché i comandi
Tauri sincroni girano sul main thread e uno lento congelerebbe la finestra; i comandi che toccano processi o disco sono
`async` e passano da lì. Capability minime in `capabilities/default.json`: i comandi dell'app non richiedono permessi,
ci sono solo quelli dei plugin e della finestra (`core:window:allow-destroy` per la chiusura dopo il salvataggio, i
permessi di titolo senza decorazioni, `opener:default`, `notification:default`, `dialog:allow-open`, clipboard, `updater`,
`process:allow-restart`).

## Frontend (`src/`)

### Struttura e convenzioni

```text
src/
  App.tsx            composizione: avvia gli hook di fondo e dispone le aree (titlebar, sidebar, workbench, statusbar)
  stores/            stato dell'app: un archivio per tema + transizioni pure (testate da node --test)
  app/               azioni e gestori degli eventi del backend: funzioni di modulo che leggono gli archivi con getState()
  hooks/             hook condivisi: useStore/useStores, useTauriEvent, caricamento iniziale, sessione, scorciatoie, ...
  lib/               logica pura e senza React: wrapper di Tauri (tauri.ts), archivio generico (store.ts), errori, layout, ...
  components/        componenti React; gli hook di una feature stanno accanto ai suoi componenti
  bindings/          tipi generati dal Rust (ts-rs): non si modificano a mano
```

- **Archivi** (`lib/store.ts`, `stores/*.ts`). `createStore(initial)` è un archivio osservabile minimale
  (`getState`, `set`, `update`, `subscribe`; nessuna notifica se lo stato non cambia per `Object.is`). Ogni file di
  `stores/` ha lo stato, le **transizioni pure** (`(stato, argomenti) => stato`, che ritornano lo stesso stato se non
  cambia nulla, così l'archivio non notifica) e una factory che le lega a un archivio (`workspacesStore`, `tabsStore`,
  `runtimeStore`, `layoutStore`, `boardsStore`, `configStore`, `uiStore`, `usageStore`, `paneDragStore`).
  L'identità dei tab (`tabs.ts`) è separata dal loro stato volatile (`tabRuntime.ts`: titolo OSC, stato dell'agente,
  metadati del worker, busy), così un titolo che cambia non ridisegna ciò che dipende solo dall'elenco dei tab.
  `selectors.ts` ha le derivazioni pure condivise (etichette, stati aggregati, tab della barra del titolo), `view.ts`
  cosa è mostrato al posto dei terminali e `session.ts` la parte pura della persistenza.
- **Lettura nei componenti** (`hooks/useStore.ts`). `useStore(store, selector?, isEqual?)` usa
  `useSyncExternalStore`: il componente si ridisegna solo quando la selezione cambia (`shallowEqual` per array e oggetti
  piccoli); `useStores` deriva un valore da più archivi mantenendo stabile il riferimento. Nessun componente tiene copie
  dello stato.
- **Azioni** (`app/actions/*.ts`, `app/events.ts`, `app/boards.ts`, `app/usage.ts`, `app/lifecycle.ts`,
  `app/sessionSaver.ts`). Funzioni di modulo (quindi con riferimento stabile) che leggono con `getState()`, chiamano
  `api` e aggiornano gli archivi: aprire/chiudere un tab (`tabs.ts`), spostare i pannelli (`panes.ts`), cambiare vista
  (`view.ts`), le impostazioni (`settings.ts`), i workspace (`workspaces.ts`).
- **Eventi del backend** (`hooks/useTauriEvent.ts`). `useTauriEvent(evento, handler)` ascolta per tutta la vita del
  componente con `listen` di Tauri; l'handler sta in un ref (cambia a ogni render senza riregistrare il listener) e
  `startSubscription` (`lib/subscription.ts`) garantisce che un cleanup arrivato prima della risoluzione della promessa
  non lasci un listener orfano. Nomi e payload sono in `TauriEvents` (`lib/tauri.ts`). Gli eventi dei coding agent e
  dell'automazione li registra una volta sola `hooks/useAppEvents.ts` con i gestori di `app/events.ts`.
- **Errori** (`lib/errors.ts`). Un errore non si ignora: `report(ambito, err, { notify? })` lo scrive sempre nel log e,
  con `notify: true`, lo mostra come avviso (`lib/notices.ts` → `components/NoticeCenter.tsx`), con l'ambito in italiano
  ("caricamento degli avvii integrati"). Un errore davvero ignorabile si dichiara con `ignoreError("motivo")`, che
  richiede il motivo e lo lascia nel log di debug. I comandi Tauri rifiutano con stringhe: `errorMessage` le normalizza.
- **Convenzione hook**: gli hook React condivisi dall'app stanno in `src/hooks/`, quelli di una feature accanto ai
  componenti della feature (es. `components/tasks/useTaskPanelCollapsed.ts`); `src/lib/` contiene solo logica pura / moduli
  non React.
- **Moduli importabili dai test.** `lib/` e `stores/` sono zero-dipendenze (nessun import di React, Tauri o CSS) e
  importano con l'estensione `.ts` e solo `import type` dai bindings: `node --test tests/*.test.mjs` li carica
  direttamente con il type-stripping di Node, senza bundler. Le funzioni che toccano l'ambiente (clock, random,
  `localStorage`) lo ricevono come argomento o lo avvolgono in try/catch (`lib/browserStorage.ts`).

### Funzionalità e componenti

- **Bot** (`lib/bots.ts`, zero-dep, `tests/bots.test.mjs`): `pickBotName(pool, taken)`
  sceglie a caso un nome non usato dai tab aperti (pool esaurita: suffisso numerico,
  "Bolt 2"; pool vuota: "Bot 1"...); `botLook(name)` ricava l'aspetto dell'avatar da un
  hash stabile del nome (colore, testa, antenna, occhi, accessorio) e `botPixels(look)`
  lo disegna su una griglia 12x12; `assignRestoredNames` assegna i nomi dei tab
  ripristinati (il secondo con lo stesso nome ne riceve uno nuovo). Il campo `bot` di
  `TabEntry` (`stores/tabs.ts`) è assegnato all'apertura di ogni tab (shell, Claude, Intake,
  worker `automation-spawn`) anche a funzione spenta, e si libera alla chiusura. `components/BotAvatar.tsx` è l'SVG
  `crispEdges` scalabile (riusabile più grande); avatar e nome compaiono, solo con
  `enabled`, in `PaneHeader`, nella lista terminali della `Sidebar` e in
  `PanelRunningCard` (`botOf(tab)` da `components/workbench/TaskPanelArea.tsx`, che li legge dagli
  archivi). Pagina Impostazioni › Generale › Bot
  (`settings/BotsPage.tsx`): interruttore, elenco con rimozione, aggiunta, ripristino
  con conferma; ogni modifica è salvata subito e scritta in `configStore` (`app/actions/settings.ts`).
  **Impostazioni › Generale › Bot** ospita anche i Bot-agente (`settings/bots/`: `AgentBotsSection` elenco, `AgentBotRow`,
  `AgentBotEditor` con `BotAgentFields`; la pool dei nomi sta in `BotNamePoolSection`). La logica del form è pura in
  `lib/botEditor.ts` (`tests/bot-editor.test.mjs`): compito, agente (“Segue il workspace” o un agente fissato), modello
  (predefinito, fascia o id concreto solo con l'agente fissato), effort (solo i livelli dichiarati dall'agente; con
  “Segue il workspace” la nota avverte che altrove viene ignorato) e skill (`useBotSkills`: plugin e utente). `general` non
  si elimina né si disattiva; “Ripristina” riporta un predefinito alla sua configurazione (anche i `kinds`). `BotKindsField` sceglie i **Tipi di
  task** del Bot (Lavoro, Review, Pianificazione; nessuno = «Solo terminali interattivi», mai scelto per i task);
  `Reviewer` e `Planner` sono predefiniti con `[review]` e `[plan]` (dettagli in BOARD.MD, “Tipi di task dei Bot”). Gli errori del backend si
  attribuiscono al campo (`fieldOfBackendError`).
  **Bot nei task** (`lib/taskBot.ts`, `tests/task-bot.test.mjs`): `BotBadge` (avatar e nome) su `KanbanCard`, `ListRow`,
  `ProposalCard` e `DetailFacts`; selettore “Bot” in `NewTaskDialog` e `DetailTaskFields` (`botOptions`: solo i Bot
  abilitati, più il Bot corrente se non è più scegliibile; “Nessun Bot” solo per i task vecchi; `defaultBotChoice`
  preseleziona `general` se esegue il tipo, altrimenti il primo Bot che lo esegue; cambiando il tipo `botAfterKindChange`
  tiene il Bot o passa al default del tipo con un avviso, e `botKindError` blocca l'invio se nessun Bot abilitato
  esegue il tipo). Mappa dei badge (Review, Pianificazione, verdetto, E2E, Bot, Agente) in BOARD.MD, “Badge dei task”. Con un Bot il selettore dell'agente sparisce (l'agente è del Bot), il modello è un override
  facoltativo per l'agente del Bot e cambiando Bot il modello si svuota (`botChange` in `lib/taskForm.ts`). Agente,
  modello, effort e skill dello snapshot li risolve il backend al salvataggio, non l'interfaccia.
  **Immagini dei bot**: l'utente associa un'immagine a un nome della pool e il bot con
  quel nome la usa come corpo. `lib/botImage.ts` (zero-dep, `tests/bot-image.test.mjs`) è
  la pipeline di pixelazione, pura: ritaglio dei margini trasparenti, adattamento in
  24x34 (`fitSize`), ricampionamento a media d'area, alfa binario con soglia al 50% e
  colore dominante; decodifica del file (`createImageBitmap`, pre-scala a 512 px) ed
  esportazione PNG stanno nello strato canvas `settings/pixelateFile.ts`. `botImageOf(settings,
  name)` risolve l'immagine di un bot ("Bolt 2" usa quella di "Bolt") e `botImageHref`
  ne fa l'URL `data:`. `botImageResolver(bots)` (`stores/config.ts`, con cache: stesso oggetto
  finché le impostazioni non cambiano) è il risolutore che i componenti passano come prop `botImage` accanto a `bot`:
  `TerminalView` → `PaneHeader`, `SidebarTerminal`, `botImageOf(tab)` dei pannelli Task →
  `PanelRunningCard`, `OfficeSession.botImage` → `OfficeRoom` → `OfficeBot` → `BotSprite`.
  `BotAvatar` disegna l'immagine in un'`<image>` 12x12 (`contain`, `image-rendering:
  pixelated`) al posto della testa. `BotSprite` la disegna al posto di busto e testa nella
  zona del corpo (x 2-14, y 0-17, appoggiata sulle gambe e centrata, `imageBodyBox`);
  braccia e gambe restano, nell'ombra del colore dominante, e le braccia (`armShiftOf`)
  si spostano verso il centro e in basso (al massimo 3 px) per restare attaccate a
  immagini strette o basse. Nella pagina Bot ogni riga ha i bottoni scegli/cambia/rimuovi
  immagine; dopo la scelta compare l'anteprima (sprite e avatar) con conferma.
- **Vista Ufficio** (`lib/office.ts` + `officeLayout.ts` + `officePlacement.ts`, zero-dep,
  `tests/office.test.mjs`; `components/office/`): una stanza per workspace (nell'ordine della
  Sidebar), un bot per ogni tab aperto. La logica pura copre la mappatura dell'attivita'
  (working / idle / attention / background / completed / failed), le scrivanie stabili
  (`assignDesks`; la mappa la tiene `Workbench` con `useStableDesks(tabs)`, su tutti i tab anche ad
  Ufficio chiuso, quindi le scrivanie restano le stesse tra le aperture e si azzerano solo al
  riavvio dell'app), le stanze, la geometria (`roomLayout`: mobili, scrivanie, spot, aree
  di chiacchiera, server), la pianificazione dell'idle con `random` e orologio iniettati
  (`planIdle`: mete di 4-10 s, chiacchiere di 2-3 bot della stessa stanza, mai due bot sullo
  stesso posto), i posti fissi per il movimento ridotto (`restingGoals`) e posizione/posa di
  ogni bot (`botPlacement`). `OfficeView` e' un overlay z-30 sopra pannello Task, griglia e
  Tasks Manager (z-20), montato solo mentre e' aperto (i `TerminalView` non si toccano; pannello
  e griglia sono `inert` come col manager e l'`.xterm` col focus viene sfocato). L'unico timer
  e' un `setInterval` da 1 s in `useIdleGoals`, attivo solo ad Ufficio aperto, senza movimento
  ridotto e con almeno un bot in idle; le mete vivono li' (mai in un archivio). Niente
  `requestAnimationFrame`: i bot si spostano con una transizione CSS su left/top (durata =
  distanza / velocita') e posa e camminata sono animazioni CSS (`office.css`, spente con
  `prefers-reduced-motion`). Ctrl+Maiusc+O (`useOfficeShortcut`, fase di cattura come
  Ctrl+Maiusc+B) e il pulsante "Ufficio" (nel footer della Sidebar, o nella barra dei mondi con l'Agents HUB acceso) lo aprono/chiudono; non fa
  nulla con un dialogo aperto o con i bot spenti (che lo chiudono). Il click su un bot chiama
  `focusTerminal`; anche il click su un terminale della Sidebar, una notifica o l'apertura del
  Tasks Manager lo chiudono.
- `lib/layout.ts`: funzioni pure zero-dep, testate da `tests/layout.test.mjs`.
  `gridRects(n)` = griglia automatica (cols = ceil(sqrt n), rows = ceil(n/cols), ultima
  riga incompleta allargata a tutta larghezza, rettangoli in %), `reorderIds`,
  e le transizioni di stato dei gruppi (`openTab`, `closeTabIn`, `selectGroup`,
  `setActivePane`, `toggleMaximize`, `reorderTabs`, `computePaneLayouts`).
- **Gruppi per workspace**: la barra del titolo ha un tab per ogni workspace con almeno un
  terminale (nome, numero di terminali, stato aggregato: ❗ > ✓ > pallino giallo).
  Ordine = prima apertura; aprire un terminale ne attiva il gruppo; il click su una card
  della Sidebar (non sui pulsanti) seleziona il gruppo; l'ultimo terminale chiuso fa
  sparire il gruppo e ne seleziona un altro.
- **Pannelli**: ogni terminale ha un'intestazione sottile (etichetta, stato, massimizza,
  chiudi; `PaneHeader.tsx`). Click ovunque nel pannello = pannello attivo + focus xterm.
  Trascinando l'intestazione su un altro pannello dello stesso gruppo (HTML5 DnD, per
  questo `dragDropEnabled: false` in `tauri.conf.json`) si sposta in quella posizione.
  Maximize: bottone, doppio click sull'intestazione o Ctrl+Maiusc+M (listener in
  cattura su `window`; niente Esc, serve alle CLI); per gruppo, gli altri pannelli
  restano vivi e nascosti.
- **Vista globale**: il 📌 nell'intestazione fissa un terminale; i fissati compaiono nel
  tab "Vista globale (N)" (primo della barra, visibile solo con N ≥ 1, con l'indicatore
  aggregato ❗/✓/in corso). È un gruppo speciale in `layout` (`GLOBAL_ID`, `global`:
  `order`/`activeTabId`/`maximizedTabId`, più `activeView`) con griglia, riordino,
  maximize e Ctrl+Maiusc+M propri; le intestazioni mostrano "workspace · etichetta". Il
  tab resta anche nel suo gruppo: ha un solo rettangolo, preso dalla vista mostrata
  (`computePaneLayouts`), le altre viste lo nascondono (`active`/`rect`, come al cambio
  gruppo, quindi fit + `pty_resize` automatici). Chiudere un tab lo toglie dalla vista.
  `focusTab` (click su notifica) e il click sulla Sidebar vanno sempre al gruppo del
  workspace. Sessione: `global` e `activeView` opzionali in v1 (nessun bump di versione).
- **Nessun rimontaggio**: tutti i `TerminalView` di tutti i gruppi sono figli diretti di un
  solo contenitore, con `key` = tabId e nell'ordine di creazione. Riordino, cambio
  gruppo, maximize e ripristino cambiano solo lo stile (rettangolo in % /
  `display: none`), mai l'albero DOM: smontare un `TerminalView` chiude il pty. `active`
  = visibile (gruppo attivo e non nascosto da un maximize), `focused` = riceve la tastiera.
- **Persistenza** (`lib/session.ts`, zero-dep, `tests/session.test.mjs`; la parte pura del salvataggio e del
  ripristino dei tab è in `stores/session.ts`, il debounce in `app/sessionSaver.ts`, il collegamento al backend e
  alla chiusura della finestra in `hooks/useSessionPersistence.ts`): schema
  `{ version: 1, activeWorkspaceId, groups: [{ workspaceId, order, activeTabId,
  maximizedTabId }], tabs: [{ tabId, workspaceId, launchId?, command, title?, agentId?,
  agentSessionId?, botName? }] }` (`botName` opzionale, nessun bump di versione). Salvataggio con debounce di 500 ms a ogni modifica e alla
  chiusura (`onCloseRequested`, con tetto di 3 s). `parseSession` tollera garbage, versioni
  sconosciute, id non validi e tab di workspace spariti (li scarta, ricostruendo i gruppi).
  Al primo avvio dopo il caricamento dei workspace i tab si ricreano con gli stessi tabId:
  ogni `TerminalView` scrive prima la cronologia salvata e la riga
  `— sessione ripristinata —`, poi lancia il pty. Il comando è `restoreCommand(tab)`:
  con `agentSessionId` valido e comando `claude`/`claude.exe` è il comando originale senza
  `--resume/-r/--continue/-c` più `--resume <id>`; con id ma senza comando claude è
  `claude --resume <id>`; altrimenti il comando originale. Poi `prune_scrollback`.
- **Scrollback**: `@xterm/addon-serialize` (ultime 2000 righe con colori, senza buffer
  alternativo né modi) salvato ~5 s dopo l'output e alla chiusura dell'app; eliminato
  alla chiusura del tab (anche se la shell esce da sola).
- **Finestra senza decorazioni** (`decorations: false`, `shadow: true`; il resize dai
  bordi lo gestisce tao): `components/TitleBar.tsx` (44px) ha logo, tab dei gruppi
  (`★ Globale (N)` per primo, poi i workspace; tab attivo fuso col contenuto sotto, tinta
  arancio se serve una risposta; scroll orizzontale in overflow) e i pulsanti
  minimizza / massimizza-ripristina / chiudi (46px, chiudi rosso). Le zone libere sono
  `data-tauri-drag-region` (trascina; doppio clic = massimizza); i controlli no.
  Chiudi usa `close()`, quindi passa dall'`onCloseRequested` registrato da
  `hooks/useSessionPersistence.ts` (`flushBeforeExit`: salva sessione e scrollback), mai `destroy()` diretto. Capability: `core:window:allow-{minimize,
  toggle-maximize,close,start-dragging,is-maximized}`.
- `components/sidebar/Sidebar.tsx` (260px; `AppSidebar.tsx` la monta): intestazione "WORKSPACE" + "+" (aggiungi workspace);
  riga = chip ambiente (`WIN`/`WSL`), nome, percorso mono. Le righe non attive sono
  compatte (al passaggio del mouse: "+" nuovo terminale, cartella, menu "⋯"); il
  workspace selezionato e' una card con l'elenco dei suoi terminali (stato, etichetta,
  pin; il click usa `focusTab`) e il pulsante diviso "+ Terminale" (avvio predefinito) con
  caret sul menu degli avvii. Menu "⋯": avvii, "Gestisci avvii…", Rinomina, Elimina
  (`DropdownMenu modal={false}`; le Impostazioni si aprono con un `setTimeout` a menu
  chiuso). Rinomina inline, elimina con doppia conferma. Footer: Impostazioni (pallino
  accent se c'e' un update), pulsante "Ufficio" (solo con i bot attivi) e versione (`getVersion()`); con l'Agents HUB
  acceso il footer non c'e': Impostazioni, Ufficio e versione stanno nella barra dei mondi (`components/rail/WorldRail.tsx`,
  64px a sinistra, con Terminali e HUB). Ctrl+Maiusc+H (`useShortcuts`) passa dal mondo Terminali al mondo Agents HUB
  (`UiState.world`, `stores/view.ts` `activeWorld`; non persistito): la colonna di 260px mostra `AppSidebar` o `HubSidebar`
  e l'overlay `HubOverlay` copre i terminali (che restano montati). I segretari: sezione SEGRETARI nella colonna
  (`components/hub/HubSecretaryList.tsx`), messaggi, menzioni e compositore in `components/hub/`, logica pura in
  `lib/hubSecretaries.ts`, domande dei worker in `HubQuestionCard` (con `HubDeliveryFailure` per la risposta non
  consegnata), `lib/hubQuestions.ts` (stato di una domanda ricavato dal thread) e `answerQuestion` / `openTaskThread`
  in `app/actions/hub.ts`, impostazioni in `components/settings/HubPage.tsx` e `components/settings/hub/` (con
  `lib/hubSettings.ts` e `app/actions/hubSettings.ts`), nomi riservati ai terminali in `lib/bots.ts`. Dettagli in
  [HUB.MD](HUB.MD), "Interfaccia".
- `components/PaneHeader.tsx` (30px): stato, etichetta mono (prefisso "workspace /" nella vista
  globale), parola di stato dell'agente, pin / massimizza / chiudi. Area dei pannelli su
  `--pane-area` con 6px di margine; pannello attivo col bordo accent; bersaglio di
  trascinamento tratteggiato con "Rilascia qui per spostare"; stato vuoto con logo.
  Resta valido: i `TerminalView` sono figli diretti di un unico contenitore fisso.
- `components/StatusBar.tsx` (26px): "N terminali", "K in attesa di risposta" e, a destra, gli
  ambienti con il pacchetto installato (`installedAgentsSummary`) delle sole integrazioni abilitate.
  Le integrazioni stanno in `configStore.integrations` (`integrations_list` lancia processi: letto
  all'avvio da `useIntegrationsSync`, riletto a ogni evento `integrations-changed` e dopo
  installa/rimuovi, vedi `src/app/actions/integrations.ts`); i componenti leggono i selettori
  `enabledIntegrations`, `defaultIntegration` ed `enabledEnvs`, mai l'elenco grezzo.
- **Tema e brand**: tema dark unico in `src/index.css` (palette `base`, `surface`, `surface-2`,
  `border-strong`, `text-secondary`, `faint` (solo decorativo), `pane-area`, `pane-header`,
  stati `status-running/attention/finished`), font Geist / Geist Mono bundlati (offline),
  logo "C" in `components/Logo.tsx` (icone in `src-tauri/icons`, sorgenti in `icons/source`).
- `components/AddWorkspaceDialog.tsx`: modal con toggle Windows/WSL.
  Windows: picker cartella nativo (`@tauri-apps/plugin-dialog`) o path manuale.
  WSL: lista distro con caricamento async, input percorso con bottone "Verifica".
- `components/SettingsDialog.tsx`: due tab, "Avvii personalizzati" (selettore
  workspace, lista/modifica/elimina/predefinito, form di aggiunta; ogni
  modifica è salvata subito via comandi) e "Integrazioni" (`settings/IntegrationsPage.tsx`: una card per integrazione con
  "Abilitata", "Predefinita", contratti dichiarati e, per env, stato e Installa/Aggiorna/Rimuovi;
  disabilitare nasconde l'integrazione ma lascia il pacchetto, con "Rimuovi dagli ambienti";
  la conferma mostra terminali aperti e task in coda coinvolti; "Prova notifica" e "Mostra utilizzo"
  stanno nella sezione per tutte le integrazioni).
- `lib/agents.ts`: logica pura zero-dep (etichette degli env, macchina a
  stati dell'agente per tab, testi delle notifiche), testata da
  `tests/agents.test.mjs`; `lib/integrations.ts` (etichette dei contratti, stato e azioni per env,
  abilitate/predefinita, impatto della disabilitazione), testata da `tests/integrations.test.mjs`. Stato per tab in `stores/tabRuntime.ts` (campo `agent`, chiave = `tab_id`, aggiornato da `app/events.ts`):
  `session-start` marca il tab come agente (idle) e salva `agentSessionId`; Invio
  -> running; `finished` -> ✓; `needs-input` -> ❗; qualsiasi input lo toglie. Per
  i tab agente sostituisce il pallino busy/idle. Gli eventi con `tab_id` senza
  tab sono ignorati (tranne `test`: solo suono). Suoni in `notifySound.ts`
  (`playDing`, `playAttention`); a finestra senza focus anche una notifica di
  sistema (`systemNotify.ts`, `tauri-plugin-notification`).
- **Sessioni automatiche** (`lib/automation.ts`, zero-dep, `tests/automation.test.mjs`):
  etichette/colori di stato, tooltip, filtro dei tab automatici. `app/events.ts` gestisce
  `automation-spawn` (apre il tab con `startCommand` = comando di avvio, `cwd` e
  `automation` in `tabRuntime` con titolo/stato; usa `openTabBackground` di `layout.ts`, che
  aggiunge il pannello al gruppo del workspace senza cambiare gruppo attivo né focus),
  `automation-close` (stesso percorso di `closeTab`) e `automation-update` (stato e
  ultimo report); alla chiusura di un tab automatico chiama `automation_tab_closed`.
  All'avvio `automation_sessions()` riallinea lo stato. `PaneHeader` mostra il badge AUTO
  (INTAKE per i tab avviati da un Bot con la skill Task Intake) e la parola di stato. I tab automatici sono esclusi dal salvataggio di sessione e
  scrollback e non vengono ripristinati.
- **Tasks Manager** (`components/tasks/`, `lib/board.ts`, `lib/boardLayout.ts`, `hooks/useBoards.ts`, `stores/boards.ts`):
  `lib/board.ts` (zero-dep, testato) ha i tipi del contratto (`BoardView`, `Task`, `Proposal`,
  `Delivery`), i quattro stati (+ "Richiede attenzione" e "In attesa di risposta", per un task con una domanda
  pendente: stato `question` di `displayStatus`, distinto dall'attenzione) con etichette e colori, selettori, ricerca/filtri,
  formati di durata e consegna, `mergeBoard` per `rev`. `useBoardsSync` (montato da `App`) carica `board_get`
  e ascolta `board-update`, `useTaskNotify` ascolta `task-notify` (notifica di sistema); i componenti chiamano `api` e
  passano la `BoardView` restituita a `storeBoard` (`app/boards.ts`, che scrive in `boardsStore`: una `rev` più vecchia
  non sostituisce quella nota). Le azioni sui task mostrano i pulsanti da `allowed_actions`, `can_edit` e `can_reorder`
  della vista: il frontend non ha una tabella delle transizioni propria. `TaskPanel` / `GlobalTaskPanel` stanno a sinistra della griglia
  dei terminali (compresso/espanso per workspace in localStorage, `lib/taskPanelPrefs.ts`); `TasksManager`
  (Ctrl+Maiusc+B o pulsante "Tasks" della sidebar) copre pannello e griglia: i `TerminalView` restano
  montati (regola del DOM sopra); la Vista Ufficio (z-30) sta sopra di lui, e aprire il manager con
  l'Ufficio aperto chiude l'Ufficio. `DependencyGraph` = React Flow (`@xyflow/react`) + auto-layout con
  `@dagrejs/dagre` (`boardLayout.ts`). "Task Intake" (`openIntake` in `app/actions/tabs.ts`) apre un nuovo tab del
  primo Bot abilitato con la skill `task-intake` (`spawnBotFromMenu`) a ogni richiesta (più chat in parallelo);
  `isIntakeLaunch(launchId, agentBots)` (`lib/automation.ts`) è vero quando il Bot del tab (`bot:<id>`) ha quella skill
  (`INTAKE_SKILL`), e guida il badge INTAKE di `PaneHeader` (prop `intake`, `tabBadge({automation, intake})`) e la
  cartella dell'Ufficio (`officeSession(tab, busy, label, agentBots, botImage?)`). Nello store di configurazione
  `sharedLaunchesFor(s, ws)` (integrati + avvii dei Bot del workspace) e `defaultLaunchFor(s, ws)` sostituiscono
  `agentLaunches`/`launchesFor`; `spawnTab` con un avvio predefinito Bot passa da `spawnBotFromMenu`, e le pagine delle
  impostazioni offrono i Bot in un gruppo “Bot” (`components/settings/DefaultLaunchOptions.tsx`). I tab ripristinati con
  un vecchio `launchId` `builtin:preset:*` sono mappati in lettura (`lib/legacyLaunch.ts::currentLaunchId`, usato da
  `lib/session.ts`); il comando di ripristino salvato non cambia. Dettagli in [BOARD.MD](BOARD.MD).
- `components/ui/dropdown-menu.tsx`: wrapper shadcn di Radix, usato dal menu
  "⋯" della card workspace.
- `lib/launch.ts`: pure functions zero-dep (sanitizza, trigger, input pty
  con , costanti di timing, `defaultLaunch`), coperte da
  `tests/launch.test.mjs`.
- `components/TerminalView.tsx`: un'istanza xterm per tab, con la sua cornice
  (posizione assoluta da `rect`, intestazione, bordo evidenziato se attivo).
  Il comando dell'avvio (catturato al mount, `initialCommand`) viene iniettato via `pty_write`
  al primo output della shell + 150 ms, con fallback a 2,5 s per gli avvii
  freddi; una sola iniezione per tab, timer puliti all'unmount.
  - Tema Campbell in `src/lib/terminalTheme.ts` (stessi 16 ANSI di Windows
    Terminal); l'host di xterm è un div interno senza padding (FitAddon
    misura il contenitore: il padding esterno sposterebbe l'ultima riga
    fuori vista). Refit di recupero dopo lo spawn e a font pronti.
  - Lo spawn avviene al mount; i tab inattivi restano montati ma nascosti
    (`display: none`) per non perdere lo stato della shell; ogni pannello visibile
    si adatta (`fit()` + `pty_resize`) quando cambia il suo rettangolo.
  - `ResizeObserver` + evento "torna attivo" tengono sincronizzate dimensioni
    xterm e PTY (`fit()` + `pty_resize`).
  - Cleanup completo all'unmount (unlisten, `dispose`, rilascio della sessione).
  - La sessione del tab passa da `ptyKeeper` (`lib/ptyKeeper.ts`, istanza in `app/ptyKeeper.ts`,
    test in `tests/pty-keeper.test.mjs`): all'unmount la chiusura (`pty_close`) e' rinviata di un giro
    dell'event loop e un rimontaggio dello stesso tab nel frattempo (double-mount di `StrictMode` in dev,
    Fast Refresh) riprende la stessa shell, con l'output arrivato nel mezzo e il comando di avvio iniettato
    una volta sola. Prima il secondo spawn trovava lo slot del tab ancora prenotato dal primo, in volo
    ("il tab ha gia' una sessione") e il primo, scartato, chiudeva la sua shell: tab senza terminale e,
    per un worker, variabili preparate dal backend gia' consumate. Uno spawn nuovo di un tab parte solo
    a chiusura precedente finita. Il backend non cambia: entro la vita del frontend gli id di sessione li
    conosce il frontend, che riaggancia senza un comando in piu'.

## Scelte e limiti noti

- Solo Windows + WSL per ora: il codice WSL/PTY è Windows-first (`wsl.exe`, ConPTY via portable-pty).
- Le board sono file JSON per workspace (`boards/`), come l'elenco dei workspace, sullo stesso `store/`; i task conclusi
  oltre i 200 più recenti passano a un archivio `.jsonl` che l'app non rilegge.
- Lo schedule è un file a parte (`schedule.json`): le fasce sono globali, ogni workspace sceglie se rispettarle
  (`use_schedule`).
- JSON invece di SQLite: adeguato per poche centinaia di workspace; la persistenza è isolata in `store/` e in chi lo usa
  (`workspaces/store.rs`, `board/persist.rs`), quindi migrabile in futuro.
- Nessuna cifratura dei dati: i path sono dati locali non sensibili. Il canale locale degli hook e dell'MCP è invece
  autenticato per sessione (segreto casuale per terminale, `automation/auth.rs`) e ascolta solo su `127.0.0.1`.
- Downgrade non supportato: un file con busta letto da una versione precedente viene messo da parte come corrotto (vedi
  "`store/`").
- Bundle JS ~1,2 MB (React, xterm.js, React Flow): accettabile per app desktop, valutabile code-splitting se cresce.

## Comandi utili

- `npm run tauri dev` — avvio app in sviluppo
- `npm run build` — typecheck + build frontend
- `cargo check` (in `src-tauri/`) — typecheck backend
- Tutti gli altri (lint, format, typecheck, test, bindings, bump) sono elencati in [CONTRIBUTING.md](../CONTRIBUTING.md).
