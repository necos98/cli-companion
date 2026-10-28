# Tasks Manager (v0.12)

Ogni workspace ha una **coda di task**. Non c'è più un orchestratore: ci sono tre
attori che non parlano tra loro.

| Attore | Cosa fa |
|---|---|
| **Task Intake** (preset, LLM) | Ci parla l'utente. Esplora il repository, scrive una specifica completa e la **propone** (`propose_tasks`). L'utente accetta o scarta dalla UI. Non riceve eventi e non viene mai svegliato. |
| **App** | Scheduler deterministico: avvia i task "Da fare" in ordine, rispetta dipendenze, slot, fasce orarie e modalità di consegna; prepara e pulisce i worktree; raccoglie l'esito. Non decide mai niente. |
| **Worker** (LLM) | `get_task`, lavora, **consegna da solo** secondo la modalità del workspace (commit, push, PR, merge), poi `complete_task`. Se ha bisogno dell'utente chiede nel suo terminale. |

Principio: semplicità. Niente owner, niente coda di eventi, niente risveglio,
niente integrazione esterna. Un task finito è già consegnato.

Riferimento visivo approvato (mockup): artifact "Tasks Manager mockup"; sorgenti in
`C:\Users\jacob\AppData\Local\Temp\claude\F--altre-robe-muse-companion\0c0ba9fb-b340-42e3-819d-2ea51b5260b5\scratchpad\mockup\project\`
(`Main.dc.html` terminali + pannello, `Kanban.dc.html`, `Lista.dc.html`, `Globale.dc.html`).

Lingua: tutto ciò che legge il modello (skill dei preset, descrizioni dei tool MCP,
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

File invariato: `<app_data>/boards/<workspace_id>.json`, scrittura atomica, salvataggio
coalescente (max 1/s per board) come oggi. Serde `snake_case`; i campi sconosciuti dei file
vecchi si ignorano (niente `deny_unknown_fields`).

```jsonc
{
  "workspace_id": "ws-...",
  "max_parallel": 3,      // 1..=16: worker in parallelo (vale solo in modalità parallela)
  "paused": false,        // coda in pausa: nessun nuovo avvio
  "updated_at": 0,
  "tasks": [ /* Task, l'ordine dell'array È l'ordine della coda */ ],
  "proposals": [ /* Proposal pendenti */ ]
}
```

Task:

```jsonc
{
  "id": "delivery-modes",   // ^[a-z0-9][a-z0-9-]{0,39}$, unico nella board
  "title": "Modalità di consegna",       // 1..=80
  "task": "...",            // specifica per il worker, 1..=100000
  "depends_on": ["task-schema"],         // id esistenti, niente cicli, max 50
  "kind": "work",           // work | review (default work, vedi "Task di review"); una review richiede almeno una dipendenza
  "repo": null,             // cartella relativa alla root; null = la root (multi-repo)
  "agent": "claude-code",
  "model": null,            // null = modello dei worker del workspace
  "status": "queued",       // queued | running | done | failed
  "worker_status": null,    // starting | running | background | needs_input | idle | ... (come oggi)
  "session": null,          // tab del worker corrente
  "attempts": 0,
  "previous_attempt": null, // { summary?, note? } del tentativo fallito, dopo "Riprova"; lo legge get_task
  "worktree": null,         // { path, branch, base, setup_done } solo in modalità parallela
  "last_report": null,      // ultimo report breve (200)
  "note": null,             // nota dell'app: errore di avvio, "interrupted: ...", "stopped by the user", ...
  "report": null,           // TaskReport
  "created_at": 0, "updated_at": 0, "started_at": null, "finished_at": null
}
```

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
  "findings": []            // solo review: problemi (con issues) o suggerimenti (con passed); omesso se vuoto
}
```

Proposal:

```jsonc
{
  "id": "p-<millis>-<n>",
  "source_tab": "tab-...",  // tab dell'Intake che l'ha proposta
  "created_at": 0,
  "source_task": "review-x", // opzionale: task di review il cui reviewer l'ha proposta (omesso se assente);
                             // in quel caso source_tab è il tab del reviewer
  "tasks": [ /* TaskSpec: id, title, task, depends_on, repo, model, kind */ ]
}
```

Limiti: 200 task per board, 20 task per proposta, 20 proposte pendenti per board.

### Stati e UI

| status | UI | note |
|---|---|---|
| `queued` | **Da fare** | parte quando: coda non in pausa, fascia aperta (se il workspace la usa), tutte le dipendenze `done`, slot libero. Con dipendenze non `done` la vista espone `waiting_on`. |
| `running` | **In corso** | con `worker_status` `needs_input` o `idle` la vista espone `attention: true` → badge "Richiede attenzione". |
| `done` | **Fatto** | consegnato. Terminale. Sblocca i dipendenti. |
| `failed` | **Fallito** | `note`/`report.summary` dicono perché. "Riprova" lo rimette in coda. |

`BoardView` = board + per task: `waiting_on: string[]` (dipendenze non `done`, solo per
`queued`), `attention: bool`; + `rev` monotona (come oggi), `schedule_blocked: bool` (fascia
chiusa per un workspace che usa le fasce), `delivery` del workspace (comodità per la UI).

### Normalizzazione al caricamento (board v0.11)

Stesso file, letto così com'è. Al caricamento:

- `queued` (anche `held`) → `queued`;
- `running` → `failed` con nota `interrupted: the app was restarted` (come oggi);
- `review` → `failed` con nota `completed but not integrated (board v0.11): retry to let the worker deliver it`;
- `blocked`, `failed` → `failed`;
- `cancelled` → rimosso (e tolto dai `depends_on` degli altri);
- `done` (anche `external`) → `done`;
- `report.status` `blocked` → `failed`;
- `kind` assente (task e `TaskSpec` delle proposte) → `work`; `source_task`, `verdict` e `findings` assenti → nessuno;
- i campi rimossi si ignorano; `requests`, `owner`, `integrations` & co. spariscono al prossimo salvataggio.

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
- Le **review** (`kind: "review"`) ricevono il worktree dalla base come ogni task (`task/<id>`);
  la pulizia a `done` funziona anche senza commit sul branch (worktree rimosso, branch cancellato).
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

### Modalità in place

Nessun worktree: il worker lavora nel checkout del repo. Prima dell'avvio l'app non modifica
nulla (niente fetch, niente worktree); nelle modalità con branch legge soltanto la base
(`base_branch` del repo, default di `origin` per `pr`, branch corrente) per scriverla nelle
istruzioni. In `pr`/`local_merge` seriali il worker crea il branch
`task/<id>` da `<base>`, consegna, torna su `<base>`, aggiorna e cancella il branch locale.

### Istruzioni di consegna (testo in inglese, generato dall'app)

`get_task` restituisce `instructions` (comuni) + `environment` (cartella, branch, base,
modalità) + `delivery` (i passi della modalità). Contenuto minimo per modalità:

- **Comuni**: work only inside `<dir>`; never touch other folders; commit only files you
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
fa merge): restituisce le istruzioni di `src-tauri/src/automation/reviewer_instructions.md`
(incluso con `include_str!`, in inglese), più `environment` normale e le regole del workspace
accodate come al solito. Vedi "Task di review".

Al retry di un task fallito `get_task` aggiunge `previous_attempt` (summary del report e
nota), così il worker riparte da dove si era fermato (stesso branch/worktree).

**Standard di ingegneria.** Ogni worker riceve, dopo le istruzioni comuni e prima delle regole
del workspace, lo standard di `src-tauri/src/automation/worker_guardrails.md` (incluso con
`include_str!`, in inglese): rispetto del progetto e dei pattern del framework, SOLID e codice
pulito, niente file "god", sicurezza come bloccante (non un follow-up), prestazioni per
progettazione, errori e casi limite, test come parte della modifica, **scenari E2E** (se la specifica ha una sezione E2E scenarios ogni scenario diventa un test della suite E2E del progetto, con locator rivolti all'utente, esiti osservabili, niente attese fisse e test indipendenti; la suite si esegue prima della consegna), rigore sul perimetro e
**autorevisione severa** del proprio diff prima di consegnare. Le regole del workspace aggiungono
specifiche del progetto e non allentano mai le regole di sicurezza.

## Scheduler

Funzione pura dopo ogni modifica (come oggi):

1. Niente se la board è `paused` o se il workspace usa le fasce (`use_schedule`) e la
   fascia è chiusa.
2. Candidati: `queued` con tutte le dipendenze `done`, nell'ordine dell'array.
3. Slot: in modalità parallela (`pr`/`local_merge` con `parallel`) fino a `max_parallel`
   running nella board; altrimenti **1 running per repository** (chiave `repo`), comunque
   entro `max_parallel`.
4. Ogni scelto passa a `running` (`worker_status = starting`, `attempts += 1`,
   `started_at`), l'avvio vero (git + spawn) gira fuori dal lock su un thread separato.

Nessun evento `plan_completed`: la UI mostra lo stato.

## Ciclo di vita

```text
queued --scheduler--> running --complete_task completed--> done (pulizia worktree)
                         |--complete_task failed----------> failed
                         |--tab chiuso / agente uscito senza report--> failed (note)
                         |--avvio fallito (git, setup, precondizione)--> failed (note)
                         \--utente: Ferma--> failed (note "stopped by the user", tab chiuso)
failed (session-end, tab aperto) --complete_task dello stesso worker--> done | failed (note "reopened: ...")
failed --utente: Riprova--> queued (in fondo alla coda; worktree e branch si riusano)
queued|failed --utente/Intake: Elimina--> rimosso (solo senza dipendenti non done)
queued|failed --utente: Elimina tutti--> rimosso insieme ai dipendenti non done, diretti e indiretti
                                         (tutto o niente: se uno è running, errore e nulla cambia)
```

Una review (`kind: "review"`) segue lo stesso ciclo: `complete_task completed` la porta sempre a
`done`, **anche con `verdict: "issues"`** (il verdetto sta nel report); una review `failed` resta
`failed`.

- Al riavvio dell'app i `running` diventano `failed` (`interrupted: the app was restarted`).
- `session-end` conta solo se viene dalla sessione dell'agente legata al worker (eventi di
  processi secondari, o senza id dopo il legame, ignorati con una traccia nella nota; senza
  legame un evento senza id vale come prima). Se fallisce comunque il task mentre
  il tab del worker e' aperto, il `complete_task` di quel worker lo riapre e lo conclude con il
  report, purche' nessun retry l'abbia ripreso (dettagli in DESIGN.MD, `automation.rs`).
- Stop hook del worker: se chiude il turno senza `complete_task` e senza lavoro in
  background, la prima volta l'app risponde con un blocco e il promemoria (inglese): "If the
  task is done, deliver it and call complete_task. If you are waiting for the user's
  decision, end your turn again: the user will answer here." Alla seconda il worker diventa
  `idle` → `attention`. `needs_input` (permesso in attesa) → `attention`.
- Notifiche di sistema (in italiano): quando un task entra in `attention` ("<titolo>
  richiede attenzione") o in `failed` ("<titolo> è fallito"). Niente notifica per `done`.

## Tool MCP

Il server MCP sceglie il ruolo con la RPC interna `whoami` (non è un tool) ed espone tool diversi in base al tab: **worker** (`role: "worker"`, tab registrato come worker di
un task `work`), **reviewer** (`role: "reviewer"`, worker di un task `review`) oppure **sessione**
(`role: "session"`, qualunque altro tab di cli-companion: Intake o terminale
normale). Fuori da cli-companion nessun tool. Descrizioni ed errori in inglese.
Le istruzioni del server MCP (inglese) dicono: you may propose tasks for this workspace's
queue with propose_tasks and read and propose changes to the workspace settings with
get_workspace_settings and propose_settings; the user approves them in the app; never wait for workers.

Worker:
- `get_task` → `{ task, title, instructions, environment, delivery, previous_attempt? }`.
- `complete_task { status: "completed" | "failed", summary, commit?, pr_url?,
  files_changed?, tests?, follow_ups? }` — `blocked` accettato come sinonimo di `failed`.
  Per i task con worktree l'app calcola il diffstat. Il tab si chiude dopo ~1,5 s.
  `verdict` e `findings` sono un errore su un task `work`.

Reviewer (worker di un task `review`; mai `update_task`, `delete_task`, `list_tasks`, tool di
impostazioni o di skill; tutti gli altri metodi di sessione restano vietati):
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
  consente solo ai worker dei task `review`. I worker normali non vedono niente di nuovo.

Istruzioni del server MCP per il reviewer (inglese): automated reviewer; call get_task first;
verify the integrated result without changing files; propose fixes with propose_tasks; always
finish with complete_task and a verdict.

Sessione (workspace = quello del tab):
- `propose_tasks { tasks: TaskSpec[] }` — `TaskSpec = { id, title, task, model,
  depends_on?, repo?, kind? }` (`model` obbligatorio nelle proposte; `kind` `work` | `review`,
  default `work`; una `review` richiede almeno una dipendenza, controllata a proposta,
  accettazione, creazione manuale e aggiornamento: "task '<id>': a review task needs at least one
  dependency (depends_on: the tasks it reviews)"). Valida tutto (id unici anche rispetto ai task esistenti e alle altre
  proposte, dipendenze verso la proposta stessa o task esistenti, niente cicli, `repo`
  esistente e git se la modalità usa branch). Crea una **proposta** pendente e ritorna
  `{ proposal, status: "pending" }`: "the user will accept or discard it in the app". Non
  aspetta la decisione.
- `list_tasks { status?: string[] }` → righe compatte `{ id, title, status, kind? (solo
  `review`), verdict? (se il report ne ha uno), attention?,
  waiting_on?, summary? (300), note? (200) }` + `counts` + `proposals_pending`.
- `get_task_details { id }` → task completo (testo, report, worktree).
- `update_task { id, title?, task?, depends_on?, repo?, model?, kind? }` — solo `queued` o
  `failed`; `repo` solo senza worktree. Ritorna il task senza testo.
- `delete_task { id }` — solo `queued`/`failed` e senza dipendenti non `done`.
- `get_workspace_settings {}` → le impostazioni che `propose_settings` può cambiare, i
  repository con la loro configurazione (`null` = nessuna), i valori ammessi, i limiti e le
  proposte di impostazioni pendenti.
- `propose_settings { reason, worker_rules?, worker_model?, worker_permission?, worker_allow?,
  worker_deny?, delivery?, use_schedule?, default_launch?, launches?, repos? }` — cambiano solo i
  campi passati; liste e `worker_rules` sostituiscono quelle attuali; `delivery` accetta anche
  solo alcuni dei suoi campi (`mode`, `parallel`, `merge_method`); `repos` ha per chiave la
  cartella del repository (`.` = root) e per valore `{ base_branch?, container_root?,
  copy_ignored?, setup?, cleanup?, remove_force? }` oppure `null` per toglierne la
  configurazione. Validazione tutto-o-niente; ritorna `{ proposal, status: "pending" }` e non
  aspetta la decisione (vedi "Proposte di impostazioni").

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
  l'obiettivo end to end (compresa l'integrazione tra i task). Segnala solo difetti reali;
  i miglioramenti opzionali vanno in `findings` come suggerimenti.
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

## Proposte (approvazione)

- `propose_tasks` aggiunge una `Proposal` alla board → `board-update`.
- UI: card "Proposta da Task Intake" in cima al pannello Task del workspace (e nel Tasks
  Manager) con titolo/i, estratto della specifica, dipendenze, modello e **Scarta** /
  **Accetta**. Nessuna modifica: per cambiarla l'utente la chiede all'Intake.
- Accetta → i task entrano in coda (`queued`, in fondo, nell'ordine della proposta) dopo una
  nuova validazione; se fallisce (es. un id nel frattempo occupato) la card mostra l'errore.
- Scarta → la proposta sparisce. Nessun feedback all'Intake.
- Una proposta fatta dal reviewer di una review registra `source_task` (id della review) e
  `source_tab` (tab del reviewer); le proposte dell'Intake non hanno `source_task`.
- Comando Tauri `proposal_decide(workspace_id, proposal_id, accept: bool) -> BoardView`.

### Proposte di impostazioni

- `propose_settings` aggiunge una `SettingsProposal { id, source_tab, created_at, reason, patch }`
  alla board (campo `settings_proposals`, tetto separato da quello delle proposte di task) →
  `board-update`. La vista espone anche i valori **attuali** dei soli campi toccati e l'elenco dei
  campi **sensibili**.
- Campi sensibili (eseguono comandi o allargano i permessi): `worker_permission`, `worker_allow`,
  `worker_deny`, `launches`, `default_launch`, `repos[<repo>].setup`, `repos[<repo>].cleanup`.
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

## Preset di agenti

Registro nell'app (`agents`): `Preset { id, name, skill }` con lo SKILL.md (inglese)
incluso con `include_str!`. Il plugin di Claude Code installato dall'app contiene
`skills/<id>/SKILL.md` per ogni preset (e non più `orchestrate`/`workflow`: l'installazione
rimuove le cartelle obsolete). Ogni preset diventa una **voce rapida** integrata
`builtin:preset:<id>` con comando `claude --dangerously-skip-permissions
"/cli-companion-notify:<id>"` e `restore_command` = il comando interattivo (ripristino
della conversazione), come faceva l'orchestratore.
- Un `default_launch` salvato come `builtin:orchestrator` diventa `builtin:preset:task-intake`.
- Primo preset: **`task-intake`**, nome "Task Intake". Skill (inglese): accogli la richiesta;
  fai domande solo se servono; esplora il repository (file, convenzioni, test, comandi di
  verifica, CLAUDE.md); se il lavoro è grande dividilo in pochi task con dipendenze;
  mostra all'utente in chat ogni task (titolo + specifica) prima di proporlo; poi
  `propose_tasks`; spiega che l'approvazione avviene nel pannello Task. Modello della
  specifica: **Goal, Context, Constraints, Done when, Verify** (comandi). Non implementare
  mai il lavoro, non toccare file, non fare merge; può usare `list_tasks`,
  `get_task_details`, `update_task`, `delete_task` per lo stato della coda. Ogni task ha
  sempre `model` (sonnet per lavoro ben definito, opus per design/rischio). La specifica deve
  bastare a un worker che non vede la conversazione. Quando la richiesta produce 2 o più task
  (o un solo task rischioso o la correzione di un bug difficile) aggiunge in fondo alla proposta
  una **review** del goal (`kind: "review"`, `model: "opus"`, `depends_on` su tutti i task del
  goal); l'utente può scartarla.
- Il pannello dei terminali mostra il badge **INTAKE** sull'intestazione dei tab avviati dal
  preset.
- Task Intake sa dei tool di impostazioni: se la richiesta riguarda la configurazione del
  workspace e non il lavoro sul codice, non crea un task ma usa `get_workspace_settings` /
  `propose_settings`, oppure suggerisce Project Setup.
- **Scenari E2E (opzionali)**: se il progetto ha gia una suite E2E (`playwright.config.*`, `cypress.config.*`, cartelle `e2e/` o `tests/e2e/`, script `test:e2e`, pytest-playwright), per i task che cambiano un flusso visibile la specifica contiene una sezione **E2E scenarios**: elenco numerato punto di partenza -> azioni in parole semplici -> esito osservabile (con il caso negativo se conta), mai selettori, ripetuti in "Done when". I task senza flussi visibili non hanno scenari. Senza suite l'Intake non scrive scenari e non propone di configurarla. Solo su richiesta esplicita dell'utente il task di configurazione prevede Playwright (salvo un framework gia in uso), `webServer` sulla porta di `CLI_COMPANION_E2E_PORT` con `baseURL` coerente, reporter `junit` su `e2e-results/junit.xml` e `html` (`open: "never"`), un test di base e i browser documentati nel README.
- **`project-setup`**, nome "Project Setup" (voce rapida `builtin:preset:project-setup`, skill
  `/cli-companion-notify:project-setup`). L'agente con cui l'utente configura il workspace e il
  modo in cui ci lavorano i worker. Skill (inglese): solo proposte, nessuna modifica diretta di
  file o impostazioni, nessun git che cambia stato; legge prima `get_workspace_settings`;
  esplora in sola lettura il repository e i sotto-repository per ricavare la configurazione dei
  worktree (file ignorati ma necessari → `copy_ignored`; docker-compose/Dockerfile → servizio e
  `container_root`; package manager → comandi `setup` dopo la creazione e `cleanup` prima della
  rimozione; branch di default → `base_branch`); comandi di setup ripetibili (al retry si
  rifanno) e su una riga sola; una domanda alla volta e solo se cambia la configurazione;
  propone solo i campi da cambiare; mostra in chat il prima → dopo e il perché; poi
  `propose_settings` con un `reason` chiaro (correggendo e richiamando in caso di errore di
  validazione) e chiude il turno spiegando che la proposta attende nel pannello Task, senza
  attendere né fare polling. Campi sensibili (permessi, allow/deny, avvii, `setup`/`cleanup`):
  solo se servono o se l'utente li chiede, con il rischio spiegato, mai `bypass` senza una
  richiesta esplicita. Spiega le modalità di consegna e che la configurazione dei worktree vale
  solo con le modalità parallele. Parla con l'utente nella sua lingua; regole e testi per i
  worker in inglese, salvo diversa richiesta.

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
- `task_create(workspace_id, spec: { title, task, depends_on?, repo?, model?, kind? }) -> BoardView`
  (id generato dal titolo: slug ASCII, max 40, suffisso `-2`, `-3`… se occupato; vuoto →
  `task-<n>`). Manuale, senza proposta.
- `task_update(workspace_id, task_id, patch) -> BoardView` (stesse regole di `update_task`).
- `task_action(workspace_id, task_id, action) -> BoardView`, `action` ∈ `retry` (failed →
  queued in fondo) | `mark_done` (failed → done a mano: tiene report e worktree, sblocca i
  dipendenti, non verifica né integra il lavoro) | `stop` (running → failed, chiude il tab) | `delete` | `delete_cascade` | `to_top`
  (queued → primo della coda). `delete_cascade` toglie un queued/failed insieme ai task non
  `done` che ne dipendono, anche indirettamente (i `done` restano e perdono il riferimento): se
  uno dei task da togliere è `running` risponde `cannot delete: <ids> are running, stop them
  first` senza toccare la board; ogni worktree dei task tolti viene ripulito. È solo un'azione
  dell'utente: il tool MCP `delete_task` non la espone.
- `task_move(workspace_id, task_id, before_id: Option<String>) -> BoardView`: riordina un
  `queued` (prima di `before_id`, o in fondo).
- `proposal_decide(workspace_id, proposal_id, accept) -> BoardView`
- `settings_proposal_decide(workspace_id, proposal_id, accept) -> { board, workspace? }`
  (vedi "Proposte di impostazioni").
- `set_delivery`, `set_use_schedule` (workspace), `schedule_get`/`schedule_set` (invariati).
- `workspace_detect_repos(workspace_id)` invariato.
- Eventi: `board-update` (BoardView), `automation-update` (stato dei tab worker),
  `automation-spawn` (apre il tab del worker: `role` non c'è più, `parent_tab_id` vuoto),
  `schedule-state`, `task-notify` (notifica di sistema, vedi sotto). L'evento `user-request` non
  esiste più.
- `task-notify`: `{ workspace_id, task_id, tab_id: string|null, kind: "attention"|"failed", title,
  body }`; `title` è il titolo del task, `body` il testo italiano ("<titolo> richiede attenzione" /
  "<titolo> è fallito") e `tab_id` il tab del worker se esiste ancora. Il frontend mostra la
  notifica di sistema (come per le altre notifiche, solo con la finestra non in primo piano).
- Attenzione: nessun hook dice che l'utente ha risposto. `pty_write` avvisa il backend quando
  nel terminale di un worker in `needs_input`/`idle` si preme Invio e il worker torna `running`.

## UI

Riferimento: il mockup. Palette dell'app (`src/index.css`): grigio `muted-foreground` = Da
fare, ambra `status-running` = In corso, arancione `status-attention` + icona triangolo =
Richiede attenzione, verde `status-finished` = Fatto, rosso `destructive` = Fallito, blu
`primary` solo per azioni e proposte. Testi in italiano.

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
  **In coda N** (numerata, trascinabile per riordinare, "attende X" se aspetta dipendenze; clic
  → dettaglio), **Falliti** (solo se non vuota, con Riprova). Piede: "+ Nuovo task" e "Task
  Intake" (apre la voce rapida del preset nel workspace).
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
  se multi-repo, modello, tempo/esito), "attende X", "Richiede attenzione". Drag: riordino in
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
