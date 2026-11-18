# Tasks Manager (v0.12)

Ogni workspace ha una **coda di task**. Non c'è più un orchestratore: ci sono tre
attori che non parlano tra loro.

| Attore | Cosa fa |
|---|---|
| **Task Intake** (Bot `intake`, LLM) | Ci parla l'utente. Esplora il repository, scrive una specifica completa e la **propone** (`propose_tasks`). L'utente accetta o scarta dalla UI. Non riceve eventi e non viene mai svegliato. |
| **App** | Scheduler deterministico: avvia i task "Da fare" in ordine, rispetta dipendenze, slot, fasce orarie e modalità di consegna; prepara e pulisce i worktree; raccoglie l'esito. Non decide mai niente. |
| **Worker** (LLM) | `get_task`, lavora, **consegna da solo** secondo la modalità del workspace (commit, push, PR, merge), poi `complete_task`. Se ha bisogno dell'utente chiede nel suo terminale. |

Principio: semplicità. Niente owner, niente coda di eventi, niente risveglio,
niente integrazione esterna. Un task finito è già consegnato.

Dove sta il codice: il modello, il ciclo di vita, lo scheduler e la vista sono nel modulo puro `board/`; lo
stato dei worker, l'esecuzione degli effetti (git, worktree, E2E, salvataggio) e le RPC dei tool MCP sono in
`automation/`; le impostazioni di consegna e dei repository in `workspaces/`. La mappa dei moduli e i flussi
(avvio, hook → evento, task dalla proposta alla consegna) sono in [DESIGN.MD](DESIGN.MD).

Riferimento visivo approvato (mockup): artifact "Tasks Manager mockup" (viste terminali + pannello, Kanban, Lista, Globale); i sorgenti del mockup non sono nel repository.

Lingua: tutto ciò che legge il modello (skill del plugin, descrizioni dei tool MCP,
errori RPC, istruzioni del worker, promemoria dello Stop hook, prompt di avvio) è
**in inglese**. Interfaccia e documentazione restano in italiano. Il testo dei task
(titolo e specifica) è nella lingua della conversazione con l'Intake.

## Cosa è stato rimosso (rispetto alla v0.11)

- Orchestratore: ruolo `Orchestrator`, owner della board, code di eventi, `get_events`,
  risveglio (`Wake`, `Type`), orchestratore on demand e automatico, badge ORCH,
  voce rapida "Orchestratore", skill `orchestrate`, comando `automation_orchestrator_open`,
  `start_board_orchestrator`.
- Tool ad-hoc: `spawn_session`, `list_sessions`, `send_message`, `close_session`, `whoami`,
  `list_agents`.
- Workflow: `workflow.md`, skill/wizard `workflow`, `get_workflow`, `save_workflow`, pagina
  Impostazioni → Workflow, comandi `workflow_get`/`workflow_save`.
- Richieste: `ask_user`, casella Richieste (`RequestsPanel`), comandi
  `answer_user_request`, `decide_user_request`, `dismiss_user_request`. `propose_settings`
  esisteva già nella v0.11 come richiesta; oggi è tornato con un'altra forma (vedi "Proposte di
  impostazioni").
- Board: `submit_plan`, `get_board`, `set_task_status`, `set_board`, integration branch,
  `base_branch`/`base_branches` della board, `integrations`, `owner`, `requests`, colonne
  `review`/`blocked`/`cancelled`/`backlog`, `touches`, `isolation`, `held`, `external`,
  `orchestrator_note`, `priority`, `permission_mode` per task, eventi `plan_completed`,
  `user_action`, `start_failed`.
- Worker: `report_fact`, `request_help`.
- Workspace: `auto_orchestrate` (sostituito da `use_schedule`), `worktree_setup`
  (migrato negli hook del repo, vedi sotto).

Restano: worktree e hook per repository (`RepoConfig`), campo `repo` per task (multi-repo),
permessi/regole/modello dei worker, Workspace rules, fasce orarie (solo come gate),
impedisci standby, notifiche di fine lavoro dei terminali normali.

## Modello dati

File: `<app_data>/boards/<workspace_id>.json` nella busta `{ "schema_version", "data" }` (vedi "File su disco: schema, migrazione e recupero"), scrittura atomica, salvataggio
coalescente (max 1/s per board) come oggi. Il blocco sotto è il contenuto di `data`. Serde `snake_case`; i campi sconosciuti dei file
vecchi si ignorano (niente `deny_unknown_fields`).

```jsonc
{
  "workspace_id": "ws-...",
  "max_parallel": 3,      // 1..=16: worker in parallelo (vale solo in modalità parallela)
  "paused": false,        // coda in pausa: nessun nuovo avvio
  "updated_at": 0,
  "tasks": [ /* Task, l'ordine dell'array È l'ordine della coda */ ],
  "proposals": [ /* Proposal pendenti */ ],
  "goals": [ /* Goal aperti e chiusi di recente, vedi "Goal" */ ]
}
```

Task:

```jsonc
{
  "id": "delivery-modes",   // ^[a-z0-9][a-z0-9-]{0,39}$, unico nella board
  "title": "Modalità di consegna",       // 1..=80
  "task": "...",            // specifica per il worker, 1..=100000
  "depends_on": ["task-schema"],         // id esistenti, niente cicli, max 50
  "kind": "work",           // work | review | plan (default work, vedi "Task di review" e "Task di pianificazione"); una review richiede almeno una dipendenza; plan lo crea solo l'app
  "e2e": false,             // true = deve superare gli E2E del repository; omesso se false (vedi "Test E2E")
  "no_git": false,          // true = la cartella del task non è un repository git: modalità libera (vedi "Cartelle senza git"); omesso se false; deciso a creazione/accettazione, riletto all'avvio
  "repo": null,             // cartella relativa alla root; null = la root (multi-repo)
  "agent": "claude-code",   // sempre valorizzato: risolto alla creazione (agente del task, poi del workspace, poi il predefinito globale) e non cambia con i predefiniti
  "model": null,            // null = modello dei worker del workspace
  "bot": "general",         // Bot che esegue il task (omesso nei task vecchi, che si comportano come prima); agent, model, effort e skill qui sotto sono lo snapshot preso all'accettazione
  "effort": null,           // sforzo del Bot, omesso se assente; all'avvio va in AutomationRequest.effort: un livello non supportato dall'agente è ignorato con una nota sul task
  "skill": null,            // SkillRef del Bot, omessa se assente; il worker la carica per prima (vedi get_task)
  "goal": "export-ordini",  // id del goal del task, omesso = task sciolto; lo assegna l'ingresso in coda e non cambia più (vedi "Goal")
  "round": 1,               // giro del goal in cui il task è entrato in coda; omesso senza goal
  "status": "queued",       // queued | running | done | failed
  "fail_reason": null,      // solo se failed, omesso se assente: worker_failed | worker_exited | stopped | interrupted | start_failed (scritto dalla transizione che porta il task a failed; si azzera con "Riprova"; i task falliti prima che esistesse restano senza: motivo sconosciuto, la UI non mostra nulla)
  "worker_status": null,    // starting | running | background | needs_input | idle | awaiting_answer | ... (come oggi)
  "session": null,          // tab del worker corrente
  "questions": [],          // TaskQuestion: storico delle domande del worker all'utente (ask_user), omesso se vuoto; ultime 20 (le più vecchie cadono), non si azzera con i tentativi
  "attempts": 0,
  "previous_attempt": null, // { summary?, note? } del tentativo fallito, dopo "Riprova"; lo legge get_task
  "worktree": null,         // { path, branch, base, setup_done } solo in modalità parallela
  "last_report": null,      // ultimo report breve (200)
  "note": null,             // nota dell'app: errore di avvio, "interrupted: ...", "stopped by the user", ...
  "report": null,           // TaskReport
  "e2e_run": null,          // E2eRun: ultima esecuzione E2E del tentativo (omesso se assente); azzerata a ogni nuovo tentativo
  "created_at": 0, "updated_at": 0, "started_at": null, "finished_at": null
}
```

TaskQuestion (da `ask_user`, solo con l'Agents HUB acceso; i campi opzionali sono omessi se assenti):

```jsonc
{
  "id": "q-<tentativo>-<n>", // n = posizione (da 1) fra le domande di quel tentativo
  "attempt": 1,             // `attempts` del task quando è stata fatta
  "text": "...",            // 1..=2000 caratteri (QUESTION_MAX)
  "options": ["..."],       // risposte suggerite: max 5 (OPTIONS_MAX), 1..=80 caratteri (OPTION_MAX), su una riga, uniche
  "asked_at": 0,
  "answered_at": 0,         // presente = chiusa
  "answer": "..."           // testo consegnato dall'app (max 4000, ANSWER_MAX); assente con answered_at = risposta data nel terminale
}
```

Le costanti stanno in `board/question.rs` e sono le stesse dello schema del tool MCP. Si tengono le ultime
20 domande per task (`QUESTIONS_KEPT`). Ne esiste al più una in attesa (senza `answered_at`).

TaskReport (da `complete_task`):

```jsonc
{
  "status": "completed",    // completed | failed
  "summary": "...",
  "commit": "abc1234",      // opzionale
  "pr_url": "https://...",  // opzionale (modalità PR)
  "files_changed": [], "tests": [{ "command", "result": "passed|failed|not_run", "details" }],
  "follow_ups": [],
  "diffstat": "...",        // calcolato dall'app quando c'è un worktree
  "verdict": "passed",      // solo review completate: passed | issues (omesso se assente)
  "findings": [],           // solo review: problemi (con issues) o suggerimenti (con passed); omesso se vuoto
  "e2e": null               // E2eRun: ultima esecuzione E2E al momento di complete_task (qualunque kind); omesso se assente
}
```

E2eRun (stesso tipo in `Task.e2e_run` e `TaskReport.e2e`; i campi opzionali sono omessi se assenti):

```jsonc
{
  "run_id": "e2e-<millis>-<n>",
  "status": "running",      // running | passed | failed | error
  "started_at": 0, "finished_at": 0, "duration_ms": 0,
  "junit": "out/junit.xml", // percorso del report configurato
  "code": { "commit": "...", "tree": "...", "changes": "..." }, // impronta all'avvio (vedi "Test E2E")
  "totals": { "tests": 0, "passed": 0, "failed": 0, "skipped": 0, "flaky": 0 }, // flaky solo se il report lo espone
  "failed_tests": [{ "name": "...", "file": "...", "message": "..." }], // file opzionale; max 20
  "error": "...",           // max 1000 caratteri: avvio fallito, timeout, annullata, report non valido, exit != 0 con report pulito
  "output_tail": "..."      // ultimi 4000 caratteri dell'output: solo se il comando è partito e finito ma il report manca/è illeggibile/non valido/senza test, o exit != 0 con report pulito
}
```

Proposal:

```jsonc
{
  "id": "p-<millis>-<n>",
  "source_tab": "tab-...",  // tab dell'Intake che l'ha proposta
  "created_at": 0,
  "source_task": "review-x", // opzionale: task di review o di pianificazione il cui worker l'ha proposta
                             // (omesso se assente); in quel caso source_tab è il tab di quel worker
  "goal": { "id", "title", "request", "done_when", "decisions"? }, // opzionale: goal nuovo, creato all'accettazione
  "goal_id": "export-ordini", // opzionale: goal aperto a cui vanno i task (dell'Intake o ereditato dalla review)
                              // goal e goal_id si escludono; nessuno dei due = task sciolti
  "tasks": [ /* TaskSpec: id, title, task, depends_on, repo, agent?, model, kind, e2e */ ]
}
```

Limiti: 200 task per board, 20 task per proposta, 20 proposte pendenti per board. Il limite di 200 conta solo i task in coda o in esecuzione (`queued` e `running`): i task `done` e `failed` restano come storico e non contano.

### Agente e modello di un task

- **`agent`** è l'id di un'integrazione (vedi README, “Integrazioni dei coding agent”, e DESIGN.MD, “Integrazioni:
  stato, risolutore e prontezza”). Alla creazione (proposta accettata, creazione manuale, `propose_tasks`) l'app lo
  risolve con `resolve_agent(task, workspace, globale)` per l'uso *worker*: deve essere registrato, abilitato e
  dichiarare l'automazione. Una `TaskSpec` senza `agent` prende quello del workspace (`Workspace.agent`), poi il
  predefinito globale. Una volta scritto non segue più i predefiniti. I file creati prima dell'introduzione del campo
  lo ricevono al caricamento (`Board::sanitize`: il predefinito globale).
- **`model`** è un nome del catalogo di *quell'* agente (per Claude Code un alias o un id, per OpenCode `provider/modello`, es. `opencode/gpt-5.5`): lo valida l'agente del task (`ModelCatalog::is_valid`, dentro la forma generica `valid_model_shape` che già ammette `/` e `:` nel parsing delle specifiche e nello schema MCP,
  senza catalogo vale la sola forma). Un modello che l'agente non accetta è rifiutato: `invalid model for <nome>: <modello>`. Cambiando agente con `update_task` si ricontrolla anche il modello. Chi non lo
  indica (`null`) usa il modello dei worker del workspace per quell'agente (`worker_agents[<agente>].model`), poi il
  predefinito dell'agente (`None` = l'app non passa nessun modello).
- **Bot.** Ogni task proposto da Intake, planner e reviewer nomina un Bot (`bot`, obbligatorio in `propose_tasks`).
  All'accettazione (`Board::check_specs`) il Bot si risolve (`bots::resolve`) e agente, modello, effort e skill si
  copiano nel task: uno snapshot, quindi cambiare o cancellare il Bot non tocca i task in coda. `model` della
  specifica, se c'è, scavalca quello del Bot; `agent`, se c'è, deve coincidere con quello del Bot (errore
  altrimenti). Un Bot sconosciuto o disabilitato, o la cui skill l'agente non può caricare, è rifiutato. Le
  creazioni dall'UI (`task_create`, `task_update`) non richiedono il Bot; il `plan` dell'Agents HUB sì (lo sceglie il
  segretario, vedi "Task di pianificazione"), quello di `create_plan_task` no. I task senza `bot`
  (anche quelli già nei file) si comportano esattamente come prima, senza migrazione.
- **Tipi di task dei Bot.** `AgentBot.kinds: TaskKind[]` (`bots.json` v3, ordine `work`, `review`, `plan`, senza
  doppioni) sono i tipi di task automatici che il Bot può eseguire; `[]` = Bot solo interattivo (tab e avvii), mai per
  un task. Il comportamento del ruolo viene sempre e solo da `kind`: `kinds` decide soltanto quali Bot si possono
  scegliere. Predefiniti: `general` `[work]`, `reviewer` "Reviewer" `[review]`, `planner` "Planner" `[plan]` (fascia
  `strong`, effort `high`, nessuna skill), `quick-answer` "Quick Answer" `[plan]` (fascia `standard`, effort `low`,
  skill `code-answer`), `bug-fixer` "Bug Fixer" `[work]` (fascia `strong`, effort `high`, skill `bug-fix`),
  `researcher` "Researcher" `[plan]`, `test-writer` "Test Writer", `docs-writer` "Docs Writer" e `refactorer`
  "Refactorer" `[work]`, `git-operator` "Git Operator" e `release` "Release" `[work]` (spenti di serie), `intake`, `setup`, `skill-wizard` `[]` (tabella completa in "Skill del plugin e Bot"); il ripristino di un predefinito rimette
  anche i suoi `kinds`, e l'utente può allargare quelli di General. La migrazione 2→3 dà `[work, review, plan]` ai Bot
  derivati e a quelli dell'utente (`custom-*`), aggiunge Reviewer e Planner una volta sola (un Bot cancellato non
  ricompare, un nome già preso diventa "Reviewer 2"...); le migrazioni 3→4 e 4→5 aggiungono Quick Answer e Bug Fixer e
  rendono esclusivi i `task` di General e Planner, la 5→6 aggiunge Researcher, Test Writer, Docs Writer e Refactorer, la 6→7 Git Operator e Release, spenti. Il controllo, senza ripieghi, vale in
  `Board::check_specs` (proposte, accettazione, task dall'UI con Bot, `plan` dell'HUB), in `update_task` quando il
  patch cambia il Bot o il `kind` (con solo `kind` lo snapshot resta, si verifica il Bot del task) e quindi anche nella
  verifica di `request_task_update` prima della card: "task '<id>': bot <nome> cannot run <kind> tasks: choose a bot
  whose kinds include <kind> (see get_workspace_settings.bots)". I task già in coda o finiti non si rivalidano.
- **Avvio con effort e skill.** `AutomationRequest.effort` riceve l'effort del task; se l'agente non lo supporta
  `automation_command` lo scarta e l'avviso finisce nella nota del task. Con una skill l'invocazione viene
  dal contratto dell'agente (`PluginSkills`/`UserSkills::skill_name`); se l'agente non può fornirla l'avvio fallisce
  con una nota. Una skill utente che non esiste più blocca l'avvio (`the skill <nome> of bot <bot> no longer
  exists: choose it again in the bots settings or change the task's bot`): nessun ripiego silenzioso.
- **Motivo di attesa.** Un task `queued` il cui agente non è pronto nel target del workspace non è mai candidato
  (anche con tutte le dipendenze `done`): non fallisce, non occupa uno slot e parte da solo appena l'agente è `ready`.
  Il motivo è `agent_unavailable: { agent, reason }` nella vista (`TaskView`, e nelle righe di `list_tasks`), con `reason` =
  `not-found` (agente non rilevato nel target), `not-installed` (pacchetto mancante), `outdated` (pacchetto da
  aggiornare) o `disabled` (integrazione disabilitata). Il campo manca se l'agente è pronto, se la prontezza si sta
  ancora controllando o se il task non è `queued`. Diverso da `waiting_on`, che elenca le dipendenze non ancora `done`.
- Un task con l'agente disabilitato resta modificabile (titolo, specifica, dipendenze); l'agente si ricontrolla solo se
  il patch contiene `agent` o `model`.

### Stati e UI

| status | UI | note |
|---|---|---|
| `queued` | **Da fare** | parte quando: coda non in pausa, fascia aperta (se il workspace la usa), tutte le dipendenze `done`, slot libero. Con dipendenze non `done` la vista espone `waiting_on`; se una di esse (anche indirettamente) è `failed` espone anche `blocked_by`. |
| `running` | **In corso** | con `worker_status` `needs_input` o `idle` la vista espone `attention: true` → badge "Richiede attenzione". Con `worker_status` `awaiting_answer` il worker aspetta la risposta a una sua domanda: è "in attesa dell'utente" ma **non** `attention` (nessun badge, il Task panel e il Tasks Manager lo mostrano "In attesa di risposta"). |
| `done` | **Fatto** | consegnato. Terminale. Sblocca i dipendenti. |
| `failed` | **Fallito** | `fail_reason` dice perché in modo strutturato (vedi sotto), `note`/`report.summary` il dettaglio. "Riprova" lo rimette in coda e azzera `fail_reason`. |

Il `fail_reason` lo scrive la transizione che porta il task a `failed` (`Outcome::Failed` di `TaskEvent::Finish`):

| `fail_reason` | Transizione | UI |
|---|---|---|
| `worker_failed` | `complete_task` con `status: "failed"` (o `blocked`) | "Il worker ha rinunciato" |
| `worker_exited` | tab chiuso o agente uscito senza report (anche: il worker non ha mai chiamato `get_task`) | "Worker uscito senza esito" |
| `stopped` | l'utente preme Ferma | "Fermato dall'utente" |
| `interrupted` | riavvio dell'app con il task in corso | "App riavviata" |
| `start_failed` | git, setup, precondizione, skill non più esistente, avvio che non finisce in tempo | "Avvio fallito" |

Serve a chi legge la coda (utente e agenti) per capire se ha senso riprovare. È indipendente da `FailCause`
(che governa solo la riapertura di un task la cui sessione è finita, `Reopen`). Sul filo è `snake_case`,
nel file della board è un campo piatto omesso se assente; un `Reopen` lo toglie, perché il task torna `running`.
Nessuna migrazione lo deduce dalle note dei task già falliti.

Gli stati stanno in un'enum (`TaskState`, `board/state.rs`) che porta i dati validi solo in quello
stato, e si cambiano soltanto con una transizione (`TaskEvent` applicato da `Task::apply`,
`board/lifecycle.rs`): nessun altro codice scrive stato, tentativi, nota e ultimo report. `running` ha due
fasi: **in preparazione** (l'app crea worktree ed esegue il setup, il task ha `worker_status: starting`
e nessuna `session`; un battito di vita lo tiene d'occhio il guardiano degli avvii) e **col worker vivo**
(`session` = il tab, `worker_status` dagli hook). Un evento che riguarda un tentativo o una sessione
che non sono più quelli del task è rifiutato (`Stale`) e non cambia niente.

`BoardView` = board + per task: `waiting_on: string[]` (dipendenze non `done`, solo per
`queued`), `blocked_by: string[]` (solo per `queued`: i task `failed` da cui dipende, direttamente o no; mai su disco; vuoto se non è bloccato), `attention: bool`, `question?: TaskQuestion` (la domanda in attesa di risposta, se c'è; la UI la mostra nel dettaglio del task, "Domanda del worker"), `allowed_actions`, `can_edit`, `can_reorder` e `cascade` (vedi "Azioni consentite"); + `rev` monotona (come oggi), `schedule_blocked: bool` (fascia
chiusa per un workspace che usa le fasce), `delivery` del workspace (comodità per la UI) e `limits: { max_parallel: { min, max } }`
(gli estremi di `max_parallel`, gli stessi che validano `board_set` e `set_board`: la UI li legge da qui).

### Badge dei task

Le card e le righe mostrano pochi badge; ognuno deriva da un campo del task e, dove indicato, cambia cosa fa il
worker o cosa si può scegliere. Compaiono in `KanbanCard`, `ListRow`, `TaskDetail`, `TaskNode`, `PanelQueue`,
`PanelRunningCard` e `ProposalCard` (`PlanBadge` anche nel pannello del thread dell'HUB).

| Badge | Origine del dato | Effetto |
|---|---|---|
| **Review** | `kind: review` | Ruolo Reviewer: `get_task` con `reviewed_tasks`, nessuna consegna, verdetto e `findings` obbligatori; `propose_tasks`, `get_task_details` e `get_workspace_settings` ma niente `ask_user`; almeno una dipendenza (i task rivisti); notifica `ReviewDone`; la proposta torna nel thread d'origine. Serve un Bot con `review` fra i `kinds` (`Reviewer`) |
| **Pianificazione** | `kind: plan`, solo task creati dall'app (HUB e `create_plan_task`) | Ruolo Planner: worktree isolato sempre, non occupa il repository nello scheduler, niente setup, consegna, commit né E2E; `list_tasks`, `get_workspace_settings` e `ask_user`; notifica `PlanWithoutProposal` se chiude senza proposta; escluso dalla chiusura automatica del thread. Il `kind` è riservato all'app. Serve un Bot con `plan` (`Planner`) |
| **Verificato** / **Problemi trovati** | verdetto del report di una review `done` | Solo informativo: i `findings` si leggono nel dettaglio |
| **E2E richiesto** / esito | `e2e` del task e ultima esecuzione (`e2e_run` o `report.e2e`) | "E2E richiesto" finché il task marcato non ha mai eseguito gli E2E, poi l'esito (vedi "Test E2E") |
| **Bot** (avatar e nome) | `bot` del task (snapshot all'accettazione); assente nei task vecchi | Dice chi esegue il task; il selettore del Bot offre solo i Bot abilitati con quel `kind` |
| **Agente** | `agent` del task (snapshot dal Bot, o dal workspace nei task senza Bot) | Agente che lancia il worker; con un Bot non si sceglie a parte |
| **Richiede attenzione** | `attention` della vista (task `running`) | Il worker aspetta l'utente: è uno stato, non un tipo |
| **attende X** / **attende N task** | `waiting_on` della vista (dipendenze non `done`) | Il task `queued` parte quando sono tutte `done` |
| **bloccato da X** / **bloccato da N task** | `blocked_by` della vista (task `failed` da cui dipende, anche indirettamente) | Il task `queued` non partirà finché non si riprova o si segna come fatto il fallito; sostituisce "attende …" (pannello Task, Kanban, Lista, nodo del grafo; il dettaglio ha la sezione "Bloccato") |
| **Motivo** del fallimento | `fail_reason` del task `failed` | "Fermato dall'utente", "App riavviata", "Avvio fallito", "Il worker ha rinunciato", "Worker uscito senza esito"; assente nei task falliti prima del campo |
| Repository | `repo` del task | Repository in cui lavora (assente = root) |

Tipo e Bot: `work` → `General` (o un Bot con `work`), `review` → `Reviewer`, `plan` → `Planner`. Nei form,
cambiando il tipo il Bot scelto resta se lo esegue, altrimenti passa al default del tipo con un avviso in linea.

### Azioni consentite (`allowed_actions`, `can_edit`, `can_reorder`, `cascade`)

Cosa si può fare a un task dipende solo dal suo stato; la tabella è una sola (`permits` in
`board/lifecycle/actions.rs`) e la leggono le guardie delle operazioni, i messaggi di errore e la
vista. `TaskAction` è l'azione di `task_action`, `snake_case` sul filo: `retry`, `mark_done`,
`stop`, `delete`, `delete_cascade`, `to_top`.

| status | azioni | modifica (`can_edit`) | riordino (`can_reorder`) |
|---|---|---|---|
| `queued` | `to_top`, `delete`, `delete_cascade` | sì | sì |
| `running` | `stop` | no | no |
| `done` | nessuna | no | no |
| `failed` | `retry`, `mark_done`, `delete`, `delete_cascade` | sì | no |

Ogni task della vista porta, dopo i campi del task, `waiting_on` e `attention` più:

- `allowed_actions: TaskAction[]`: la tabella, ristretta dal contesto della board a ciò che
  riuscirebbe adesso (`delete` solo se nessun task non `done` ne dipende; `delete_cascade` solo se ci
  sono dipendenti e nessun task da togliere è `running`). La UI può mostrare i pulsanti da questa
  lista invece di riderivarla dallo stato: il frontend non ha una propria tabella delle transizioni;
- `can_edit: bool`: il task si può modificare (`task_update`);
- `can_reorder: bool`: il task si può riordinare (`task_move`); la UI lo usa per il trascinamento;
- `cascade: { dependents: string[], running: string[] }`: cosa toglierebbe "Elimina tutti".
  `dependents` sono i task non `done` che dipendono dal task, direttamente o no (il task escluso);
  `running` i `running` fra il task e i suoi dipendenti. Vuoto per gli stati che non si possono
  eliminare. Lo stesso calcolo (`board/cascade.rs`, un indice delle dipendenze inverse costruito
  una volta per vista) serve a `delete_cascade`, quindi vista e azione non possono divergere;
- `blocked_by: string[]`: per un task `queued`, i task `failed` da cui dipende, direttamente o no (in ordine di
  coda, mai su disco). Lo calcola lo stesso indice delle dipendenze inverse: per ogni `failed`, i suoi
  dipendenti `queued`. Un task `done` nel mezzo spezza la catena (resta com'è). Diverso da `waiting_on`, che
  resta l'elenco delle dipendenze non `done`; un task con la dipendenza solo `queued` o `running` non è bloccato.

### File su disco: schema, migrazione e recupero

Il file `<app_data>/boards/<workspace_id>.json` usa la busta comune a tutti i file dell'app
(modulo `store`):

```jsonc
{ "schema_version": 3, "data": { /* la board, come in "Modello dati" */ } }
```

La versione corrente è il numero dei passi di migrazione (`BOARD_MIGRATIONS` in
`board/persist.rs`); oggi è **3**. Un file senza busta è la versione 0: il formato scritto
dall'app fino alla v0.12 e quello della v0.11 (stessi campi, più stati e campi eliminati).
Il passo 1 → 2 (`board/legacy_settings.rs`) sposta i campi dei worker delle proposte di impostazioni
in `worker_agents`; il passo 2 → 3 (`v2_to_v3`, `board/legacy_goals.rs`) aggiunge `goals: []`
senza ricostruire nessun goal dai task esistenti, che restano sciolti.

**Migrazione (`v0_to_v1`, `board/legacy.rs`).** Funzione pura sul JSON del file, senza orologio e
senza stato dell'app; sul formato della v0.12 non cambia nulla. Per i file della v0.11:

- `queued` (anche `held`) → `queued`; `done` (anche `external`) → `done`; `running` resta `running`;
- `review` → `failed` con nota `completed but not integrated (board v0.11): retry to let the worker deliver it`;
- `blocked`, `failed` → `failed`;
- `cancelled` → rimosso (e tolto dai `depends_on` degli altri);
- `report.status` `blocked` → `failed`;
- `session` e `worker_status` dei task non `running` → assenti (i tab dei worker non si ripristinano);
- `kind` assente (task e `TaskSpec` delle proposte) → `work`; `source_task`, `verdict` e `findings` assenti → nessuno;
- i campi rimossi si ignorano; `requests`, `owner`, `integrations` & co. spariscono al prossimo salvataggio.

Un task `failed` per migrazione senza `finished_at` lo prende da `updated_at`. Dopo la migrazione
si applicano le invarianti sui valori (`Board::sanitize`): `max_parallel` fra 1 e 16, un `agent`
per ogni task (il predefinito se manca), `round` solo sui task di un goal (almeno 1), goal con
`round` almeno 1 e con `outcome`/`review` solo se chiusi (un goal chiuso senza esito vale
`abandoned`, senza review `none`); `workspace_id` vuoto rende il file inutilizzabile.

**Recupero dopo il riavvio (`Board::recover_after_restart`, `board/recovery.rs`).** Distinto dalle
migrazioni e applicato a ogni caricamento, anche di un file già alla versione corrente: nessun
worker sopravvive all'app, quindi ogni task `running` (in preparazione o con il worker) diventa
`failed` con nota `interrupted: the app was restarted` e `finished_at` = adesso, e ogni
esecuzione E2E ancora `running` (quella del task e la copia nel report) diventa `error`
con lo stesso testo. Ritorna quanti task ha recuperato; rieseguirlo non cambia nulla.

**Errori di lettura.** Li gestisce lo `store`, per workspace: un file corrotto, non migrabile o
scritto da una versione dell'app più recente non viene mai cancellato ne' sovrascritto, ma
rinominato `<workspace_id>.json.bak-<AAAAMMGG-OOMMSSmmm>` (si tengono gli ultimi 5); la board
riparte vuota e il frontend riceve un `store-warning` (comando `store_warnings`). Un file che non
si riesce nemmeno a leggere resta dov'è e i salvataggi di quella board sono rifiutati (con un
errore nel log) finché non si riavvia l'app. Il vecchio `<id>.json.bak` non esiste più.

### Archivio dei task conclusi

La board tiene al massimo `MAX_FINISHED_KEPT` = 200 task conclusi (`done` e `failed`): a ogni
pubblicazione (`flush_one`) i più vecchi per `finished_at` oltre i 200 più recenti escono dalla
board (`Board::take_archivable`, `board/archive.rs`). Restano, anche oltre il tetto, i task con
un `worktree` (ancora da ripulire o da riusare con "Riprova"), quelli da cui dipende un task non `done` o una
proposta pendente e quelli di un goal **aperto**; i riferimenti ai task usciti spariscono dai `depends_on` di quelli rimasti.

I task usciti finiscono in `<app_data>/boards/archive/<workspace_id>.jsonl`, una riga JSON per
task in coda al file (la cartella e il file si creano alla prima volta):

```jsonc
{"schema_version":1,"data":{ /* il task, come in "Modello dati" */ }}
```

L'archivio viene scritto prima del salvataggio della board senza quei task (`Effect::Archive`
prima di `Effect::Publish`, applicato da `Host::archive_tasks`): se l'app cade in mezzo, i task
restano nel file della board e, per una riga, anche nell'archivio. Se la scrittura dell'archivio
fallisce, l'errore va nel log con gli id dei task e i task **non si perdono**: tornano in fondo alla
board in memoria e nel file che il `Publish` successivo salva (la vista inviata al frontend resta
quella senza di loro), e la prossima modifica vera li archivia di nuovo
(`Registry::restore_unarchived`). L'archivio non si legge più dall'app; si cancella insieme alla
board (`delete_board`, rimozione del workspace).

I **goal chiusi** seguono lo stesso meccanismo: la board ne tiene `MAX_CLOSED_GOALS_KEPT` = 100 (i chiusi
più di recente, per `closed_at`; gli aperti restano sempre), i più vecchi escono
(`Board::take_archivable_goals`, `board/goal/archive.rs`) e vanno in
`<app_data>/boards/archive/<workspace_id>.goals.jsonl`, una riga `{"schema_version":1,"data":{ /* il goal */ }}`
per goal (`Host::archive_goals`). Stesso `Effect::Archive` (con `tasks` e `goals`) prima del `Publish`, stessa
garanzia: un goal rifiutato dall'archivio torna nella board (`Board::restore_archived`) e si riprova alla
modifica successiva. Il file si cancella con la board.

## Consegna (impostazione del workspace)

Nuovo campo del Workspace `delivery` (`#[serde(default)]`), comando Tauri
`set_delivery(id, delivery) -> Workspace`:

```jsonc
{
  "mode": "commit",        // "pr" | "local_merge" | "push" | "commit"   (default "commit")
  "parallel": false,       // solo per "pr" e "local_merge"
  "merge_method": "squash" // solo per "pr": "squash" | "merge" | "rebase"
}
```

| Modalità | parallel = true | parallel = false |
|---|---|---|
| `pr` — branch + push + PR + merge (`gh`) | worktree per task, fino a `max_parallel` | branch nel checkout del repo, 1 worker per repo |
| `local_merge` — branch + rebase + fast-forward della base locale | worktree per task, fino a `max_parallel` | branch nel checkout del repo, 1 worker per repo |
| `push` — commit sul branch corrente + `pull --rebase` + push | — | checkout del repo, 1 worker per repo |
| `commit` — solo commit sul branch corrente | — | checkout del repo, 1 worker per repo |

"Checkout del repo" = `<root>` o `<root>/<repo>`. La modalità vale per tutti i repo del
workspace. Cambiarla non tocca i task già `running`.

### Branch base

Per le modalità con branch: `repos[repo].base_branch` → (solo `pr`) default di `origin`
(`git symbolic-ref --short refs/remotes/origin/HEAD`, senza `origin/`) → branch corrente del
checkout del repo, risolto **all'avvio** del task. Per `push`/`commit`: il branch corrente.

### Worktree (solo modalità parallele)

Come oggi: `<repoDir>/.worktrees/<task-id>`, branch `task/<task-id>`, `.worktrees/` in
`info/exclude`, hook del repo (`copy_ignored`, `setup`) dopo la creazione, `setup_done`,
riuso al retry, guardiano dei 5 minuti, timeout git 120 s, WSL via `wsl.exe -e`.
- Gli **hook** `setup` e `cleanup` girano con il runner dei processi dell'app (`automation/process.rs`,
  lo stesso degli E2E): dell'output si tengono solo gli ultimi 64 KiB per stream e, allo scadere
  del timeout, si uccide l'intero albero di processi (vedi "Test E2E", Annullamento).
- Le **review** (`kind: "review"`) ricevono il worktree dalla base come ogni task (`task/<id>`);
  la pulizia a `done` funziona anche senza commit sul branch (worktree rimosso, branch cancellato).
- I **task di pianificazione** (`kind: "plan"`) hanno un worktree `task/<id>` dalla base **in ogni
  modalità**, anche in quelle senza worktree (vedi "Task di pianificazione"): niente hook `setup`
  (leggono solo il codice), niente precondizione `local_merge` sul checkout, stessa pulizia a `done`.
  Eccezione: i piani e i task in una cartella senza git (modalità libera, vedi "Cartelle senza git"):
  nessun worktree, nessun branch, niente da ripulire.
- `pr`: prima della creazione `git fetch origin <base>` (cache 60 s, come oggi) e worktree da
  `origin/<base>` se esiste, altrimenti dal `<base>` locale.
- `local_merge`: worktree dal `<base>` locale. **Precondizione** all'avvio: il checkout del
  repo deve essere sul branch `<base>` (serve al fast-forward); altrimenti il task fallisce
  all'avvio con nota `local_merge needs <repoDir> checked out on <base> (now on <x>)`.
- Pulizia quando il task diventa `done`: comandi `cleanup` del repo, `git worktree remove`
  (`--force` se `remove_force`), poi `git branch -D task/<id>` per `pr` (il merge è squash/
  rebase lato server: la consegna è confermata dal worker) e `git branch -d task/<id>` per
  `local_merge`. Gli errori diventano avvisi nella `note`, mai bloccanti. Un task `failed`
  conserva worktree e branch per il retry.
- **Riallineamento delle worktree**: la pulizia alla chiusura del tab dipende da un flag in
  memoria, quindi un crash o la chiusura dell'app nel suo ritardo la perdono. All'avvio (primo
  giro del timer, dopo il ripristino delle board) e poi ogni 5 minuti l'app confronta le board con
  `git worktree list --porcelain` di ogni repository dei workspace non archiviati con consegna a
  worktree, e ripulisce:
  - i task `done` che hanno ancora un worktree e il cui tab non è aperto (come alla chiusura del tab);
  - le worktree `task/<id>` in `<repo>/.worktrees/<id>` che nessun task della board conosce (task
    eliminato, pulizia persa): sempre un avviso nel log; si rimuovono solo se non hanno modifiche
    non committate (mai `--force` d'ufficio, nemmeno con `remove_force`) e il loro branch si
    cancella sempre con `-d`, anche in `pr`: nessuno ha confermato la consegna. Se non si riesce a
    leggerne lo stato restano dove sono.
  - Alla rimozione di un workspace i worktree di tutti i suoi task vengono ripuliti (con la copia
    del workspace presa prima della rimozione: lo store non lo ha più).
- Tutti i comandi git (e gli script `sh` in WSL) passano dal runner dei processi: output limitato
  e, al timeout, uccisione dell'intero albero (in WSL anche i processi Linux, marcati con
  `CLI_COMPANION_RUN`).

### Modalità in place

Nessun worktree: il worker lavora nel checkout del repo. Prima dell'avvio l'app non modifica
nulla (niente fetch, niente worktree); nelle modalità con branch legge soltanto la base
(`base_branch` del repo, default di `origin` per `pr`, branch corrente) per scriverla nelle
istruzioni. In `pr`/`local_merge` seriali il worker crea il branch
`task/<id>` da `<base>`, consegna, torna su `<base>`, aggiorna e cancella il branch locale.

### Istruzioni di consegna (testo in inglese, generato dall'app)

`get_task` restituisce `instructions` (comuni) + `environment` (cartella, branch, base,
modalità) + `delivery` (i passi della modalità). Contenuto minimo per modalità:

- **Comuni** (con `dependencies` o `goal` in `get_task`, anche la riga che li cita): work only inside `<dir>`; never touch other folders; commit only files you
  changed, never stash/reset/discard changes you did not make; do not push or merge unless
  the delivery steps say so; run the verification the task asks for before delivering; when
  delivery succeeded call `complete_task` with `status: "completed"` (with `commit`, and
  `pr_url` in PR mode); if you cannot complete or deliver (conflict you cannot resolve,
  tests failing, missing tool or auth) call `complete_task` with `status: "failed"` and the
  reason; if you need a decision from the user, ask the question in plain text and end your
  turn: the user answers in your terminal. Workspace rules accodate come oggi.
- **pr, parallel** (worktree su `task/<id>` da `<base>`): commit; `git fetch origin`;
  `git rebase origin/<base>` (risolvi i conflitti, ripeti la verifica); `git push -u origin
  task/<id>` (`--force-with-lease` dopo un rebase); `gh pr create --base <base> --head
  task/<id> --title <title> --body <summary>` (se esiste già, riusala); `gh pr merge <url>
  --<merge_method>` (se il repo richiede check o review usa `--auto` e attendi lo stato
  `MERGED` con `gh pr view <url> --json state`; se il merge non è possibile → `failed`
  con il link della PR); se la base è avanzata ripeti rebase + push; poi `git push origin
  --delete task/<id>` (ignora l'errore se già cancellato). Non fare checkout di altri branch:
  il worktree resta su `task/<id>`.
- **pr, in place**: verifica che il checkout sia pulito per i file che toccherai; `git fetch
  origin`; `git switch -c task/<id> origin/<base>` (o `<base>` senza remote); lavoro; stessi
  passi PR; alla fine `git switch <base> && git pull --ff-only` e `git branch -D task/<id>`.
- **local_merge, parallel**: commit; `git rebase <base>`; ripeti la verifica; `git -C
  <repoDir> merge --ff-only task/<id>`; se fallisce perché `<base>` è avanzata, rebase e
  riprova; se il checkout principale ha modifiche che lo impediscono → `failed`.
- **local_merge, in place**: `git switch -c task/<id> <base>`; lavoro; commit; `git switch
  <base>`; `git merge --ff-only task/<id>` (se non è fast-forward: `git rebase <base>
  task/<id>` e riprova); `git branch -d task/<id>`.
- **push**: commit sul branch corrente; `git pull --rebase`; ripeti la verifica se ha portato
  modifiche; `git push`.
- **commit**: commit sul branch corrente. Niente push.

**Istruzioni del reviewer.** Per i task `review` `get_task` non restituisce né le istruzioni del
worker né `delivery` (nessun passo di consegna: il reviewer non committa, non pusha, non
fa merge): restituisce le istruzioni di `src-tauri/src/board/prompts/reviewer_instructions.md`
(incluso con `include_str!`, in inglese), seguite dallo standard di ingegneria con cui giudica i
diff (la frase `REVIEWER_STANDARD_INTRO` e le regole numerate, senza il preambolo del worker; vedi
"Standard di ingegneria"), più `environment` normale e le regole del workspace accodate come al
solito. Vedi "Task di review".

**Istruzioni del pianificatore.** Per i task `plan` `get_task` non restituisce `delivery` né lo
standard di ingegneria: restituisce le istruzioni di `src-tauri/src/board/prompts/planner_instructions.md`
(inglese, derivate dalla skill Task Intake), un `environment` proprio (`planner_environment`: senza
modalità di consegna, con il worktree di sola lettura) e le regole del workspace. Vedi "Task di pianificazione"
(e "Pianificazione in una cartella senza git" per la sezione in più di `planner_root.md`).

**Task marcati E2E** (`e2e: true`, `work`). Il testo cambia solo per loro; per gli altri è
identico a prima. Il passo `E2E_STEP` (`board/texts.rs`) dice di lanciare `run_e2e`, poi
`get_e2e_result` con `wait_seconds: 25` finché lo stato non è più `running`, che conta solo
l'esecuzione dell'app (lanciare il comando a mano no) e di proseguire solo con `passed`
(altrimenti correggere, committare, ripetere verifica e passo, oppure `complete_task failed`).
Posizione del passo:

- `pr` (parallel e in place): dopo il rebase su `origin/<base>` e prima di `git push`.
- `local_merge` parallel: dopo `git rebase <base>`, prima del `merge --ff-only`.
- `local_merge` in place: dopo il commit del lavoro, prima di `git switch <base>`.
- `push`: dopo `git pull --rebase` (e la ripetizione della verifica), prima di `git push`.
- `commit`: dopo il commit, ultimo passo.

Prima della chiamata finale a `complete_task` c'è la riga `E2E_GATE`: `completed` è accettato
solo se l'ultima esecuzione è passata sul codice attuale (stesso albero e stesse modifiche non
committate); dopo un rebase, merge o commit successivo va rifatto il passo; `failed` è sempre
accettato. In `environment` una riga `- E2E tests: required for this task...` riporta comando,
report JUnit e timeout (solo se il repository ha ancora la configurazione).

Al retry di un task fallito `get_task` aggiunge `previous_attempt` (summary del report e
nota), così il worker riparte da dove si era fermato (stesso branch/worktree).

**Standard di ingegneria.** Ogni worker riceve, dopo le istruzioni comuni e prima delle regole
del workspace, lo standard `WORKER_GUARDRAILS` (`board/texts/standard.rs`, in inglese): il preambolo rivolto
al worker di `src-tauri/src/board/prompts/worker_standard_preamble.md`, una riga vuota e le regole
numerate di `src-tauri/src/board/prompts/engineering_standard.md` (entrambi inclusi con
`include_str!`). Le regole sono **un'unica fonte** con due varianti: nel file il testo che vale solo
per i task in coda sta fra `<queue>` e `</queue>`, quello che vale solo in un tab interattivo fra
`<interactive>` e `</interactive>`, il resto vale per entrambe; i marcatori non arrivano mai al
modello, una regola che resta vuota sparisce e le regole si rinumerano. La variante dei task in coda
(`ENGINEERING_STANDARD`, 13 regole) la ricevono il worker e il reviewer, introdotta per lui da una
frase propria (i worker dei task rivisti dovevano seguire lo standard, i diff si giudicano su di esso;
le regole che parlano di consegna, `follow_ups`, report o `failed` descrivono doveri del worker da
controllare, non da eseguire). La **variante interattiva** (`INTERACTIVE_STANDARD`, stesso preambolo,
12 regole) è per i Bot con `work` avviati in un tab: niente regola degli scenari E2E della specifica,
i problemi estranei si segnalano all'utente invece che in `follow_ups`, l'autorevisione precede la
fine del lavoro invece della consegna e, se la verifica non passa, si dice all'utente cosa fallisce
invece di chiudere il task come `failed`; non cita mai `complete_task`, `follow_ups` né la consegna.
L'app la scrive (`bots/engineering_standard.rs`, scrittura atomica, solo se è cambiata) in
`<cartella dati>/bot-instructions/engineering-standard.md` a ogni avvio e ripristino di un tab Bot, e
la consegna come `InteractiveRequest::instructions_file` nel formato di path del target (Windows o
`/mnt/...` per WSL, rifiutato se non è sicuro fra doppi apici nelle shell): Claude Code la aggiunge
al prompt di sistema con `--append-system-prompt-file "<file>"` (anche in `restore_command`), OpenCode
con la chiave `instructions` di `OPENCODE_CONFIG_CONTENT` (anche in `restore_env`). Un agente che non
dichiara `applies_instructions` avvia comunque il Bot, senza lo standard e con l'avviso `<agente> does
not support system instructions: the engineering standard is not applied`; un file che non si è potuto
scrivere dà l'avviso `the engineering standard could not be prepared (...): not applied` (al
ripristino un avviso in italiano). Un tab salvato prima di questa versione non ha il flag nel comando
di ripristino di Claude Code: riparte senza lo standard finché non si riapre il Bot. Le regole: rispetto del progetto e dei pattern del framework, SOLID e codice
pulito, niente file "god", sicurezza come bloccante (non un follow-up), prestazioni per
progettazione, errori e casi limite, **concorrenza** (solo se la modifica tocca stato condiviso,
file, lock, thread o task asincroni: niente race né deadlock, niente I/O sotto lock, scritture
atomiche con file temporaneo e rename, operazioni ritentate idempotenti), **test utili** come
parte della modifica (falliscono se il comportamento si rompe, asseriscono esiti osservabili,
deterministici e indipendenti), **scenari E2E** (se la specifica ha una sezione E2E scenarios ogni scenario diventa un test della suite E2E del progetto, con locator rivolti all'utente, esiti osservabili, niente attese fisse e test indipendenti; la suite si esegue prima della consegna), **UI** (solo se la modifica
tocca un'interfaccia: tastiera e nome accessibile per ogni elemento interattivo, focus visibile e
gestito nei dialog, testi tradotti col meccanismo del progetto, stati di caricamento, errore e
vuoto), **documentazione allineata** nella stessa modifica (i commenti spiegano il perché),
rigore sul perimetro e **autorevisione severa** del proprio diff prima di consegnare, con un
**report onesto**: cosa è stato verificato e come, cosa no e perché, mai un controllo dichiarato
passato se non è stato eseguito sul codice finale. Le regole del workspace aggiungono
specifiche del progetto e non allentano mai le regole di sicurezza.

## Scheduler

Funzione pura dopo ogni modifica (come oggi):

1. Niente se la board è `paused`, se il workspace è archiviato o se usa le fasce (`use_schedule`) e la
   fascia è chiusa.
2. Candidati: `queued` con tutte le dipendenze `done`, nell'ordine dell'array.
   Un task il cui agente non è pronto nel target del workspace (non trovato, pacchetto mancante o
   vecchio, integrazione disabilitata) non è candidato e non occupa uno slot: non fallisce, resta
   `queued` e parte da solo appena l'agente è `Ready`. La vista espone `agent_unavailable: { agent,
   reason }` (reason = `not-found`, `not-installed`, `outdated`, `disabled`; assente se l'agente è
   pronto o la prontezza si sta ancora controllando). La prontezza è l'ultima nota (rinnovata ogni
   15 s in background, mai sotto il lock del registro) e si rivaluta a ogni giro del timer e a ogni
   cambio; `get_workspace_settings` la riporta per agente in `agents[].readiness`.
3. Slot: due task della stessa cartella senza git (`no_git`, vedi "Cartelle senza git") non girano
   mai insieme, nemmeno in modalità parallela. In modalità parallela (`pr`/`local_merge` con `parallel`) fino a `max_parallel`
   running nella board; altrimenti **1 running per repository** (chiave `repo`), comunque
   entro `max_parallel`. Un task `plan`, che gira sempre nel suo worktree, non occupa il repository.
4. Ogni scelto passa a `running` (`worker_status = starting`, `attempts += 1`,
   `started_at`), l'avvio vero (git + spawn) gira fuori dal lock su un thread separato.

**Avvio del worker e agente.** Il comando del worker lo costruisce `WorkerAutomation::command` dell'agente
del task e ritorna un `AutomationLaunch { command, env }`: il comando va digitato nel tab, le variabili `env`
le imposta il backend sul processo del tab prima che la shell parta (per Claude Code non ce ne sono; OpenCode
vi passa modello, agente di partenza e regole di permesso, con modelli `provider/modello`). Il modello è
quello del task, poi del workspace per l'agente, poi il predefinito dell'agente, ed è sempre validato
dall'agente del task.

Nessun evento `plan_completed`: la UI mostra lo stato.

## Ciclo di vita

```text
queued --scheduler--> running --complete_task completed--> done (pulizia worktree)
                         |--complete_task failed----------> failed
                         |--tab chiuso / agente uscito senza report--> failed (note)
                         |--avvio fallito (git, setup, precondizione)--> failed (note)
                         \--utente: Ferma--> failed (note "stopped by the user", tab chiuso)
                         \--workspace archiviato--> queued (note "interrupted: the workspace was archived";
                                                    i worker si staccano, le esecuzioni E2E si annullano)
failed (session-end, tab aperto) --complete_task dello stesso worker--> done | failed (note "reopened: ...")
failed --utente: Riprova--> queued (in fondo alla coda; worktree e branch si riusano)
queued|failed --utente/Intake: Elimina--> rimosso (solo senza dipendenti non done)
queued|failed --utente: Elimina tutti--> rimosso insieme ai dipendenti non done, diretti e indiretti
                                         (tutto o niente: se uno è running, errore e nulla cambia)
```

Una review (`kind: "review"`) segue lo stesso ciclo: `complete_task completed` la porta sempre a
`done`, **anche con `verdict: "issues"`** (il verdetto sta nel report); una review `failed` resta
`failed`. Un task di pianificazione (`kind: "plan"`) segue lo stesso ciclo: `completed` (con la
proposta o con le domande per l'utente nel `summary`) → `done`, senza consegna né diffstat.

**E2E nel ciclo.** `complete_task completed` di un task `work` con `e2e: true` passa dal blocco
(vedi "Test E2E", Il blocco): se è rifiutato il task resta `running`, senza altri effetti.
Quando il task lascia `running` (completato, fallito, fermato, tab chiuso) l'app emette
l'effetto `CancelE2e { ws_id, task_id }`, applicato fuori dal lock, che annulla le sue
esecuzioni in corso; archiviare o rimuovere il workspace annulla quelle di tutto il workspace.

- Al riavvio dell'app i `running` diventano `failed` (`interrupted: the app was restarted`): vedi "File su disco: schema, migrazione e recupero".
- `session-end` conta solo se viene dalla sessione dell'agente legata al worker (eventi di
  processi secondari, o senza id dopo il legame, ignorati con una traccia nella nota; senza
  legame un evento senza id vale come prima). Se fallisce comunque il task mentre
  il tab del worker e' aperto, il `complete_task` di quel worker lo riapre e lo conclude con il
  report, purche' nessun retry l'abbia ripreso (dettagli in DESIGN.MD, `automation/`).
- Stop hook del worker: se chiude il turno senza `complete_task` e senza lavoro in
  background, la prima volta l'app risponde con un blocco e il promemoria (inglese): "If the
  task is done, deliver it and call complete_task. If you are waiting for the user's
  decision, end your turn again: the user will answer here." Alla seconda il worker diventa
  `idle` → `attention`. `needs_input` (permesso in attesa) → `attention`.
- Domanda all'utente (solo con l'Agents HUB acceso): il worker chiama `ask_user`, che registra la
  domanda sul task e ritorna subito; il worker chiude il turno. Allo Stop hook con una domanda in
  attesa l'app **non blocca** il turno (niente promemoria) e il worker passa a `awaiting_answer`
  (non `attention`, perché l'utente risponde dall'HUB). La risposta (`answer_question`, vedi
  "Rispondere a una domanda") arriva nel terminale come prossimo messaggio dell'utente: la
  domanda si chiude e il worker torna `running`. Se invece l'utente preme Invio nel terminale
  mentre il worker è `awaiting_answer`, `pty_write` chiude la domanda senza testo (`answered_at`
  senza `answer`) e riprende il worker. Un permesso richiesto durante la domanda porta il worker a
  `needs_input` → `attention`: lì Invio approva il permesso e non chiude la domanda, e
  `answer_question` è rifiutata finché il permesso non è risolto.
- Notifiche di sistema (in italiano): quando un task entra in `attention` ("<titolo>
  richiede attenzione"), in `failed` ("<titolo> è fallito") o il suo worker comincia ad aspettare
  una risposta ("<titolo> ha una domanda"). Niente notifica per `done`.

## Autenticazione delle sessioni e ruoli

Le chiamate dei tool MCP e gli eventi degli hook arrivano da processi figli di un terminale, sullo stesso
socket locale (`127.0.0.1`). Perché un processo non possa spacciarsi per un'altra sessione ogni terminale ha
un **segreto suo**:

- **Emissione.** All'apertura del terminale (`pty_spawn`) `AutomationState::issue_credential` genera 32 byte dal
  generatore casuale del sistema (`getrandom`), 64 caratteri esadecimali, legati all'id del tab. Il segreto
  arriva alla shell in `CLI_COMPANION_TOKEN` (su WSL anche in `WSLENV`, con `/u`) insieme a
  `CLI_COMPANION_SESSION` (l'id del tab) e `CLI_COMPANION_PORT`. Riaprire un tab con lo stesso id sostituisce il
  segreto: il vecchio non vale più.
- **Verifica.** `--notify` e il server MCP lo rimandano nella riga JSON (`token`, `session`). Un evento hook senza
  `tab_id` valido e senza il segreto **di quel tab** è scartato dal cancello di `ipc` (non arriva né al core né
  al frontend, e il segreto non esce mai dal backend). Una RPC (`{"rpc":1,"token","session","method","params"}`) passa da
  `automation::auth::authenticate`: versione del protocollo, id del tab valido, segreto uguale (confronto a tempo
  costante). Id non valido, sessione sconosciuta e segreto sbagliato danno **lo stesso errore** (`invalid session
  credentials`): niente oracolo per chi prova a indovinare.
- **Revoca.** La credenziale vive quanto il flusso di output della shell: quando la shell esce o il terminale si
  chiude viene revocata (alla chiusura in ritardo di una vecchia credenziale, quella del tab riaperto con lo
  stesso id resta valida). Il segreto non compare mai nei log né in un `Debug`.
- **Self-test.** "Prova notifica" (`test_agent_notify`) usa una credenziale di prova usa e getta
  (`probe-<casuale>`), non quella di un terminale.

Il **ruolo** del chiamante lo decide l'app, non il chiamante: viene dal tab (`Role::of_worker`). Il worker di un task
`work` è `worker`, quello di un task `review` è `reviewer`, quello di un task `plan` è `planner`, ogni altro tab
di cli-companion (Intake compreso) è `session`. La tabella unica dei metodi consentiti a ogni ruolo è `Method::roles` in `automation/roles.rs`
(un `match` esaustivo: aggiungere un metodo obbliga a decidere chi lo chiama); `rpc::dispatch` la usa per il
controllo di accesso e il server MCP per scegliere i tool, e un test verifica che coincidano. Un metodo non consentito
risponde "`<metodo>` is reserved for automated sessions" (a una sessione) o "`<metodo>` is not available to
automated sessions" (a un worker); un tab di un workspace archiviato risponde "workspace is archived" a tutto.

| Metodo | `worker` | `reviewer` | `planner` | `session` |
|---|:-:|:-:|:-:|:-:|
| `whoami` (handshake del server MCP, non è un tool) | sì | sì | sì | sì |
| `get_task`, `complete_task` | sì | sì | sì | no |
| `ask_user` (solo con l'Agents HUB acceso) | sì | no | sì | no |
| `run_e2e`, `get_e2e_result` | sì | sì | no | no |
| `propose_tasks`, `get_task_details` | no | sì | sì | sì |
| `list_tasks`, `list_goals` | no | no | sì | sì |
| `get_goal` | no | sì | sì | sì |
| `get_workspace_settings` | no | sì | sì | sì |
| `update_task`, `delete_task`, `propose_settings` | no | no | no | sì |
| `save_skill`, `list_skills`, `delete_skill` | no | no | no | sì |

`ask_user` richiede l'Agents HUB (`Method::requires_hub`): è l'HUB che porta la domanda all'utente e raccoglie la risposta; senza HUB
nessuno potrebbe rispondere. Il revisore non lo ha apposta: la sua verifica non deve fermarsi su una risposta, e i dubbi
li mette nel verdetto.

## Tool MCP

Il server MCP sceglie il ruolo con la RPC interna `whoami` (non è un tool) ed espone tool diversi in base al tab: **worker** (`role: "worker"`, tab registrato come worker di
un task `work`), **reviewer** (`role: "reviewer"`, worker di un task `review`), **planner**
(`role: "planner"`, worker di un task `plan`) oppure **sessione**
(`role: "session"`, qualunque altro tab di cli-companion: Intake o terminale
normale). Fuori da cli-companion nessun tool. Se l'app non risponde a `whoami` entro 3 s l'handshake
non si blocca: la lista è vuota, il server riprova in background e, appena riceve il ruolo, avvisa il client
(`notifications/tools/list_changed`) e i tool compaiono. Descrizioni ed errori in inglese.
Le istruzioni del server MCP (inglese) dicono: you may propose tasks for this workspace's
queue with propose_tasks and read and propose changes to the workspace settings with
get_workspace_settings and propose_settings; the user approves them in the app; never wait for workers.

Worker:
- `get_task` → `{ task, title, instructions, environment, delivery, dependencies?, goal?, previous_attempt?, skill? }`.
  `dependencies` (omesso se vuoto): per ogni dipendenza diretta `done`, la stessa forma e gli stessi limiti di
  `reviewed_tasks`. `goal` (omesso per un task sciolto): `{ title, request, done_when, decisions }`, con `decisions`
  come `{ source, text }`. Se c'è almeno uno dei due, le istruzioni comuni guadagnano la riga "Read `dependencies`
  and `goal` before starting..." (vedi docs/GOALS.MD, "Passaggio di consegne"); altrimenti nulla cambia. Se il task ha
  una skill (snapshot del Bot), `skill` è il nome da caricare con lo strumento skill dell'agente e `instructions`
  inizia con "Before starting, load the skill <skill> with your skill tool; the instructions of this task take
  precedence over the skill where they conflict." (stesso per i task `review` e `plan`). Senza skill l'output non cambia.
- `complete_task { status: "completed" | "failed", summary, commit?, pr_url?,
  files_changed?, tests?, follow_ups? }` — `blocked` accettato come sinonimo di `failed`.
  Per i task con worktree l'app calcola il diffstat. Il tab si chiude dopo ~1,5 s.
  `verdict` e `findings` sono un errore su un task `work`. Per un task marcato E2E
  `completed` è soggetto al blocco (vedi "Test E2E"); il report riceve in `e2e` l'ultima esecuzione.
- `run_e2e {}` → `{ run_id, status: "running" }`. Ritorna subito: l'app lancia gli E2E del
  repository del task nella cartella del worker (vedi "Test E2E"). Errori (inglese): repository
  senza configurazione E2E ("E2E tests are not configured for repository '<repo>': ask the user
  to configure them in the repository settings"); esecuzione già in corso nella cartella ("an
  E2E run is already in progress in this folder (run_id ...): wait for it with get_e2e_result");
  nessuna porta libera; task non più assegnato alla sessione.
- `get_e2e_result { run_id?, wait_seconds? }` → l'`E2eRun` dell'ultima esecuzione del task (forma in
  "Modello dati"). Con `run_id` diverso dall'ultimo: errore `unknown run_id: the last E2E run of
  this task is <id>`; senza esecuzioni: "no E2E run for this task: call run_e2e first".
  `wait_seconds` intero 0..=25 (default 0): se l'esecuzione è `running` aspetta fino a quel tempo
  che finisca (attesa fuori dai lock). Il tetto di 25 s tiene ogni RPC sotto i 30 s di
  `RPC_TIMEOUT` del server MCP; il chiamante ripete finché lo stato non è più `running`.
  Valore fuori range: "wait_seconds must be an integer from 0 to 25".

Reviewer (worker di un task `review`; mai `update_task`, `delete_task`, `list_tasks`,
`propose_settings` o tool di skill; tutti gli altri metodi di sessione restano vietati):
- `get_task` → `{ task, title, instructions, environment, reviewed_tasks, previous_attempt? }`.
  `instructions` sono quelle del reviewer, **senza `delivery`**. `reviewed_tasks` ha, per ogni
  dipendenza diretta, `{ id, title, status, summary (troncato), commit, pr_url, files_changed,
  diffstat, tests, follow_ups, truncated? }` (liste e testi limitati; `truncated: true` se
  qualcosa è stato tagliato). Non include la specifica: il reviewer la legge con
  `get_task_details`.
- `complete_task { status, summary, verdict?, findings?, ... }` (come sopra, più i campi di verdetto):
  con `status: "completed"` serve `verdict: "passed" | "issues"`; `findings: string[]` (stessi
  limiti di `follow_ups`: max 100 voci, 1..=2000 caratteri) è obbligatorio e non vuoto con
  `issues`, ammesso come suggerimenti con `passed`. Con `status: "failed"` `verdict` è un
  errore (`findings` ammessi).
- `get_task_details { id }` e `propose_tasks { tasks }`: come per la sessione, ma la RPC li
  consente solo ai worker dei task `review`. I worker normali non vedono niente di nuovo. Lo schema di
  `propose_tasks` del reviewer (e del planner) non ha `goal` né `goal_id`: passarli è un errore ("goal and goal_id
  cannot be set by an automated session: …"); la proposta del reviewer **eredita** il goal della sua review
  (vedi "Goal").
- `get_goal { id }`: come per la sessione (il reviewer verifica l'obiettivo del goal).
- `get_workspace_settings {}`: sola lettura, nel workspace del task (come per il planner, con la
  descrizione che non cita `propose_settings`): il reviewer sceglie da `bots` il Bot e da
  `agents[].models` l'eventuale `model` dei task di correzione e della nuova review.
- `run_e2e` e `get_e2e_result`: gli stessi del worker, sulla cartella e sul task del reviewer
  (l'esecuzione sta in `Task.e2e_run` della review; non c'è blocco: il verdetto è del reviewer).

Istruzioni del server MCP per il reviewer (inglese): automated reviewer; call get_task first;
verify the integrated result without changing files; propose fixes with propose_tasks; always
finish with complete_task and a verdict.

`whoami` risponde anche `hub: bool` (l'Agents HUB è acceso) a ogni ruolo; il server MCP lo legge una
volta col ruolo (assente = spento). Con l'HUB acceso le istruzioni di worker e pianificatore dicono di
chiamare `ask_user` per un dubbio che blocca (invece di chiedere a parole); a HUB spento sono quelle di prima.

`ask_user { question, options? }` (worker e planner, **solo con l'HUB acceso**; elencato subito dopo
`complete_task`): chiede all'utente una domanda bloccante.
- `question`: stringa 1..=2000 caratteri; `options`: al più 5 stringhe di 1..=80 caratteri, su una riga, uniche.
  Lo schema del tool usa le costanti di `board/question.rs`.
- Risposta: `{ question_id, message: "Question sent to the user. End your turn now: the answer will arrive as
  the user's next message in this terminal." }`. Ritorna subito: il worker chiude il turno e la risposta
  gli arriva nel terminale.
- Errori (inglese): HUB spento ("ask_user is not available: the Agents HUB is off. Ask in plain text and end
  your turn."; senza HUB il tool non è nemmeno elencato); un'altra domanda in attesa ("a question is already
  waiting for the user's answer: end your turn now, the answer will arrive in this terminal"); parametri non
  validi (testo vuoto o troppo lungo, troppe opzioni, opzione vuota, su più righe, doppia o troppo lunga).

### Rispondere a una domanda (`answer_question`)

API interna per l'Agents HUB (`automation::answer_question(app, ws_id, task_id, question_id, text)`, non un
comando Tauri né un tool). Consegna la risposta dell'utente a una domanda del worker: la chiude sul task e la
digita nel terminale del worker, che riprende (`running`). Non blocca: la scrittura è sincrona sul terminale, l'invio
è differito di 150 ms. Errori in italiano, per l'interfaccia:
- "Task non trovato." / "Domanda non trovata." / "La domanda ha già una risposta.";
- "Il worker del task non è più attivo (tab chiuso o lavoro finito): la domanda resta registrata." (domanda non più in attesa o tab chiuso);
- mai sopra un permesso in attesa: "Il worker aspetta un permesso nel terminale: rispondi lì prima di inviare la risposta.";
- mai mentre lavora: "Il worker sta ancora lavorando: la risposta si può inviare quando si ferma in attesa.";
- "La risposta è vuota." / "La risposta è troppo lunga (massimo 4000 caratteri).";
- "Il terminale del worker non è raggiungibile: la domanda resta in attesa." (la domanda torna in attesa).

Nessun lock è in mano mentre si scrive nel terminale. Codifica (`automation/answer_input.rs`): a capo
normalizzati a `\n`, tabulazioni in spazi, ogni altro carattere di controllo rimosso (ESC compreso: nessuna
sequenza di escape), spazi ai bordi tolti; una riga si digita com'è, più righe si incollano con il
"bracketed paste" di xterm (gli a capo non inviano), poi un `\r` differito invia. Sono input standard dei
terminali dei coding agent TUI: non serve un contratto dell'agente (se un agente avesse bisogno di tasti
diversi, si aggiunge un contratto in `agents/contracts/` invece di distinguere l'agente nel core).

Planner (worker di un task `plan`; tool in `mcp/tools/planner.rs`; mai `update_task`,
`delete_task`, E2E, `propose_settings` o tool di skill):
- `get_task` → `{ task, title, instructions, environment, previous_attempt? }`, **senza
  `delivery`** (vedi "Istruzioni del pianificatore").
- `complete_task { status, summary }`: con `status: "completed"` `commit` e `pr_url` sono un
  errore ("commit and pr_url are not accepted for plan tasks: a plan delivers no code"), `verdict` e
  `findings` lo sono sempre ("only accepted for review tasks"); `failed` come per ogni worker.
  Niente diffstat. Il tool espone solo `status` e `summary`.
- `propose_tasks`: come per la sessione, con `source_task` = id del task `plan`. **Una sola**
  proposta registrata per worker: la seconda risponde "this plan task already made its proposal: a
  planner calls propose_tasks once. Call complete_task now." (una chiamata rifiutata dalla
  validazione non conta).
- `list_tasks`, `get_task_details`, `get_workspace_settings`: come per la sessione, nel workspace
  del task (la descrizione di `get_workspace_settings` non cita `propose_settings`).

Istruzioni del server MCP per il planner (inglese): automated planner; call get_task first;
explore without changing anything; a single propose_tasks call or the questions in the summary;
always finish with complete_task; nobody reads the terminal.

Sessione (workspace = quello del tab):
- `propose_tasks { tasks: TaskSpec[] }` — `TaskSpec = { id, title, task, bot, model?, agent?,
  depends_on?, repo?, kind? }` (`bot` obbligatorio nelle proposte: id di un Bot di `get_workspace_settings.bots` i cui `kinds`
  includono il `kind` della specifica (`general` solo per il lavoro, mai per una review), altrimenti "tasks[<i>]: task <id>: bot is required (see
  get_workspace_settings.bots)"; `model` è un override facoltativo del modello del Bot; `kind` `work` | `review`,
  default `work`; una `review` richiede almeno una dipendenza, controllata a proposta,
  accettazione, creazione manuale e aggiornamento: "task '<id>': a review task needs at least one
  dependency (depends_on: the tasks it reviews)"). `kind: "plan"` è rifiutato qui come in
  `update_task`, `task_create` e `task_update` (`kind "plan" is reserved to the app: …`), e un task
  `plan` non cambia tipo ("the kind of a plan task cannot be changed"). `e2e` (bool, default `false`) richiede che il
  repository del task abbia `e2e` configurato, controllato negli stessi quattro punti: "task
  '<id>': e2e requires an E2E command configured for repository '<repo>'". Valida tutto (id unici anche rispetto ai task esistenti e alle altre
  proposte, dipendenze verso la proposta stessa o task esistenti, niente cicli, `repo`
  esistente e git se la modalità usa branch). Crea una **proposta** pendente e ritorna
  `{ proposal, status: "pending" }`: "the user will accept or discard it in the app". Non
  aspetta la decisione. Con un Bot il task usa agente, modello, effort e skill del Bot: `model`
  (nome del catalogo dell'agente del Bot) lo scavalca, `agent` è facoltativo e deve coincidere con quello del
  Bot, altrimenti "task '<id>': agent <a> does not match the agent of bot <bot> (<agente>): omit agent or choose
  another bot". Errori del Bot: "task '<id>': unknown bot: <id> (see get_workspace_settings.bots)", "task '<id>':
  <nome> is disabled: enable it in the bots settings", agente non usabile (come sotto), "bot <bot> needs the
  skill <skill>, which <agente> cannot load: choose another bot". Senza Bot (task vecchi, creati dall'utente) `agent`
  (id di un agente, da `get_workspace_settings.agents`) è l'agente che esegue il task e `model` è un nome del suo
  catalogo; assente = quello del workspace (poi il predefinito globale). Lo stesso controllo vale a proposta,
  accettazione, creazione manuale e aggiornamento: "task '<id>': unknown agent: <id>" (o "<nome> is disabled…",
  "<nome> does not support automated sessions", "invalid model for <nome>: <modello>"). La proposta tiene la
  specifica com'è scritta; **all'accettazione** il Bot si risolve e agente, modello, effort e skill vengono
  **copiati nel task** (snapshot, con l'id del Bot per la UI): modificare o eliminare il Bot dopo non cambia i
  task già in coda.
- `list_tasks { status?: string[] }` → righe compatte `{ id, title, status, goal? (id del goal, se il task ne ha
  uno), kind? (solo `review` e `plan`), e2e? (solo `true`), verdict? (se il report ne ha uno), attention?,
  waiting_on?, blocked_by? (id dei `failed` da cui dipende un `queued`), fail_reason? (solo se `failed` e noto), agent_unavailable? (`{ agent, reason }`, vedi “Agente e modello di un task”), summary? (300),
  note? (200) }` + `counts` + `proposals_pending`.
- `get_task_details { id }` → task completo (testo, report, worktree, `agent`, `model`) con la sua vista (`agent_unavailable` compreso).
- `list_goals { open_only?: bool }` → `{ goals: [{ id, title, phase, round, waiting, tasks: { done, total } }] }`,
  nell'ordine della board (aperti e chiusi ancora sulla board; con `open_only: true` solo gli aperti).
  Errore: "open_only must be a boolean".
- `get_goal { id }` → `{ goal: { id, title, request, done_when, decisions, round, created_at, closed_at?, outcome?,
  review?, phase, waiting, progress: { done, total }, tasks: [{ id, title, status, kind, round }] } }`. Errore:
  "unknown goal: <id>".
- `update_task { id, title?, task?, depends_on?, repo?, bot?, agent?, model?, kind?, e2e? }` — solo `queued` o
  `failed`; `repo` solo senza worktree; il goal non cambia mai ("the goal of a task cannot be changed") e i nuovi
  `depends_on` rispettano le regole di appartenenza (vedi "Goal"). Ritorna il task senza testo. L'agente si ricontrolla
  solo se il patch contiene `agent`, `model` o `bot` (`agent: null` = quello del workspace o il
  predefinito globale, risolto adesso). Se il task ha un Bot, `agent` e `model` si azzerano salvo che il patch li
  contenga e il Bot si risolve di nuovo (nuovo snapshot); `bot: null` torna al comportamento senza Bot e azzera
  effort e skill. Un patch senza queste chiavi lascia lo snapshot com'è; con `kind` il Bot del task deve poter
  eseguire il nuovo tipo (vedi "Tipi di task dei Bot"). Il titolo di un task il cui agente è stato disabilitato
  si modifica comunque.
- `delete_task { id }` — solo `queued`/`failed` e senza dipendenti non `done`.
- `get_workspace_settings {}` → `{ settings, agents, bots, repos, allowed, limits, pending_settings_proposals }`:
  `bots` elenca **tutti** i Bot (abilitati o no), nell'ordine della lista, ognuno con `{ id, name, task, enabled,
  agent, agent_error, readiness, model, model_choice, effort, skill, kinds }`: `task` è a cosa serve il Bot (inglese),
  `kinds` i tipi di task che può eseguire (`[]` = solo interattivo),
  `agent` l'agente risolto nel workspace (`null` se non risolvibile, con `agent_error`), `readiness` quella di
  quell'agente nel workspace (`ready` = usabile), `model` il modello concreto risolto (`null` = predefinito della CLI).
  Le impostazioni che `propose_settings` può cambiare (`settings`, con `agent` e `worker_agents`), i
  repository con la loro configurazione (`null` = nessuna), i valori ammessi, i limiti
  (`limits.agents_per_proposal`) e le proposte di impostazioni pendenti. `agents` elenca gli **agenti
  abilitati**, nell'ordine del registro, ognuno con `{ id, name, is_default, models: [{ id, label, tier }],
  default_model, rule_format: { max_rules, max_rule_chars, example } | null, readiness }`: `tier` è `standard`
  (lavoro ben definito) o `strong` (progettazione, rischio, bug difficili); `default_model` è `null` se decide
  l'agente; `rule_format` è la forma delle regole `allow`/`deny` di *quell'* agente (`null` = non applica
  regole); `readiness` (`ready`, `not-found`, `not-installed`, `outdated`, `disabled`) è sondata al momento nel
  target del workspace, quindi la chiamata può richiedere un attimo. È la fonte da cui l'Intake e Project Setup
  ricavano agenti, modelli e sintassi delle regole: nessuna skill nomina un agente o un modello.
- `propose_settings { reason, worker_rules?, agent?, worker_permission?, worker_agents?, delivery?, use_schedule?, default_launch?, launches?, repos? }` — cambiano solo i
  campi passati; liste e `worker_rules` sostituiscono quelle attuali. `agent` (stringa o `null`) è l'agente dei
  worker del workspace: `null` = il predefinito globale, altrimenti un'integrazione registrata, abilitata e con
  l'automazione (stessi errori di `propose_tasks`). `worker_agents` ha per chiave l'id di un agente (registrato,
  anche se disabilitato: le sue impostazioni si conservano; al massimo `limits.agents_per_proposal` per proposta)
  e per valore `{ model?, allow?, deny? }`, almeno un campo: `model` è un nome valido per quell'agente (`null` = il
  suo predefinito), `allow` e `deny` sostituiscono le liste e rispettano `rule_format` dell'agente; una voce che
  resta vuota viene tolta. `delivery` accetta anche
  solo alcuni dei suoi campi (`mode`, `parallel`, `merge_method`); `repos` ha per chiave la
  cartella del repository (`.` = root) e per valore `{ base_branch?, container_root?,
  copy_ignored?, setup?, cleanup?, remove_force?, e2e? }` (`e2e`: `{ command, junit,
  timeout_min? }` al posto dell'attuale, `null` lo toglie) oppure `null` per toglierne la
  configurazione. Validazione tutto-o-niente; ritorna `{ proposal, status: "pending" }` e non
  aspetta la decisione (vedi "Proposte di impostazioni").
- `save_skill`, `list_skills`, `delete_skill` — le skill globali dell'utente (skill `skill-wizard`, Bot `skill-wizard`); non
  appartengono a un workspace e sono vietate ai worker (vedi "Autenticazione delle sessioni e ruoli").

## Test E2E

Gli E2E sono un'attivazione esplicita e facoltativa, in tre parti: configurazione del
repository, flag del task, esecuzione e blocco della consegna a carico dell'app
(`automation/e2e.rs` e `automation/e2e/`, parte pura in `board/e2e.rs`).

### Configurazione e flag

- **Configurazione del repository** (`RepoConfig.e2e`, `workspaces/e2e.rs`, opzionale, omessa se
  assente): `{ command, junit, timeout_min }`. `command`: una riga, non vuota, max 2000
  caratteri. `junit`: percorso relativo alla cartella del task, `/` come separatore, senza `..`,
  max 300 caratteri. `timeout_min`: da 1 a 60, predefinito 20 (i vecchi file senza il campo
  si leggono). Validata da `RepoConfig::normalized`, quindi anche da `set_repo_config` e da
  `propose_settings`; `get_workspace_settings` la restituisce con il resto della configurazione.
- **Flag del task** (`Task.e2e` / `TaskSpec.e2e`, bool, default `false`, omesso dal JSON se
  `false`): "questo task deve superare gli E2E". Ammesso solo se il repository del task ha `e2e`
  configurato (`check_e2e_configured`, `automation/worktrees.rs`), controllato a proposta,
  accettazione, creazione manuale e aggiornamento. Visibile in `list_tasks` (solo se `true`) e
  in `get_task_details`. Un task `e2e` in una cartella senza git è rifiutato a proposta, accettazione,
  creazione e aggiornamento, e fallisce all'avvio se la cartella non è più git ("task '<id>': e2e needs a
  git repository: <cartella> is not one …"): il blocco della consegna confronta un'impronta git del codice
  (vedi "Il blocco della consegna"). Le esecuzioni manuali (`run_e2e`, il pulsante) restano possibili.
  Vedi "Cartelle senza git".

### Esecuzione

Il comando lo lancia l'app, non la shell del worker: conta solo l'esito dell'app. `run_e2e`
(worker e reviewer; le sessioni normali non hanno questi tool) o l'esecuzione manuale (sotto)
avviano un job in background su un thread dell'host (`Host::defer`), mai sotto il lock del
registro: la chiamata ritorna subito lo stato `running`.

- **Dove**: nella cartella del task (worktree o checkout, nel formato del workspace: Windows o
  WSL); per l'esecuzione manuale la cartella del repository. Un'esecuzione alla volta per
  cartella (anche fra task e manuale); un'altra richiesta riceve l'errore "già in corso" con il
  `run_id` attivo.
- **Timeout**: `timeout_min` (poi `error`, `timed out after N s`).
- **Ambiente**: le variabili degli hook del repository (`REPO`, `WORKTREE`, `TASK_ID`, `BRANCH`,
  `BASE`, `CONTAINER_WORKTREE`) più `CLI_COMPANION_E2E_PORT`: una porta TCP locale libera
  riservata per l'esecuzione (mai la stessa per due esecuzioni in corso, rilasciata alla fine).
  È un contratto con i progetti che la usano (il nome non cambia): il comando la usa per il
  server dell'app sotto test. Con WSL2 la libertà della porta si verifica sul lato Windows;
  il servizio nella distro la riapre da sé.
- **Report vecchio**: prima di partire l'app cancella il file JUnit configurato (se non ci
  riesce: `error` "could not delete the old JUnit report"), così il report di un'esecuzione
  precedente non può essere scambiato per quello nuovo.
- **Impronta del codice** (`code`), calcolata all'inizio: `commit` (HEAD), `tree` (albero di
  HEAD) e `changes` (hash FNV-1a a 64 bit, 16 cifre esadecimali, dell'output di `git status
  --porcelain --untracked-files=no`: modifiche non committate dei file tracciati). Assente se
  git fallisce.
- **Output**: del comando si tengono solo gli ultimi 64 KiB per stream (stdout, stderr); in
  `output_tail` finiscono gli ultimi 4000 caratteri. Il testo d'errore è limitato a 1000.

**Verdetto** (deciso dall'app, senza modelli):

| Situazione | Stato |
|---|---|
| exit 0, JUnit presente con almeno 1 test e nessun fallimento/errore | `passed` |
| JUnit con almeno un fallimento/errore (qualunque exit) | `failed` |
| exit != 0 con report pulito | `failed` (`error`: "the command exited with an error although the report has no failures") |
| comando non partito, scaduto o annullato | `error` |
| JUnit assente, illeggibile (oltre 16 MiB, non UTF-8), XML non valido o senza test | `error` |

**Parser JUnit** (`automation/e2e/junit.rs`, quick-xml, entità esterne mai espanse): tollerante
sul contenuto (Playwright, Vitest, pytest, Surefire), rigido sulla forma (XML malformato o root
non `testsuites`/`testsuite` = errore). Conta per `testcase`: fallito se ha `failure` o `error`,
`skipped` se ha `skipped`; i `testsuite` annidati si contano una volta. Gli attributi
`failures`/`errors` di `testsuites`/`testsuite` non si perdono: i fallimenti sono il massimo fra
il conteggio dei testcase e quello degli attributi. `flaky` c'è solo se il report espone dei
nuovi tentativi (testcase passati dopo un retry). `failed_tests` riporta i primi 20 fallimenti
nell'ordine del report; `file` viene dal testcase, ripiego sul file o nome della suite; il
messaggio è la prima riga di `message` (o del testo di `failure`), ripiego "failed". Nome, file
e messaggio sono troncati (300, 300, 500 caratteri).

**Annullamento.** Le esecuzioni di un task si annullano quando il task lascia `running`
(completato, fallito, "Ferma", tab chiuso o agente uscito) e quelle del workspace quando lo si
archivia o rimuove. All'uscita dell'app si annullano tutte le esecuzioni in corso (di task e
manuali) e si aspetta, al massimo 12 s in tutto, che il loro albero di processi sia terminato,
prima di salvare le board; senza esecuzioni in corso l'uscita non aspetta. L'esecuzione annullata finisce `error` ("cancelled") senza leggere il JUnit.
Il processo e tutti i suoi discendenti (cmd, npx, node, browser) si uccidono: su Windows con
`taskkill /PID <pid> /T /F`; in WSL prima i processi Linux il cui ambiente contiene il marcatore
`CLI_COMPANION_RUN=<marcatore univoco>` (ereditato dai discendenti; lo script gira nella distro
e riceve il marcatore come argomento), poi l'albero di `wsl.exe`. Lo stesso vale per il
timeout e per gli hook `setup`/`cleanup`.

**Persistenza.** `Task.e2e_run` tiene l'ultima esecuzione (anche `running`) ed è azzerata quando
parte un nuovo tentativo (retry). L'esito sostituisce `running` solo se il `run_id` è ancora
quello del task. Alla conclusione `complete_task` copia l'esecuzione in `TaskReport.e2e` per
ogni tipo di task (anche `failed` e review); se l'esecuzione finisce dopo, l'esito aggiorna
anche la copia nel report. Al caricamento le esecuzioni `running` (anche nel report) diventano
`error` con la nota di interruzione, perché il processo non esiste più.

**Esecuzione manuale** (UI, per repository, fuori da ogni task): comandi Tauri `e2e_run_repo` e
`e2e_last_result` (vedi "Comandi ed eventi Tauri"). Parte nella cartella del repository, richiede
la configurazione `e2e`, ritorna l'esecuzione `running` ed emette `e2e-update` alla partenza e
alla fine. L'ultimo esito sta solo in memoria (per repository, perso al riavvio).

### Il blocco della consegna

Si applica solo a `complete_task` con `status: "completed"` di un task `work` con `e2e: true`.
Task non marcati e review: nessun blocco; `failed` è sempre accettato. L'impronta attuale si
calcola fuori dal lock (`git rev-parse HEAD HEAD^{tree}` + `git status`). Il rifiuto lascia il
task `running` con l'errore `complete_task rejected: <motivo> The task stays in progress.`, per:

- nessuna esecuzione ("this task requires a passed E2E run on the delivered code. Call run_e2e...");
- esecuzione ancora `running`;
- ultima esecuzione `failed` (test falliti su totali e primi 3 nomi) o `error` (con l'errore);
- esecuzione `passed` su codice diverso: si confrontano **`tree` e `changes`**, non il commit,
  quindi uno squash o un rebase che lascia lo stesso albero passa; il messaggio riporta commit
  allora e adesso e se le modifiche non committate differiscono; senza impronta salvata o con
  git non leggibile: "cannot verify the code against the last E2E run ... Run run_e2e again".

Configurazione tolta dopo l'assegnazione: nessun blocco; il task si completa con la nota
`E2E check skipped: the repository no longer has E2E tests configured`.

**Assunzione.** Nelle modalità `push` e `pr` il worker spinge **prima** di `complete_task` (i
passi di consegna mettono gli E2E prima del push). Il blocco garantisce che un task marcato non
diventi mai `done` senza un'esecuzione E2E passata sul codice consegnato; l'ordine "E2E prima
del push" è garantito solo dalle istruzioni (l'app non può impedire un push precedente).

## Task di review

Contratto del backend. La UI è descritta in "UI delle review".

Una **review** è un task `kind: "review"` che verifica il lavoro di altri task. È un normale
task di coda con `depends_on` su tutti i task di un obiettivo: lo scheduler lo avvia quando sono
tutti `done` (nessuna modifica allo scheduler).

- **Chi la crea**: Task Intake, quando serve, includendola nella proposta (nessun automatismo
  nell'app). Deve avere almeno una dipendenza.
- **Chi la esegue**: un **reviewer** (`whoami` → `role: "reviewer"`), con istruzioni proprie
  (`reviewer_instructions.md`) e i tool descritti in "Tool MCP". Non modifica file tracciati, non
  fa commit/push/merge/branch e lascia il checkout com'era; in modalità PR verifica lo stato
  integrato con `git fetch origin` e `origin/<base>`. Legge le specifiche con `get_task_details`,
  i diff dei task rivisti, esegue build e test richiesti, se una specifica rivista ha scenari E2E esegue la suite E2E sullo stato integrato e controlla che ogni scenario abbia un test (scenario senza test o test che fallisce = difetto reale), controlla ogni "Done when" e
  l'obiettivo end to end (compresa l'integrazione tra i task). Segnala solo difetti reali:
  obiettivo mancato, bug, regressioni, test mancanti, sicurezza e le **violazioni chiare dello
  standard di ingegneria introdotte dai diff** dei task rivisti (test mancanti o che non possono
  fallire, file cresciuto oltre la soglia o con responsabilità mescolate, helper esistenti
  duplicati, errori ignorati, documentazione non aggiornata, report che dichiara verifiche non
  eseguite, regole su concorrenza o UI violate quando si applicano), ognuna con la regola citata
  (numero e nome) e il punto del codice (`file:riga`). Stile, gusto, miglioramenti opzionali e
  codice preesistente fuori dai diff vanno in `findings` come suggerimenti, mai in un task di
  correzione.
- **E2E**: se almeno un task rivisto ha `e2e: true` **e** il repository della review ha ancora
  la configurazione `e2e`, le istruzioni del reviewer ricevono in coda un paragrafo (`E2E_REVIEWER`):
  `run_e2e` sullo stato integrato, `get_e2e_result` con `wait_seconds: 25` finché non è più
  `running`; esito `failed` o `error` = verdetto `issues` con i test falliti (o l'errore) in
  `findings`; l'esito va citato nel `summary`, e l'app allega l'esecuzione al report (`e2e`).
  La review non ha blocco di consegna.
- **Cartella senza git**: la review di un task in una cartella senza git (modalità libera) non ha worktree,
  branch integrato, commit né PR: le istruzioni del reviewer ricevono in coda un paragrafo che dice di
  ignorare le istruzioni git e di rivedere i file direttamente nella cartella (vedi "Cartelle senza git").
- **Esito**: `complete_task` con `verdict` `passed` o `issues` (+ `findings`), salvati nel
  `TaskReport`. La review completata va sempre a `done`, anche con `issues`; se non può essere
  eseguita, `status: "failed"` e resta `failed`.
- **Problemi**: il reviewer chiama `propose_tasks` con task di correzione autosufficienti
  (Goal, Context, Constraints, Done when, Verify; `model` obbligatorio; nella lingua del testo
  dei task) più una nuova `review` che dipende dalle correzioni e riafferma l'obiettivo, poi
  `complete_task` con `verdict: "issues"`. Non attende la decisione sulla proposta, che porta
  `source_task`.
- **Nessun ciclo automatico**: le correzioni le decide l'utente accettando o scartando la
  proposta.

## Task di pianificazione

Contratto del backend (tappa 3 dell'Agents HUB). Un **task di pianificazione** è un task
`kind: "plan"`: un coding agent esplora il repository in sola lettura, scrive le specifiche e chiude
con una proposta di task, o con le domande per l'utente. È un `kind` e non un Bot perché non
consegna codice: niente branch da integrare, niente istruzioni di consegna. Regole pure in
`board/plan.rs`, lato automazione `automation/planner.rs`.

- **Chi lo crea**: solo l'app, con l'API interna `automation::create_plan_task(st, host, ws_id,
  &PlanRequest { title, task, model?, repo?, bot? })`. L'Agents HUB passa da
  `planner::create_hub_plan_task` (dall'app reale `automation::queue_hub_plan_task(app, ws_id, title, task, bot)`,
  che fa git: fuori dal main thread), con il repository radice e il **Bot scelto dal segretario**, obbligatorio:
  agente, modello, effort e skill del task sono la copia di quelli del Bot (come per i task proposti, vedi il punto
  "Bot." più sopra). Un Bot sconosciuto, disabilitato o senza `plan` fra i suoi `kinds`
  è un errore, senza ripieghi (il segretario vede nel system prompt solo i Bot abilitati che pianificano); non c'è più un modello scelto dall'app per il pianificatore. Le proposte l'HUB le
  decide con `automation::decide_hub_proposal`, lo
  stesso `proposal_decide` dei pulsanti del pannello Task. Il task entra **direttamente in coda** (niente proposta), con
  l'id generato dal titolo come `task_create` e, senza Bot, l'agente del workspace; `model` assente = quello del Bot
  o, senza Bot, quello dei worker del workspace. Titolo, testo, modello e repo si validano come ogni task; il workspace non deve
  essere archiviato e la cartella del piano può
  essere un repository git (worktree, come sempre) oppure no (gira sul posto in sola lettura, vedi
  "Pianificazione in una cartella senza git"). Il repository, se indicato, deve esistere.
- **Mai da fuori**: `propose_tasks` (Intake, revisore, pianificatore), `update_task`, `task_create` e
  `task_update` rifiutano `kind: "plan"`. `task_create` non lo espone di proposito: un piano ha senso
  solo con chi ne consuma l'esito (l'HUB, che lega proposta e domande al suo thread); creato a mano
  sarebbe un Intake senza utente. Un `plan` in coda si modifica (titolo, testo, modello…) ma non
  cambia tipo e non accetta `e2e: true` ("a plan task cannot require E2E tests"). `depends_on` è
  ammesso ma non obbligatorio.
- **Esecuzione**: sempre in un worktree `task/<id>` dalla base (tranne in una cartella senza git, vedi
  "Pianificazione in una cartella senza git"), anche nelle modalità senza worktree:
  il pianificatore non tocca mai la copia di lavoro dell'utente, e un worktree è più semplice da
  garantire di un "non scrivere" affidato alle istruzioni. Niente hook `setup`, niente precondizione
  `local_merge`. Lo scheduler lo conta in `max_parallel` ma **non occupa il repository**: nelle
  modalità in place può girare accanto al task `work` del checkout.
- **Chi lo esegue**: un **planner** (`whoami` → `role: "planner"`), con le istruzioni di
  `planner_instructions.md` (derivate da `agents/presets/task-intake.md`): esplorazione in sola
  lettura, coda letta per evitare duplicati e trovare le dipendenze, task autosufficienti (Goal,
  Context, Constraints, Done when, Verify), dimensionamento, review di obiettivo, scenari E2E se il
  repository ha una suite. Differenze dall'Intake: nessun utente nel terminale, nessuna modifica né
  commit, al massimo **una** proposta e poi `complete_task`; se la richiesta è troppo ambigua non
  propone e chiude `completed` con un `summary` che inizia con "Questions for the user:" e le domande
  numerate. Se la richiesta è solo una domanda e non chiede modifiche (o lo chiede la skill del Bot) non
  propone e chiude `completed` con un `summary` che inizia con "Answer:" e la risposta: mai per evitare di proporre.
- **Interfaccia**: badge «Pianificazione» (`PlanBadge`, `lib/plan.ts`) su schede, righe, nodi, pannello Task e dettaglio; nel
  dettaglio un `plan` concluso con «Questions for the user:» mostra le domande in evidenza («Domande del pianificatore»),
  uno concluso con «Answer:» il badge «Risposta» (`PlanAnswerBadge`) e la risposta.
  Le azioni disponibili sono quelle di `allowed_actions`, come per ogni task.
- **Esito**: `complete_task` senza `commit`, `pr_url`, `verdict`, `findings` (vedi "Tool MCP");
  `done` → tab chiuso → worktree rimosso e branch cancellato (`-D` in `pr`, `-d` altrimenti), mai
  merge, push o PR. Un piano `failed` tiene il worktree per il retry, come ogni task; eliminarlo lo
  ripulisce.

### Pianificazione in una cartella senza git

Un workspace può contenere più progetti: la root può non essere un repository git, le sottocartelle
dirette sì (per esempio `backend/` e `frontend/`), oppure nessuna lo è. Un piano di una cartella senza
git (la root o una sottocartella indicata da `repo`) non ha un worktree da creare e gira sul posto.
Codice in `automation/planner/root.rs`.

- **Quando**: si decide all'avvio (`prepare_start` → `planner::root::start`), in modo deterministico:
  task `plan` senza worktree già registrato la cui cartella non risponde `true` a
  `git rev-parse --is-inside-work-tree`. Non c'è più nessun errore "no repository": senza git la cartella è
  semplicemente in modalità libera. Con la cartella git tutto resta com'è (worktree).
- **Avvio**: `cwd` = la cartella; nessun worktree, branch, base, fetch né setup, e nessun controllo sul
  checkout o sulla modalità di consegna. Lo scheduler lo tratta come ogni `plan` (non occupa il
  repository), ma due task della stessa cartella senza git non girano insieme.
- **Cosa sa** (`get_task`): alle istruzioni del pianificatore si aggiunge `prompts/planner_root.md`
  (inglese): lavora in una cartella che non è un repository git; **sola lettura** stretta (niente file
  creati, modificati o cancellati, niente git commit/checkout/stash, installazioni o build che scrivono);
  elenca i repository git rilevati e dice che ogni cartella senza git (root compresa) esegue i suoi task
  in modalità libera, senza passi git e senza `e2e`. L'`environment` (`planner_environment`) dice root,
  repository e cartella di lavoro, senza worktree.
- **Proposta**: un task può avere `repo` uguale a un repository git, a un'altra cartella esistente della
  root, oppure nessun `repo` (la root stessa). Non c'è più il controllo "ogni task deve avere un repo
  rilevato"; valgono i controlli di sempre (`ensure_repos`, `e2e` rifiutato nelle cartelle senza git).
- **Controllo di sola lettura**: resta solo per i repository git. All'avvio l'app salva per ogni
  repository `HEAD` e `git status --porcelain --untracked-files=all`. Alla chiusura del tab (dopo
  `complete_task`, completato o fallito, o a un tab chiuso prima; anche per il worker che non è mai
  partito) rilegge lo stato su un thread (`Effect::ReadOnlyCheck`) e confronta. Se un repository è
  cambiato il task resta concluso com'è ma riceve la nota "il pianificatore ha modificato file in:
  backend, frontend (doveva lavorare in sola lettura)" (accodata a quella che c'è) e un avviso
  `attention`. Nulla si annulla. Un repository che non si legge (git assente, errore) si scrive nei log e
  non si confronta. Nelle cartelle senza git non c'è un confronto: la sola lettura è affidata alle istruzioni.
- **Fine**: niente diffstat, niente worktree da rimuovere né branch da cancellare.

## Cartelle senza git

Modalità libera (`no_git`): quando la cartella di un task (la root del workspace o la sottocartella `repo`)
**non è un repository git**, il task gira direttamente in quella cartella, qualunque sia la consegna del
workspace (`pr`, `local_merge`, `push`, `commit`). Niente worktree, branch, commit, merge, push né PR: la
consegna sono i file lasciati nella cartella e il report di `complete_task`, che elenca i file cambiati.
Codice in `automation/worktrees/start/free.rs` (avvio) e `board/texts/free.rs` (testi).

- **Regola**: `ensure_repos` non rifiuta più una cartella senza git con `pr`/`local_merge`; la cartella deve
  però esistere. Se git non è disponibile sul sistema è un errore ("git is not available: …") solo con le
  modalità con branch (`pr`/`local_merge`); con `push`/`commit` la cartella conta come senza git.
- **Campo `no_git`** (`Task.no_git` / `TaskView.no_git`, bool, omesso se `false`): fissato alla creazione e
  all'accettazione per i task in coda (serve alla UI e allo scheduler) e riletto con autorità all'avvio. Un
  retry che ha già un worktree è sempre un task git.
- **Avvio**: `cwd` = la cartella; nessun fetch, worktree, base, hook `setup`. Un piano gira sul posto in sola
  lettura (vedi "Pianificazione in una cartella senza git").
- **Istruzioni** (`get_task`, in inglese): al posto dei passi di consegna git c'è "free mode (the folder is
  not a git repository)": lavora nella cartella, non c'è nulla da committare né pushare, non cancellare né
  annullare modifiche altrui, e chiudi con `complete_task` `completed` con un riassunto che elenca i file
  cambiati (creati, modificati, cancellati). L'`environment` dice che la cartella non è un repository.
- **Fine del task**: niente diffstat, niente pulizia del worktree; la riconciliazione dei worktree li ignora.
- **Scheduler**: due task della stessa cartella senza git non girano mai insieme, nemmeno in modalità
  parallela (condividono la cartella).
- **Review**: rivede i file direttamente nella cartella (vedi "Task di review").
- **E2E**: un task `e2e` in una cartella senza git è rifiutato alla creazione, proposta, accettazione e
  aggiornamento, e fallisce all'avvio, perché il blocco della consegna richiede un'impronta git del codice
  ("task '<id>': e2e needs a git repository: <cartella> is not one … Remove e2e or ask the user to run git
  init there"). Le esecuzioni E2E manuali funzionano.
- **UI**: nel nodo del grafo Dipendenze, dove altrimenti c'è il branch, un task `no_git` mostra
  "cartella senza git" con il tooltip "Modalità libera: la cartella non è un repository git, il worker
  lavora direttamente lì senza branch né commit" (`isNoGit`, `NO_GIT_HELP` in `lib/board.ts`).

## Proposte (approvazione)

- `propose_tasks` aggiunge una `Proposal` alla board → `board-update`.
- UI: card "Proposta da Task Intake" in cima al pannello Task del workspace (e nel Tasks
  Manager) con titolo/i, estratto della specifica, dipendenze, modello e **Scarta** /
  **Accetta**. Nessuna modifica: per cambiarla l'utente la chiede all'Intake.
- Accetta → i task entrano in coda (`queued`, in fondo, nell'ordine della proposta) dopo una
  nuova validazione; se fallisce (es. un id nel frattempo occupato) la card mostra l'errore.
- Scarta → la proposta sparisce. Nessun feedback all'Intake.
- Una proposta fatta dal reviewer di una review registra `source_task` (id della review) e
  `source_tab` (tab del reviewer); lo stesso per il pianificatore di un task `plan` (id del task
  `plan`, al massimo una proposta per worker); le proposte dell'Intake non hanno `source_task`.
- Comando Tauri `proposal_decide(workspace_id, proposal_id, accept: bool) -> BoardView`.
- Una proposta con `goal` nuovo crea il goal **solo all'accettazione**; scartarla non crea nulla. Vedi "Goal".

## Goal

Il goal (obiettivo) è l'oggetto tracciato sopra i task: nasce da una proposta accettata, raggruppa i suoi task, ha
una fase calcolata e una chiusura. Il progetto completo, con le tappe successive (branch del goal, consegna,
correzione automatica, follow-up), è in [GOALS.MD](GOALS.MD); qui il contratto consegnato (tappa T2). Il codice è
in `board/goal.rs` e `board/goal/` (puro, senza orologio).

```jsonc
{
  "id": "export-ordini",      // ^[a-z0-9][a-z0-9-]{0,39}$, unico fra i goal della board
  "title": "...",             // 1..=80
  "request": "...",           // la richiesta dell'utente con le sue parole, 1..=4000
  "done_when": ["..."],       // criteri di riuscita dell'obiettivo: 1..=20, 1..=300 caratteri ciascuno
  "decisions": [              // registro delle decisioni: max 50, le più vecchie cadono
    { "at": 0, "source": "intake", "text": "..." }   // source: intake | user | app; text 1..=500
  ],
  "round": 1,                 // 1 = il lavoro iniziale; +1 a ogni proposta di correzione accettata
  "created_at": 0,
  "closed_at": 0,             // presente = chiuso (omesso se aperto); un goal chiuso non si riapre
  "outcome": "delivered",     // solo se chiuso: delivered | abandoned
  "review": "passed"          // solo se chiuso: passed | issues | none (verdetto della review del goal conclusa per ultima)
}
```

I campi delle tappe successive (`branches`, `auto_fix`, `auto_deliver`, `report`, `follow_ups`) non esistono
ancora. Limiti: `MAX_OPEN_GOALS` = 50 goal aperti per board ("too many open goals on the board (max 50): add the
tasks to an open goal with goal_id").

**Nascita** (`propose_tasks`, poi l'accettazione dell'utente):

| Origine | Goal dei task |
|---|---|
| proposta dell'Intake con `goal` | goal creato all'accettazione; i task ricevono `goal` e `round: 1`; le `decisions` entrano nel registro come voci `intake` con l'ora dell'accettazione |
| proposta dell'Intake con `goal_id` | i task entrano nel goal aperto con il suo `round` corrente |
| proposta del reviewer di una review con goal | **ereditato dall'app** (`goal_id` = goal della review, il reviewer non indica nulla); all'accettazione il goal passa al giro successivo e i task entrano con il nuovo `round` |
| proposta del reviewer di una review sciolta, del planner | nessuno (un worker non può indicare `goal`/`goal_id`) |
| task creato a mano (`task_create` con `goal`) | il goal aperto indicato, al giro corrente |
| proposta senza `goal` né `goal_id` | nessuno: task sciolti, come prima dei goal |
| task `plan` | mai ("a plan task cannot belong to a goal") |

Una proposta di correzione è quella con `source_task` e `goal_id` (`Proposal::is_correction`). Errori (inglese, a
proposta e di nuovo all'accettazione, che lascia la proposta pendente): "pass goal (a new goal) or goal_id (an open
goal), not both", "unknown goal: <id>", "goal <id> is closed: propose a new goal", "goal id already in use: <id>"
(un goal della board o il goal nuovo di un'altra proposta pendente), i limiti dei campi (`goal.title is too long
(max 80 characters)`, `goal.done_when is empty: …`, …).

**Appartenenza.** Un task sta al più in un goal e tutti i task di una proposta nello stesso goal. Un task di un goal
dipende solo da task dello stesso goal ("task '<id>': depends on '<x>', which belongs to another goal", "… which is
not part of goal '<g>'"); un task sciolto non dipende da task di un goal aperto ("… which belongs to the open goal
'<g>'"). Le regole valgono a proposta, accettazione, creazione manuale e `update_task`. Il goal di un task non
cambia mai.

**Fase e attesa** (`GoalView`, derivate da `board/goal/phase.rs`, mai su disco né impostabili):

| `phase` | Quando |
|---|---|
| `executing` | almeno un task `work` del goal non è `done` |
| `verifying` | i `work` sono `done` e una review del goal non lo è (in coda, in corso o fallita) |
| `deciding` | nessun task aperto, una proposta del goal è pendente |
| `ready` | nessun task aperto né proposta pendente: in questa tappa non dura, il goal si chiude subito |
| `closed` | `closed_at` presente |

`waiting` (vuoto = il goal procede da solo), nell'ordine: `proposal` (una proposta del goal è pendente), `failed`
(un task del goal è `failed`), `attention` (un task del goal ha il worker in attesa dell'utente). Un goal chiuso non
aspetta nulla.

**Chiusura** (l'unico stato del goal scritto su disco):

- **da sola** (regola provvisoria fino alla consegna del goal): a ogni pubblicazione della board
  (`Board::close_finished_goals`, prima della vista e dell'archivio) un goal aperto senza task aperti (tutti `done`)
  né proposte pendenti si chiude `delivered`; un goal rimasto senza task (tutti eliminati) si chiude `abandoned`;
- **"Chiudi goal"** (comando `goal_close`): elimina i task `queued` e `failed` del goal, con i task non `done` che ne
  dipendono, scarta le sue proposte pendenti e lo chiude `abandoned`. Tutto o niente: se uno dei task da togliere è
  `running` risponde "cannot close goal '<id>': <ids> are running, stop them first" senza toccare la board. I
  worktree dei task tolti vengono ripuliti. I task `done` restano.

In entrambi i casi `review` è il verdetto della review del goal conclusa per ultima (`none` se nessuna). Un goal
chiuso rifiuta nuovi task e note.

**Note dell'utente** (comando `goal_add_note`): una voce `user` nel registro delle decisioni di un goal aperto
(1..=500 caratteri; oltre 50 voci cadono le più vecchie).

**Vista.** `BoardView.goals: GoalView[]` (i campi del goal più `phase`, `waiting` e `tasks: { done, total }`), nell'ordine
della board; ogni `TaskView` porta il suo `goal` e `round`; le proposte portano `goal`/`goal_id`.

### Proposte di impostazioni

- `propose_settings` aggiunge una `SettingsProposal { id, source_tab, created_at, reason, patch }`
  alla board (campo `settings_proposals`, tetto separato da quello delle proposte di task) →
  `board-update`. La vista espone anche i valori **attuali** dei soli campi toccati e l'elenco dei
  campi **sensibili**.
- Campi sensibili (eseguono comandi o allargano i permessi): `worker_permission`,
  `worker_agents[<agente>].allow`, `worker_agents[<agente>].deny`, `launches`, `default_launch`, `repos[<repo>].setup`, `repos[<repo>].cleanup`,
  `repos[<repo>].e2e` (se impostato; toglierlo non è sensibile).
- UI: card "Proposta di impostazioni" in cima al pannello Task (e nel Tasks Manager): motivazione
  e, per ogni campo, il confronto **prima → dopo** (testi lunghi compressi con "Mostra tutto";
  liste come righe tolte/aggiunte; i repository raggruppati per cartella). I campi sensibili sono
  evidenziati con un'avvertenza. **Scarta** / **Applica**. Nessuna modifica dalla card.
- Applica → la patch viene **rivalidata** sul workspace di quel momento (può essere cambiato dopo
  la proposta) e applicata; se la rivalidazione fallisce la card mostra l'errore e la proposta
  resta. Consegna e fasce orarie riallineano lo scheduler. Scarta → la proposta sparisce. Nessun
  feedback all'agente.
- Comando Tauri `settings_proposal_decide(workspace_id, proposal_id, accept: bool) ->
  { board: BoardView, workspace: Workspace | null }` (`workspace` valorizzato se applicata).

## Skill del plugin e Bot

Registro nell'app (`agents`): `Preset { id, name, skill }` (le skill del plugin; gli avvii sono i Bot) con lo SKILL.md (inglese)
incluso con `include_str!`. Il pacchetto di ogni integrazione che dichiara `plugin-skills` (contratto `PluginSkills`,
solo `skill_path`) contiene la skill di ogni preset (per Claude Code `skills/<id>/SKILL.md` nel plugin; non più
`orchestrate`/`workflow`: l'installazione rimuove le cartelle obsolete). Le skill non hanno voci rapide proprie
(niente sottomenu “Avvia con…”): una sessione guidata da una skill del plugin parte solo
come **Bot** (`bot:<id>`, `bots/launches.rs`), che porta agente, modello, effort e skill nella sua configurazione
(`bots.json`). Un Bot con `work` fra i suoi `kinds` (General, Bug Fixer, Refactorer, Test Writer, Docs Writer, Git
Operator, Release e i Bot dell'utente con `work`) avviato in un tab riceve anche la **variante interattiva dello
standard di ingegneria** come istruzioni di sistema, ripristino dopo un riavvio compreso (vedi "Standard di
ingegneria." in "Istruzioni di consegna"); Intake, Setup, Skill Wizard (`kinds` vuoti), i Bot solo `review` o `plan`
e i tab dell'agente puro non la ricevono. I Bot predefiniti `intake`, `setup` e `skill-wizard` usano le skill `task-intake`, `project-setup` e
`skill-wizard`; `code-answer` e `bug-fix` sono skill da worker dei Bot `quick-answer` e `bug-fixer` (vedi sotto e
"Bot predefiniti"); `code-research`, `test-qa`, `docs-writer` e `safe-refactor` sono le skill da worker dei Bot
`researcher`, `test-writer`, `docs-writer` e `refactorer`; `git-ops` e `release-prep` sono
le skill da worker dei Bot `git-operator` e `release`, spenti di serie (vedi "Bot predefiniti"). Un Bot con una skill del plugin richiede un agente che dichiara `plugin-skills`. Una skill non
nomina mai un agente o un modello: li legge da `get_workspace_settings.agents`.
- Migrazione: un `default_launch` salvato come `builtin:orchestrator` o `builtin:preset:task-intake` diventa `bot:intake`
  (`builtin:preset:project-setup` → `bot:setup`, `builtin:preset:skill-wizard` → `bot:skill-wizard`,
  `builtin:preset:<preset>@<agente>` → `bot:<bot>-<agente>`, un Bot derivato creato una volta sola all'avvio): vedi
  DESIGN.MD, `launch_migration/`. Una versione precedente dell'app mette da parte i file migrati.
- Prima skill: **`task-intake`**, nome "Task Intake". Skill (inglese): accogli la richiesta;
  fai domande solo se servono; esplora il repository (file, convenzioni, test, comandi di
  verifica, CLAUDE.md); se il lavoro è grande dividilo in pochi task con dipendenze;
  mostra all'utente in chat ogni task (titolo + specifica) prima di proporlo; poi
  `propose_tasks`; spiega che l'approvazione avviene nel pannello Task. Modello della
  specifica: **Goal, Context, Constraints, Done when, Verify** (comandi). Non implementare
  mai il lavoro, non toccare file, non fare merge; può usare `list_tasks`,
  `get_task_details`, `update_task`, `delete_task` per lo stato della coda. Ogni task ha
  sempre `bot`, scelto fra i Bot di `get_workspace_settings.bots` abilitati e con agente `ready` i cui `kinds`
  includono il `kind` del task, e fra questi quello il cui `task` corrisponde al lavoro (la skill non nomina id fissi
  di Bot, quindi anche i Bot nuovi sono scelti), altrimenti avvisa l'utente; `model` è solo un override
  facoltativo quando serve una fascia diversa da quella del Bot (`tier: "standard"` per lavoro ben
  definito, `"strong"` per design/rischio), letto da `get_workspace_settings.agents` e mai a memoria; `agent` si omette
  (deve coincidere con quello del Bot). La specifica deve
  bastare a un worker che non vede la conversazione. Quando la richiesta produce 2 o più task
  (o un solo task rischioso o la correzione di un bug difficile) aggiunge in fondo alla proposta
  una **review** del goal (`kind: "review"`, un Bot i cui `kinds` includono `review` con modello della fascia `strong`, se l'utente vuole un controllo
  indipendente anche su un altro agente (un Bot che lo usa), `depends_on` su tutti i task del goal); l'utente può scartarla.
- Il pannello dei terminali mostra il badge **INTAKE** sull'intestazione dei tab avviati da un
  Bot con la skill `task-intake`.
- Task Intake sa dei tool di impostazioni: se la richiesta riguarda la configurazione del
  workspace e non il lavoro sul codice, non crea un task ma usa `get_workspace_settings` /
  `propose_settings`, oppure suggerisce Project Setup.
- **Scenari E2E (opzionali)**: se il progetto ha gia una suite E2E (`playwright.config.*`, `cypress.config.*`, cartelle `e2e/` o `tests/e2e/`, script `test:e2e`, pytest-playwright), per i task che cambiano un flusso visibile la specifica contiene una sezione **E2E scenarios**: elenco numerato punto di partenza -> azioni in parole semplici -> esito osservabile (con il caso negativo se conta), mai selettori, ripetuti in "Done when". I task senza flussi visibili non hanno scenari. Senza suite l'Intake non scrive scenari e non propone di configurarla. Solo su richiesta esplicita dell'utente il task di configurazione prevede Playwright (salvo un framework gia in uso), `webServer` sulla porta di `CLI_COMPANION_E2E_PORT` con `baseURL` coerente, reporter `junit` su `e2e-results/junit.xml` e `html` (`open: "never"`), un test di base e i browser documentati nel README.
- **`project-setup`**, nome "Project Setup" (Bot predefinito `bot:setup`, skill
  `/cli-companion-notify:project-setup`). L'agente con cui l'utente configura il workspace e il
  modo in cui ci lavorano i worker. Skill (inglese): solo proposte, nessuna modifica diretta di
  file o impostazioni, nessun git che cambia stato; legge prima `get_workspace_settings`;
  esplora in sola lettura il repository e i sotto-repository per ricavare la configurazione dei
  worktree (file ignorati ma necessari → `copy_ignored`; docker-compose/Dockerfile → servizio e
  `container_root`; package manager → comandi `setup` dopo la creazione e `cleanup` prima della
  rimozione; branch di default → `base_branch`); comandi di setup ripetibili (al retry si
  rifanno) e su una riga sola, con le variabili nella sintassi della shell del workspace
  (`%WORKTREE%` in `cmd` su Windows, `$WORKTREE` in `sh` su WSL). Progetti PHP gestiti da
  **Lampo** (solo workspace Windows, `lampo.exe` installato e repository registrato in Lampo):
  propone nel `setup` `lampo.exe worktree up "%WORKTREE%" --db shared --json` e nel `cleanup`
  `lampo.exe worktree down "%WORKTREE%"` (percorso completo tra virgolette), così ogni worktree ha
  il suo sito di anteprima; con `delivery.parallel` e task che toccano lo schema chiede tra DB
  `shared` e `fresh`/`clone:<db>` (con `fresh` aggiunge le migrazioni, con un DB per worktree
  aggiunge `--drop-database` e avverte che il `.env` copiato punta ancora al DB del padre). Una
  domanda alla volta e solo se cambia la configurazione;
  propone solo i campi da cambiare; mostra in chat il prima → dopo e il perché; poi
  `propose_settings` con un `reason` chiaro (correggendo e richiamando in caso di errore di
  validazione) e chiude il turno spiegando che la proposta attende nel pannello Task, senza
  attendere né fare polling. Campi sensibili (permessi, allow/deny, avvii, `setup`/`cleanup`):
  solo se servono o se l'utente li chiede, con il rischio spiegato, mai `bypass` senza una
  richiesta esplicita. Spiega le modalità di consegna e che la configurazione dei worktree vale
  solo con le modalità parallele. Parla con l'utente nella sua lingua; regole e testi per i
  worker in inglese, salvo diversa richiesta.
- **Skill da worker** (`code-answer` "Code Answer", `bug-fix` "Bug Fix", `code-research` "Code Research", `test-qa`
  "Test QA", `docs-writer` "Docs Writer", `safe-refactor` "Safe Refactor", `git-ops` "Git Ops",
  `release-prep` "Release Prep"): le carica un worker il cui Bot le nomina
  (vedi `get_task.skill`); la `description` limita l'attivazione a un task di cli-companion il cui Bot nomina la
  skill, perché le skill del plugin compaiono anche nelle sessioni interattive. Sezioni `When this applies`, `Rules`,
  `Workflow`, `Report`; non ripetono lo standard dei worker né le istruzioni di consegna (vedi POOL.MD §5).
  `code-answer` (task `plan`): sola lettura, git solo in lettura, nessuna proposta, `summary` che inizia con
  "Answer:" in al massimo 10 righe nella lingua della domanda. `bug-fix` (task `work`): riproduce prima (test che
  fallisce o passi documentati), corregge la causa, la riproduzione resta come test di regressione; se il bug non si
  riproduce `ask_user` o chiusura `failed` con i passi provati; report con causa, correzione e test.
  `code-research` (task `plan`): sola lettura, esperimenti solo con build/test i cui output git ignora, fonti esterne
  citate (solo con la rete), proposta di task solo se richiesta; altrimenti esito "Answer:" con `Question`,
  `Findings`, `Options` (pro, contro, effort relativo), `Recommendation`, `Open points`. `test-qa` (task `work`):
  framework e convenzioni esistenti, nessun framework nuovo senza richiesta, test deterministici; un test che rivela
  un bug non si aggiusta ma va in `follow_ups`. `docs-writer` (task `work`): lettore e lingua dalla specifica
  (default la lingua della richiesta), niente gergo e passi numerati per un lettore non tecnico, ogni comando o
  percorso citato verificato nel repository, codice toccato solo nei commenti. `safe-refactor` (task `work`): test
  verdi prima (scritti prima se mancano), passi piccoli verificati, nessun cambio di comportamento, API o formati;
  report con i passi e la prova dell'equivalenza. `git-ops` (task `work`, regole bloccanti): mai push forzato su
  branch condivisi né storia riscritta di main o del branch base; `reset --hard`, `clean -fd`, `branch -D`,
  `stash drop` ed eliminazione di branch remoti solo dopo un `ask_user` che elenca cosa si perde; prima di ogni
  operazione distruttiva un riferimento `backup/<data>` (tag o branch); git nel worktree del task salvo richiesta;
  mai `stash` sullo stack condiviso senza messaggio unico; un task senza commit propri salta commit, push, PR e
  merge della consegna e chiude `completed` senza `commit` (in modalità PR `gh pr create` fallirebbe); report con
  comandi, stato prima/dopo e come tornare indietro. `release-prep` (task `work`): senza istruzioni di rilascio nel
  repository chiude `failed` chiedendo di documentarle; version bump e changelog dai commit integrati; solo i
  comandi di build e pubblicazione documentati; pubblicazione, tag remoti e deploy solo dopo un `ask_user` con il
  riepilogo.
- **Skill nuove e pacchetto.** La versione del pacchetto di entrambe le integrazioni dipende dal testo di tutte le
  skill (`plugin_version` in `agents/claude_code/plugin.rs`, `version_of` in `agents/opencode/plugin.rs`): una skill
  nuova o cambiata rende la prontezza `outdated` finché l'utente non aggiorna il pacchetto, e solo dopo
  l'aggiornamento l'agente la vede (per OpenCode `SKILL_IDS` nell'`index.ts` generato).

### Bot predefiniti

`PRESETS` in `bots/presets.rs`, 14 voci (12 accese e 2 spente alla creazione, campo `enabled_by_default`), tutte con l'agente del workspace (`agent: None`) e il modello per fascia, mai
per nome: valgono con ogni agente. Il `task` (inglese) è ciò che leggono pianificatore, Intake e segretari per
scegliere il Bot; i testi sono **esclusivi**, così ogni richiesta ha un solo Bot adatto: fra i Bot di uno stesso
`kind` il verbo iniziale non si ripete (test `the_tasks_of_the_bots_of_a_kind_start_with_different_verbs`).

| Id | Nome | `task` | Modello | Effort | Skill | Kinds |
|---|---|---|---|---|---|---|
| `general` | General | General-purpose coding agent: new features and any change no specialized bot covers. | dei worker del workspace | — | — | `[work]` |
| `intake` | Intake | Turns requests into queue tasks. | `strong` | high | `task-intake` | `[]` |
| `setup` | Setup | Configures the workspace and the worker rules. | `standard` | medium | `project-setup` | `[]` |
| `skill-wizard` | Skill Wizard | Creates and manages global skills. | `standard` | medium | `skill-wizard` | `[]` |
| `reviewer` | Reviewer | Verifies the integrated result of finished tasks and proposes fixes. | `strong` | high | — | `[review]` |
| `planner` | Planner | Plans changes to the project: explores the repository read-only and proposes queue tasks. | `strong` | high | — | `[plan]` |
| `quick-answer` | Quick Answer | Answers a quick question about the code or its git history, read-only, in a few lines. Changes nothing and proposes nothing. | `standard` | low | `code-answer` | `[plan]` |
| `bug-fixer` | Bug Fixer | Fixes a reported bug: reproduces it, adds a failing regression test, fixes the root cause and verifies. | `strong` | high | `bug-fix` | `[work]` |
| `researcher` | Researcher | Researches a technical question in depth, read-only: options, feasibility, risks and effort, in a structured report. May propose tasks only if the user asked for them. | `strong` | high | `code-research` | `[plan]` |
| `test-writer` | Test Writer | Adds or improves automated tests for existing behaviour, following the project's test conventions; changes production code only to make it testable. | `standard` | medium | `test-qa` | `[work]` |
| `docs-writer` | Docs Writer | Writes or updates documentation (README, guides, comments) for the stated audience and language; does not change behaviour. | `standard` | medium | `docs-writer` | `[work]` |
| `refactorer` | Refactorer | Restructures existing code without changing its behaviour, in small verified steps, with tests run before and after. | `strong` | high | `safe-refactor` | `[work]` |
| `git-operator` | Git Operator | Performs git maintenance asked by the user (branch cleanup, conflict resolution, reverting a change) safely: never rewrites shared history or force-pushes. | `strong` | medium | `git-ops` | `[work]`, spento |
| `release` | Release | Prepares a release: version bump, changelog and release notes from the merged work; never publishes, tags remotely or deploys without the user's explicit confirmation. | `strong` | high | `release-prep` | `[work]`, spento |

- **Quick Answer** è un Bot `plan` perché il `plan` gira in sola lettura (worktree isolato) e il suo report torna
  intero nel thread dell'HUB: chiude con l'esito "Answer:" (vedi "Task di pianificazione"), senza proposte. Fascia
  `standard` ed effort `low` perché è la richiesta più frequente; su OpenCode l'effort si scarta.
- **Costo di Bug Fixer.** È `strong`/`high`: dove prima un bug finiva su `general` con il modello dei worker del
  workspace, ora il pianificatore sceglie il Bot dedicato e il costo medio di una correzione sale. È voluto (qualità
  della correzione); chi vuole spendere meno ne cambia modello o effort in Impostazioni › Generale › Bot, o lo disattiva.
  Vale lo stesso per Refactorer (`strong`/`high`: una regressione non vista costa più del modello).
- **Researcher e Quick Answer** sono entrambi `plan`: Quick Answer per una domanda puntuale (poche righe, `standard`/`low`),
  Researcher per un'analisi (opzioni, fattibilità, rischi, effort; `strong`/`high`) che chiude con un report "Answer:"
  o, solo se l'utente l'ha chiesto, con una proposta di task come il Planner.
- **Git Operator e Release nascono spenti** (`enabled_by_default: false` in `BotPreset`; file nuovo, migrazione 6→7 e
  «Ripristina i predefiniti mancanti» li creano con `enabled: false`; «Ripristina predefinito» su un Bot esistente non ne cambia
  l'accensione). Perché: sono gli unici Bot con effetti fuori dal flusso di consegna. Git Operator usa comandi git
  che toccano il repository condiviso (eliminare branch, annullare un cambiamento) e spesso chiude senza commit;
  Release prepara versione, changelog e note e, solo dopo un `ask_user` di conferma, pubblica, crea tag remoti o fa il
  deploy: effetti esterni e irreversibili che dipendono dal progetto. Spenti, il segretario non li elenca
  (`hub/engine/bot_choice.rs`) e la board li rifiuta ("task '<id>': Git Operator is disabled: enable it in the bots
  settings"); `get_workspace_settings.bots` li mostra con `enabled: false`, così il pianificatore, invece di proporre,
  chiude con una domanda che nomina il Bot da accendere (`board/prompts/planner_instructions.md`). **Come accenderli:**
  Impostazioni › Generale › Bot, editor del Bot, casella «Attivo»; per Release conviene prima documentare nel repository
  i comandi di rilascio (la skill `release-prep`, senza istruzioni, chiude `failed` chiedendo di scriverle) o dargli
  una skill utente con quei comandi.
- **Migrazioni** (`bots.rs`, passi congelati in `bots/migrations.rs`; `bots.json` alla `schema_version` 7):
  `add_answer_and_fix_bots` (3→4) aggiunge in fondo Quick Answer e Bug Fixer se l'id manca, una volta sola (un Bot
  cancellato non ricompare, un nome già preso fra i Bot diventa "Quick Answer 2");
  `exclusive_general_and_planner_tasks` (4→5) dà a `general` e `planner` il `task` nuovo **solo se è ancora il
  vecchio testo del preset** ("General-purpose coding agent.", "Explores the repository read-only and proposes queue
  tasks."): un testo personalizzato resta; `add_research_and_work_bots` (5→6) aggiunge in fondo Researcher, Test
  Writer, Docs Writer e Refactorer, accesi, con le stesse regole del 3→4; `add_git_and_release_bots` (6→7) aggiunge in
  fondo Git Operator e Release, **spenti**, con le stesse regole (un nome già preso diventa "Git Operator 2"). I passi non leggono i segretari (`bots.json` si carica prima di
  `hub_settings.json`): all'avvio, caricati entrambi, `bots::rename_bots_named_like_secretaries`
  (`bots/secretary_names.rs`) dà il primo nome libero a un Bot che porta il nome di un segretario e salva solo se ne
  ha rinominato uno (un segretario `researcher` fa diventare il Bot "Researcher 2").
- **Esempi non predefiniti** (idee da creare in Impostazioni › Generale › Bot; l'app non li crea):
  - *Security Reviewer*: `kinds` `[review]`, fascia `strong`, effort `high`, una skill utente con la checklist di
    sicurezza dell'organizzazione, per una seconda review indipendente. Non è predefinito perché raddoppierebbe il
    costo di ogni review.
  - *Bot su un altro agente*: per esempio un Reviewer su OpenCode, per un controllo da un modello diverso. Il
    pianificatore lo prevede già ("a Bot that uses that agent", `board/prompts/planner_instructions.md`) e i Bot
    derivati (`bots/derived.rs`) ne sono un caso.
  - *Bot di stack*: `kinds` `[work]` e una skill utente con le convenzioni del progetto (framework, struttura,
    comandi), creata con Skill Wizard; il `task` dice per quale parte del progetto sceglierlo.

## Fasce orarie (semplificate)

`schedule.json` invariato (`enabled`, `windows`, `keep_awake`). Per workspace
`use_schedule: bool` (`#[serde(default)]`; migrazione: `auto_orchestrate.enabled == true` →
`true`). Comando `set_use_schedule(id, bool) -> Workspace`.
- Gate dello scheduler: con schedule `enabled` e `use_schedule`, fuori fascia non parte
  nessun task (quelli in corso finiscono).
- Impedisci standby (`keep_awake`): il PC resta sveglio finché c'è almeno un task `running`
  in qualunque workspace non archiviato.
- Timer di 30 s come oggi per accorgersi delle aperture di fascia (riavvia lo scheduler) e
  per l'evento `schedule-state`.

## Workspace: migrazioni

- `worktree_setup` (vecchio comando di setup): se presente e `repos["."].setup` è vuoto,
  diventa `repos["."].setup = [cmd]`; il campo sparisce.
- `auto_orchestrate` → `use_schedule` (vedi sopra); il campo sparisce.
- `delivery` assente → default `{ mode: "commit", parallel: false, merge_method: "squash" }`.

## Comandi ed eventi Tauri

Argomenti camelCase lato JS.
- `board_get(workspace_id) -> BoardView`
- `board_set(workspace_id, max_parallel?, paused?) -> BoardView`
- `task_create(workspace_id, spec: { title, task, depends_on?, repo?, agent?, model?, kind?, goal? }) -> BoardView`
  (id generato dal titolo: slug ASCII, max 40, suffisso `-2`, `-3`… se occupato; vuoto →
  `task-<n>`). Manuale, senza proposta. `kind: "plan"` non è ammesso (vedi "Task di pianificazione"). `goal`
  (facoltativo, `NewTaskSpec.goal`) è un goal aperto: il task vi entra al giro corrente (vedi "Goal").
- `goal_close(workspace_id, goal_id) -> BoardView`: "Chiudi goal" (vedi "Goal").
- `goal_add_note(workspace_id, goal_id, text) -> BoardView`: nota dell'utente nel registro delle decisioni.
- `task_update(workspace_id, task_id, patch) -> BoardView` (stesse regole di `update_task`).
- `task_action(workspace_id, task_id, action) -> BoardView`, `action` è un `TaskAction` (vedi "Azioni consentite"; una stringa sconosciuta è rifiutata in deserializzazione) ∈ `retry` (failed →
  queued in fondo) | `mark_done` (failed → done a mano: tiene report e worktree, sblocca i
  dipendenti, non verifica né integra il lavoro) | `stop` (running → failed, chiude il tab) | `delete` | `delete_cascade` | `to_top`
  (queued → primo della coda). `delete_cascade` toglie un queued/failed insieme ai task non
  `done` che ne dipendono, anche indirettamente (i `done` restano e perdono il riferimento): se
  uno dei task da togliere è `running` risponde `cannot delete: <ids> are running, stop them
  first` senza toccare la board; ogni worktree dei task tolti viene ripulito. È solo un'azione
  dell'utente: il tool MCP `delete_task` non la espone.
- `task_move(workspace_id, task_id, before_id: Option<String>) -> BoardView`: riordina un
  `queued` (prima di `before_id`, o in fondo).
- `e2e_run_repo(workspace_id, repo?: String) -> E2eRun`: lancia a mano gli E2E del repository
  (`repo` assente = la root) nella sua cartella; ritorna subito l'esecuzione `running`. Errori
  (italiano): "E2E non configurati per il repository", "esecuzione E2E gia' in corso per questo
  repository", "nessuna porta locale libera per gli E2E: riprova fra un attimo".
- `e2e_last_result(workspace_id, repo?: String) -> E2eRun | null`: ultimo esito manuale del
  repository, solo in memoria.
- `e2e_open_report(workspace_id, task_id?: String, repo?: String)`: apre con l'applicazione predefinita il
  file JUnit dell'ultima esecuzione del task (`task_id`) o, senza, di quella manuale del repository.
  Il percorso lo ricava il backend (worktree del task o cartella del repository + `junit`, per i
  workspace WSL convertito in un percorso UNC `wsl.localhost` della distro), mai il frontend. Errori (italiano): nessuna
  esecuzione, report non piu' presente (cartella ripulita).
- `proposal_decide(workspace_id, proposal_id, accept) -> BoardView`
- `settings_proposal_decide(workspace_id, proposal_id, accept) -> { board, workspace? }`
  (vedi "Proposte di impostazioni").
- `set_delivery`, `set_use_schedule` (workspace), `schedule_get`/`schedule_set` (invariati).
- `workspace_detect_repos(workspace_id)` invariato.
- `automation_tab_closed(tab_id)`: il frontend ha chiuso un tab automatico (utente, app o shell uscita); il
  backend chiude il worker e, se il task è concluso, avvia la pulizia. `automation_sessions() ->
  AutomationTabInfo[]`: i worker con il tab aperto, per ricostruire lo stato dopo un ricaricamento.
  `frontend_ready()`: sessione ripristinata e listener registrati; il timer dell'automazione parte da qui
  (vedi DESIGN.MD, "Avvio"). `set_workspace_archived(workspace_id, archived)` (workspace): archiviato = congelato,
  la board non avvia task e i `running` tornano `queued` (vedi "Ciclo di vita"). `set_workspace_location` (workspace)
  cambia la cartella e si rifiuta finché un task è `running` o ha ancora un worktree (`board::relocation_blockers`):
  il percorso del worktree punta alla vecchia cartella (vedi DESIGN.MD, `workspaces/`).
- Eventi: `board-update` (BoardView), `automation-update` (stato dei tab worker),
  `automation-spawn` (apre il tab del worker: `role` non c'è più, `parent_tab_id` vuoto),
  `schedule-state`, `e2e-update` (`{ workspace_id, repo, run: E2eRun }`: l'esecuzione manuale di
  un repository è partita o conclusa; `repo` = chiave del repository, `.` = la root; le
  esecuzioni dei task passano da `board-update`), `task-notify` (notifica di sistema, vedi sotto). L'evento `user-request` non
  esiste più.
- `task-notify`: `{ workspace_id, task_id, tab_id: string|null, kind: "attention"|"failed"|"question", title,
  body }`; `title` è il titolo del task, `body` il testo italiano ("<titolo> richiede attenzione" /
  "<titolo> è fallito") e `tab_id` il tab del worker se esiste ancora. Il frontend mostra la
  notifica di sistema (come per le altre notifiche, solo con la finestra non in primo piano).
- Agents HUB: ogni revisione della board davvero pubblicata (dopo `board-update`; mai una revisione
  vecchia scartata) passa anche da `Host::board_published(view)`, chiamato da
  `AutomationState::publish` sotto il lock delle pubblicazioni, nell'ordine delle revisioni. `TauriHost`
  la inoltra a `hub::board_published` (nulla con l'HUB spento; acceso, accoda un'istantanea compatta a un
  thread dedicato che scrive gli eventi nei thread fuori da ogni lock della board); l'host finto dei test
  la registra. Board, `BoardView` e `task-notify` non cambiano. Vedi [HUB.MD](HUB.MD), "Eventi della board".
- Attenzione: nessun hook dice che l'utente ha risposto. `pty_write` avvisa il backend quando
  nel terminale di un worker in `needs_input`/`idle` si preme Invio e il worker torna `running`; in
  `awaiting_answer` Invio riprende il worker e chiude la domanda senza testo.
- `task-notify.kind` è `attention`, `failed` o `question` ("<titolo> ha una domanda", quando il worker
  comincia ad aspettare la risposta).

## UI

Riferimento: il mockup. Palette dell'app (`src/index.css`): grigio `muted-foreground` = Da
fare, ambra `status-running` = In corso, arancione `status-attention` + icona triangolo =
Richiede attenzione, verde `status-finished` = Fatto, rosso `destructive` = Fallito, blu
`primary` solo per ciò che l'utente decide: azioni, proposte e domande dei worker ("In attesa di risposta").
Testi in italiano.

### Pannello Task (accanto ai terminali)

- Nella vista di un workspace, a sinistra della griglia dei terminali (sotto la barra dei tab
  dei workspace), largo ~284 px. **Sempre presente**, comprimibile a una barra di 48 px con i
  soli contatori (proposte, in corso, attenzione, in coda, falliti); stato compresso/espanso
  ricordato per workspace (`localStorage`, con try/catch).
- Intestazione: "Task", pausa/riprendi la coda, "Apri Tasks Manager" (Ctrl+Maiusc+B), comprimi.
  Con la coda in pausa: banner "Coda in pausa…"; con la fascia chiusa: "Fuori fascia oraria".
- Sezioni: **Proposte** (card Accetta/Scarta e card "Proposta di impostazioni" Applica/Scarta,
  vedi "Proposte di impostazioni"), **In corso N/M** (card: titolo, modello · tempo
  trascorso oppure "Richiede attenzione"; clic → mette a fuoco il tab del worker nella griglia),
  **In coda N** (numerata, trascinabile per riordinare, "attende X" se aspetta dipendenze, "bloccato da X" se una è fallita; clic
  → dettaglio), **Falliti** (solo se non vuota, con il motivo e Riprova). Piede: "+ Nuovo task" e "Task
  Intake" (apre il primo Bot abilitato con la skill `task-intake`).
- Da vuoto: "Nessun task" + pulsante "Apri Task Intake".
- I tab dei worker restano nella griglia dei terminali come oggi (badge AUTO).
- **Vista Globale**: il pannello raggruppa per workspace (solo quelli con task attivi o
  proposte): intestazione del workspace con riepilogo, card in corso, riga "Prossimo: …".
  Accetta/Scarta e Task Intake solo dentro il workspace.

### Sidebar

Il pulsante "Board" della card del workspace diventa **Tasks**, con i contatori (in corso in
ambra · in coda); apre il Tasks Manager.

### Tasks Manager (ex Board)

Copre l'area dei pannelli del workspace (i terminali restano montati: regola del DOM in
`App.tsx`). Barra: "← Terminali", titolo "Tasks" + "nome workspace · consegna: <modalità>",
selettore viste **Kanban** (default) | **Lista** | **Dipendenze**, ricerca (sempre visibile,
`/` la mette a fuoco), stepper Worker (solo in modalità parallela; altrimenti testo "1 per
repository"), Pausa, "+ Nuovo task", "Task Intake". Le proposte pendenti compaiono in cima.

- **Kanban**: colonne Da fare (ordine di partenza), In corso, Fatto (ultimi 7 giorni + "Vedi
  tutti i fatti nella Lista"), Fallito (solo se non vuota). Card: titolo (2 righe), meta (repo
  se multi-repo, modello, tempo/esito), "attende X" o "bloccato da X", motivo del fallimento, "Richiede attenzione". Drag: riordino in
  Da fare (`task_move`), Fallito → Da fare = Riprova; durante il drag In corso e Fatto si
  attenuano (non accettano drop). Ricerca filtra le card.
- **Lista**: ricerca full-text (titolo, specifica, esito, id), chip di stato a selezione
  multipla con conteggi, filtro repo solo nei multi-repo, "Azzera filtri", stato vuoto.
  Colonne: Stato, Titolo (+ sottotitolo: esito o motivo), Modello, Aggiornato (ordinamento
  default, desc), Durata. Filtri ricordati per workspace. Piede "N di M task".
- **Dipendenze**: il grafo React Flow attuale, ripulito (nodi con i 4 stati).
- **Dettaglio** (pannello laterale destro agganciato, uguale per tutte le viste e aperto anche
  dal pannello Task): stato, id, titolo, azioni per stato (Da fare: Modifica, In cima alla
  coda, Elimina; In corso: Apri terminale, Ferma; Fatto: Apri PR se c'è `pr_url`; Fallito:
  Riprova, Apri terminale se il tab esiste, Elimina). Se il task ha dipendenti non fatti,
  Elimina chiede una conferma in linea che li elenca e il pulsante «Elimina tutti» esegue
  `delete_cascade`; se un dipendente è in esecuzione il pulsante è disattivato e il tooltip
  chiede di fermarlo prima), esito (summary, commit, link PR, test
  con esito), nota, **specifica renderizzata in markdown** (senza dipendenze nuove: un
  renderer minimale per titoli, liste, codice inline/blocchi, grassetto, link), dipendenze,
  modello, repo, durata. Modifica (solo Da fare/Fallito): titolo, testo, dipendenze, modello.
- **Nuovo task** (dialog): titolo, specifica (textarea), dipendenze (multi-selezione tra i task
  non fatti), modello (default del workspace), repo (solo multi-repo). Suggerimento: "Per una
  specifica completa usa Task Intake".

### Impostazioni

- Workspace → **Consegna** (nuova sezione nella pagina Worker o pagina propria): modalità
  (4 opzioni con una riga di spiegazione ciascuna), "Worker in parallelo (worktree)" solo per
  PR/merge locale, metodo di merge solo per PR, avviso prerequisiti (`gh` autenticato per PR;
  per merge locale il checkout deve stare sulla base).
- Generale → **Integrazioni**: una scheda per integrazione registrata (abilitata, predefinita, funzioni
  dichiarate, stato e azioni per ogni ambiente). Disabilitare chiede conferma elencando i terminali aperti e i
  task in coda che usano quell'agente.
- Workspace → **Worker**: "Agente dei worker" (Eredita il predefinito globale, o un'integrazione abilitata con
  l'automazione; avviso se l'agente salvato non è più utilizzabile), poi, per l'agente scelto, "Modello dei
  worker" (suggerimenti dal catalogo dell'agente) e le regole allow/deny nella sua sintassi
  (`worker_agents`). Comandi Tauri: `set_workspace_agent`, `set_worker_model`, `set_worker_permission_rules`.
- Task (form di creazione e modifica, pannello e Tasks Manager): campo **Agente** con gli agenti abilitati che
  dichiarano l'automazione, preselezionato quello del workspace; i suggerimenti del modello seguono l'agente e un
  modello non consigliato dal nuovo agente si svuota. Il badge dell'agente compare su card Kanban, righe,
  card in corso, dettaglio e proposte (sempre sulle proposte; altrove solo con più integrazioni abilitate o se
  l'agente differisce da quello del workspace); la lista si filtra per agente (filtro ricordato per workspace).
  Un task in coda con `agent_unavailable` mostra “In attesa: <agente> non è pronto in <ambiente>” (o “è
  disabilitato”) con **Cambia agente** e **Apri Integrazioni**.
- Workspace → **Repository**, per ogni repository: sezione "Test E2E" (comando, percorso JUnit,
  timeout in minuti; tutto vuoto = nessun E2E) con un testo di aiuto: gli E2E sono facoltativi,
  l'app esegue il comando nella cartella del task e legge il JUnit, la porta del server di
  sviluppo arriva in `CLI_COMPANION_E2E_PORT`, il blocco vale solo per i task marcati E2E. Nelle
  proposte di impostazioni la card mostra `e2e` prima e dopo, come campo sensibile.
- Workspace → "Rispetta le fasce orarie" (toggle `use_schedule`); Generale → Fasce orarie
  invariata (senza riferimenti all'orchestratore).
- Rimosse: pagina Orchestratore, pagina Workflow, campo "comando di setup del worktree".

## UI delle review

I task `review` si riconoscono dal badge **Review** in Kanban, Lista, coda e card in corso del
pannello, nodo del grafo, dettaglio e righe di una proposta. Una review `done` mostra il verdetto:
**Verificato** (`passed`) oppure **Problemi trovati** (`issues`, colore di avviso: il task non è
fallito); con `issues` il dettaglio elenca i `findings` sopra il sommario. Una proposta con
`source_task` si intitola "Proposta dal reviewer" e mostra il task di origine (titolo, o id se non
è più nella board); senza, resta "Proposta da Task Intake". Il form "Nuovo task" e la modifica
(solo dove il task è modificabile) hanno l'opzione "Task di revisione" (`kind: "review"`): senza
almeno una dipendenza il form blocca il salvataggio, come il backend. Logica in `src/lib/review.ts`.

## UI degli E2E

Logica pura in `src/lib/e2e.ts`: nel frontend non c'è nessuna logica di esito, le etichette
traducono solo lo stato che arriva dal backend (`E2eRun.status`, `totals`).

- **Flag nei form.** "Nuovo task" e modifica hanno la casella "Richiede E2E" (`e2e`). È attiva solo
  se il repository scelto ha `e2e` configurato (`Workspace.repos`); altrimenti è disattivata e
  spiega di configurarli in Impostazioni → Repository. Cambiare repository verso uno senza E2E
  spegne il flag; un flag già acceso resta visibile e si può togliere anche se la configurazione è sparita.
- **Badge** (stile dei badge delle review): "E2E richiesto" (task marcato, mai eseguito), poi l'esito
  dell'ultima esecuzione (`Task.e2e_run`, altrimenti `report.e2e`): "E2E 12/12" verde, "E2E 2 falliti"
  rosso, "E2E in corso", "E2E errore" rosso. Su card del Kanban, righe della Lista, card in corso del
  pannello e dettaglio; le specifiche di una proposta mostrano solo "E2E richiesto".
- **Dettaglio del task.** Sezione "Test E2E (verificati dall'app)": esito, totali, durata, commit,
  test falliti con il messaggio, errore e coda dell'output; "Apri report" (`e2e_open_report`)
  apre il JUnit dell'esecuzione, disattivato finché è in corso.
- **Pagina Repository.** Per i repository con `e2e` configurato "Esegui E2E" (`e2e_run_repo`) con
  stato in corso, ultimo risultato, test falliti e "Apri report"; si aggiorna con l'evento `e2e-update`.
