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
- Richieste: `ask_user`, `propose_settings`, casella Richieste (`RequestsPanel`), comandi
  `answer_user_request`, `decide_user_request`, `dismiss_user_request`.
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
  "diffstat": "..."         // calcolato dall'app quando c'è un worktree
}
```

Proposal:

```jsonc
{
  "id": "p-<millis>-<n>",
  "source_tab": "tab-...",  // tab dell'Intake che l'ha proposta
  "created_at": 0,
  "tasks": [ /* TaskSpec: id, title, task, depends_on, repo, model */ ]
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

Al retry di un task fallito `get_task` aggiunge `previous_attempt` (summary del report e
nota), così il worker riparte da dove si era fermato (stesso branch/worktree).

**Standard di ingegneria.** Ogni worker riceve, dopo le istruzioni comuni e prima delle regole
del workspace, lo standard di `src-tauri/src/automation/worker_guardrails.md` (incluso con
`include_str!`, in inglese): rispetto del progetto e dei pattern del framework, SOLID e codice
pulito, niente file "god", sicurezza come bloccante (non un follow-up), prestazioni per
progettazione, errori e casi limite, test come parte della modifica, rigore sul perimetro e
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
```

- Al riavvio dell'app i `running` diventano `failed` (`interrupted: the app was restarted`).
- `session-end` conta solo se viene dalla sessione dell'agente legata al worker (eventi di
  processi secondari ignorati, con una traccia nella nota). Se fallisce comunque il task mentre
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

Il server MCP sceglie il ruolo con la RPC interna `whoami` (non è un tool) ed espone tool diversi in base al tab: **worker** (tab registrato come worker di
un task) oppure **sessione** (qualunque altro tab di cli-companion: Intake o terminale
normale). Fuori da cli-companion nessun tool. Descrizioni ed errori in inglese.
Le istruzioni del server MCP (inglese) dicono: you may propose tasks for this workspace's
queue with propose_tasks; the user approves them in the app; never wait for workers.

Worker:
- `get_task` → `{ task, title, instructions, environment, delivery, previous_attempt? }`.
- `complete_task { status: "completed" | "failed", summary, commit?, pr_url?,
  files_changed?, tests?, follow_ups? }` — `blocked` accettato come sinonimo di `failed`.
  Per i task con worktree l'app calcola il diffstat. Il tab si chiude dopo ~1,5 s.

Sessione (workspace = quello del tab):
- `propose_tasks { tasks: TaskSpec[] }` — `TaskSpec = { id, title, task, model,
  depends_on?, repo? }` (`model` obbligatorio nelle proposte). Valida tutto (id unici anche rispetto ai task esistenti e alle altre
  proposte, dipendenze verso la proposta stessa o task esistenti, niente cicli, `repo`
  esistente e git se la modalità usa branch). Crea una **proposta** pendente e ritorna
  `{ proposal, status: "pending" }`: "the user will accept or discard it in the app". Non
  aspetta la decisione.
- `list_tasks { status?: string[] }` → righe compatte `{ id, title, status, attention?,
  waiting_on?, summary? (300), note? (200) }` + `counts` + `proposals_pending`.
- `get_task_details { id }` → task completo (testo, report, worktree).
- `update_task { id, title?, task?, depends_on?, repo?, model? }` — solo `queued` o
  `failed`; `repo` solo senza worktree. Ritorna il task senza testo.
- `delete_task { id }` — solo `queued`/`failed` e senza dipendenti non `done`.

## Proposte (approvazione)

- `propose_tasks` aggiunge una `Proposal` alla board → `board-update`.
- UI: card "Proposta da Task Intake" in cima al pannello Task del workspace (e nel Tasks
  Manager) con titolo/i, estratto della specifica, dipendenze, modello e **Scarta** /
  **Accetta**. Nessuna modifica: per cambiarla l'utente la chiede all'Intake.
- Accetta → i task entrano in coda (`queued`, in fondo, nell'ordine della proposta) dopo una
  nuova validazione; se fallisce (es. un id nel frattempo occupato) la card mostra l'errore.
- Scarta → la proposta sparisce. Nessun feedback all'Intake.
- Comando Tauri `proposal_decide(workspace_id, proposal_id, accept: bool) -> BoardView`.

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
  bastare a un worker che non vede la conversazione.
- Il pannello dei terminali mostra il badge **INTAKE** sull'intestazione dei tab avviati dal
  preset.

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
- `task_create(workspace_id, spec: { title, task, depends_on?, repo?, model? }) -> BoardView`
  (id generato dal titolo: slug ASCII, max 40, suffisso `-2`, `-3`… se occupato; vuoto →
  `task-<n>`). Manuale, senza proposta.
- `task_update(workspace_id, task_id, patch) -> BoardView` (stesse regole di `update_task`).
- `task_action(workspace_id, task_id, action) -> BoardView`, `action` ∈ `retry` (failed →
  queued in fondo) | `stop` (running → failed, chiude il tab) | `delete` | `to_top`
  (queued → primo della coda).
- `task_move(workspace_id, task_id, before_id: Option<String>) -> BoardView`: riordina un
  `queued` (prima di `before_id`, o in fondo).
- `proposal_decide(workspace_id, proposal_id, accept) -> BoardView`
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
- Sezioni: **Proposte** (card Accetta/Scarta), **In corso N/M** (card: titolo, modello · tempo
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
  Riprova, Apri terminale se il tab esiste, Elimina), esito (summary, commit, link PR, test
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
