# Board dei task (v0.7)

Ogni workspace ha una **board**: un piano di task con dipendenze, salvato su
disco e eseguito da uno **scheduler** dell'app. L'orchestratore (LLM) fa il
project manager: crea i task, dichiara dipendenze e aree toccate, integra il
lavoro (merge, test di regressione, manutenzione del repo) e segna i task come
fatti. L'app fa lo scheduler: avvia i worker quando un task è pronto, crea e
pulisce i worktree git, raccoglie i report strutturati e mostra tutto in una
vista a grafo (React Flow) per workspace.

Il flusso ad-hoc (`spawn_session`) resta invariato e convive con la board.

## Principio

L'LLM decide, l'app esegue. Tutto ciò che è deterministico (quando partire,
dove lavorare, creare/rimuovere worktree, calcolare il diffstat) lo fa l'app;
l'orchestratore viene svegliato solo quando serve una decisione o
un'integrazione.

## Modello dati

Board per workspace, file `<app_data>/boards/<workspace_id>.json` (scrittura
atomica con `write_atomic`, come `workspaces.json`). Serde `snake_case`.

```jsonc
{
  "workspace_id": "ws-...",
  "max_parallel": 3,          // 1..=16, worker di board in esecuzione insieme
  "paused": false,            // board in pausa: lo scheduler non avvia nulla
  "base_branch": null,        // default per i worktree; null = branch corrente della root
  "base_branches": {},        // repo -> branch base (multi-repository, vedi sotto)
  "integration_branch": null, // modello del branch di integrazione ("integration/{date}"); null = spento
  "integrations": {},         // repo -> { branch, path, base }, risolti dall'app (persistiti)
  "owner": null,              // tab dell'orchestratore proprietario (non persistito: null al caricamento)
  "updated_at": 0,
  "tasks": [ /* Task */ ]
}
```

Task:

```jsonc
{
  "id": "auth-api",           // ^[a-z0-9][a-z0-9-]{0,39}$, unico nella board
  "title": "API di login",    // 1..=80
  "task": "...",              // testo per il worker, 1..=100000, in inglese
  "depends_on": ["db-schema"],// id esistenti, niente cicli
  "touches": ["src/auth/"],   // aree toccate, path relativi alla root (max 50, ognuno <=256, niente "..")
  "isolation": "worktree",    // "worktree" (default) | "none"
  "base_branch": null,        // override del base branch per questo task
  "repo": null,               // repository del task (cartella relativa alla root); null = la root
  "agent": "claude-code",
  "model": null,
  "permission_mode": null,    // null = tetto del workspace; solo restrizioni
  "priority": 0,              // piu' alto = prima
  "held": false,              // trattenuto: lo scheduler non lo avvia
  "status": "queued",         // queued | running | review | done | failed | blocked | cancelled
  "worker_status": null,      // WorkerStatus del worker corrente (starting, running, background, needs_input, idle, ...)
  "session": null,            // id del tab worker corrente
  "attempts": 0,              // avvii effettuati
  "worktree": null,           // { "path": "...", "branch": "task/auth-api", "base": "main" }
  "last_report": null,        // ultimo fact/report troncato (200)
  "note": null,               // nota dell'app/utente: "interrupted: the app was restarted", errori di worktree, ...
  "external": false,          // true = completato fuori dalla board (done da queued, la nota dice come)
  "orchestrator_note": null,  // nota privata dell'orchestratore (max 2000): mai inviata al worker
  "report": null,             // TaskReport, vedi sotto
  "created_at": 0, "updated_at": 0, "started_at": null, "finished_at": null
}
```

TaskReport (da `complete_task`, campi strutturati opzionali):

```jsonc
{
  "status": "completed",      // completed | failed | blocked
  "summary": "...",
  "commit": "abc1234",        // opzionale
  "files_changed": ["src/auth/api.rs"],
  "tests": [{ "command": "cargo test", "result": "passed", "details": null }], // result: passed | failed | not_run
  "follow_ups": ["..."],
  "diffstat": "3 files changed, ..."  // calcolato dall'app per i task con worktree
}
```

Limiti: max 200 task per board, max 50 task per `submit_plan`.

### Colonna (derivata)

Ogni task esposto (`get_task_details`, comandi Tauri, evento `board-update`; `get_board` lo
riporta nelle righe compatte) ha un
campo `column` calcolato dal backend:

| status      | condizione                         | column      | etichetta UI   |
|-------------|------------------------------------|-------------|----------------|
| queued      | qualche dipendenza non `done`      | `backlog`   | In attesa      |
| queued      | tutte le dipendenze `done`         | `ready`     | Pronto         |
| running     |                                    | `running`   | In corso       |
| review      |                                    | `review`    | Da integrare   |
| done        |                                    | `done`      | Fatto          |
| failed      |                                    | `failed`    | Fallito        |
| blocked     |                                    | `blocked`   | Bloccato       |
| cancelled   |                                    | `cancelled` | Annullato      |

Una dipendenza `cancelled` **non** soddisfa: il dipendente resta in backlog
finché l'orchestratore non aggiorna le dipendenze.

`held` è un flag mostrato a parte ("In pausa"), non una colonna. Un task trattenuto
è nella colonna `ready` ma non parte: `get_board` lo marca con `held: true` e il campo
`held` di primo livello ne dà il numero (assente se 0); la legenda della UI mostra
"Pronto N · M in pausa".

Un task `external` è in colonna `done` (sblocca i dipendenti) ma non ha avuto worker né
worktree: la UI mostra "Completato fuori board" con la nota.

## Ciclo di vita

```text
queued --(scheduler)--> running --complete_task completed--> review --orchestratore: done--> done
                          |  \--complete_task failed/blocked--> failed / blocked
                          |  \--tab chiuso senza report (exited)--> failed (note)
                          \--creazione worktree fallita--> failed (note, evento start_failed)
failed/blocked/cancelled/review --retry--> queued (attempts resta; il worktree si riusa)
qualunque non-done --cancel--> cancelled (se running: il tab viene chiuso)
```

- `stopped_without_report` (worker fermo dopo il sollecito dello Stop hook):
  il task resta `running` con `worker_status = idle`; l'orchestratore decide
  (send_message, retry, cancel).
- Al **riavvio dell'app** i task `running` diventano `failed` con nota
  `interrupted: the app was restarted` (i tab automatici non si ripristinano).

## Scheduler

Funzione pura, eseguita dopo ogni modifica della board (submit, update,
cambio stato, transizione di un worker, azione utente, `set_board`):

1. Board `paused`: non si avvia nulla. L'assenza di un owner invece non
   blocca: si avvia comunque e gli eventi restano in coda per il prossimo owner.
2. Candidati: `queued`, `!held`, dipendenze tutte `done`; ordinati per
   `priority` desc, poi `created_at`, poi posizione.
3. Slot liberi = `max_parallel - task running della board`, limitati anche dal
   tetto di 16 worker attivi per orchestratore (ad-hoc + board).
4. Si scelgono greedy i candidati che **non sono in conflitto** con i running
   e con quelli già scelti:
   - **touches**: due task confliggono se un'area dell'uno è uguale o prefisso
     per segmenti di un'area dell'altro (normalizzazione: trim, `\` → `/`,
     via `./` iniziale e `/` finale; su workspace Windows senza distinzione
     di maiuscole). `touches` vuoto = nessuna area dichiarata.
   - **isolation none**: condividono la cartella del workspace, quindi al
     massimo un task `none` non read-only (permesso effettivo diverso da
     `plan`) alla volta.
5. Ogni task scelto passa subito a `running` (`worker_status = starting`,
   `attempts += 1`, `started_at`), poi l'avvio vero (git + spawn) avviene
   fuori dal lock su un thread separato (mai sul main thread: git è lento).

Quando tutti i task sono `done` o `cancelled` (e ce n'è almeno uno) parte
l'evento `plan_completed`, una volta sola per transizione. Il testo dà i conteggi:
"Plan completed: N done (of which E completed outside the board), F failed, C cancelled."
Se tutti i task sono stati annullati senza mai partire (`attempts` 0) non parte nessun
evento: non c'è nulla da riferire (l'orchestratore li ha annullati lui).

### Orchestratore disponibile (on demand)

Lo scheduler avvia un nuovo task solo se nel workspace c'è un orchestratore
disponibile: il tab dell'owner della board vivo, un orchestratore aperto
dall'utente o quello aperto dall'app. I worker già in esecuzione non vengono
mai fermati.

- Se la board ha bisogno di un orchestratore (non in pausa con task avviabili,
  oppure task in `review`, oppure richieste di aiuto/eventi in coda) e nessuno è
  vivo, l'app ne apre uno da sola (**on demand**, in qualunque workspace, non
  solo in quelli automatici) con un prompt di avvio che gli dice di caricare la
  skill orchestrate, chiamare `get_workflow`, `get_board` e `get_events` e
  portare avanti il piano. Lo chiude da sola quando è idle e la board non
  richiede altro.
- I workspace con l'orchestratore automatico mantengono le finestre di
  esecuzione (`docs/SCHEDULE.MD`): fuori finestra non parte nulla e non si apre
  nulla.
- Se l'utente chiude l'orchestratore aperto dall'app, o l'avvio fallisce 3
  volte, l'app non lo riapre: la vista Board mostra "Nessun orchestratore
  attivo" con il pulsante "Avvia orchestratore". La vista mostra anche gli
  stati "in attesa" e "avvio".
- Per gli orchestratori: chiudere il turno è sicuro; un nuovo orchestratore parte
  da `get_board` e `get_events` per recuperare gli eventi pendenti. La
  preparazione del worktree spetta agli hook di setup del repo: il worker parte
  appena worktree e setup sono pronti, l'orchestratore non può prepararlo nel
  mezzo.

## Worktree gestiti dall'app

Per i task con `isolation: "worktree"`:

- Path: `<root>/.worktrees/<task-id>` (separatore dell'ambiente: `\` su
  Windows, `/` su WSL); branch `task/<task-id>`.
- Base: `task.base_branch` → `board.base_branch` → branch corrente della root
  (`git rev-parse --abbrev-ref HEAD`) **al momento dell'avvio**, così i
  dipendenti partono dalla base già aggiornata con i merge delle dipendenze.
- Creazione: `git worktree add <path> -b <branch> <base>`. Retry: se il
  worktree esiste si riusa; se esiste solo il branch, `git worktree add <path> <branch>`.
- Se l'exclude non si aggiorna (o non si trova) l'avvio prosegue, ma l'avviso
  finisce nella `note` del task (`start warnings: ...`).
- Ogni comando git (e `wsl.exe`) ha un tetto di 120 s, poi il processo si
  uccide e l'avvio fallisce. Gli avvii dello stesso task sono serializzati
  (cancel + retry non sovrappongono due `git worktree add`). Su WSL gli argv
  passano con `wsl.exe -e` (mai `--`: la shell di default della distro
  rompeva spazi, `;` e apici).
- Guardiano (giro da 30 s): un task `running` senza sessione da oltre 5 minuti
  diventa `failed` come un avvio fallito (evento `start_failed`); un worker di
  board che dopo 5 minuti non ha mai chiamato `get_task` fa fallire il task
  (nota "never called get_task"), manda `exited` all'owner e chiude il tab.
- Lo scheduler rigira anche quando un worker dell'owner chiude il tab o
  riferisce (libera un posto nel tetto di 16) e, per tutte le board, al primo
  giro dopo l'avvio dell'app. Il gate della finestra e' calcolato all'ora
  corrente dello scheduling, non all'ultimo giro del timer.
- `.worktrees/` viene aggiunto a `<git-common-dir>/info/exclude` se manca
  (non si tocca `.gitignore`).
- Git su Windows: `git` con `current_dir` e `CREATE_NO_WINDOW`; su WSL:
  `wsl.exe -d <distro> --cd <dir> -- git ...` (argomenti passati come argv,
  mai interpolati in una shell).
- `submit_plan` con task `worktree` fallisce se la cartella del repository del task
  (la root, o `repo` sotto la root) non è un repo git.
- **Setup**: nuovo campo del workspace `worktree_setup: Option<String>` (es.
  `npm ci`), impostato nelle impostazioni del workspace. Non lo esegue
  l'app: viene scritto nell'ambiente del task restituito da `get_task` come
  primo passo da eseguire. Il setup eseguito dall'app, per repository, e' descritto
  in "Hook per repository" (sotto).
- **Diffstat**: a `complete_task` l'app calcola `git diff --stat <base>...HEAD`
  più il numero di file non committati (`git status --porcelain`), troncato a
  4000 caratteri, e lo salva in `report.diffstat`.
- **Pulizia**: `set_task_status done` (con `cleanup`, default true) esegue
  `git worktree remove <path>` (senza `--force`) e `git branch -d task/<id>`
  (safe). Gli errori non bloccano il passaggio a `done`: tornano come
  `warnings` nel risultato e in `note`.

### Workspace multi-repository

A workspace whose root is not a git repository but contains several (e.g.
`/home/u/freud-container/{freud-manager,DDB,...}`) works through the task field
`repo` and two board settings. Tasks without `repo` behave exactly as before.

- `task.repo`: folder relative to the workspace root, normalized to `/`, no
  trailing `/`, no absolute path, no `..`; `"."` or `null` = the root (stored as
  `null`). Settable in `submit_plan` and in `update_task` (only while the task is
  updatable and has no worktree yet). `touches` are relative to the repo.
- Validation: `submit_plan` / `update_task` check, once per distinct repo
  directory, that every `worktree` task has a git repo (`rev-parse
  --is-inside-work-tree`); the error names the repo. Outside the registry lock.
- Worktree: `<root>/<repo>/.worktrees/<id>`, branch `task/<id>`, `.worktrees/`
  added to that repo's `info/exclude`, every git command runs with the repo as
  cwd (also diffstat and cleanup). `isolation: "none"` runs in `<root>/<repo>`.
- Base branch of a task: `task.base_branch` → (integration branch, below) →
  `board.base_branches[repo]` (key `"."` = root) → the workspace setting
  `repos[repo].base_branch` (see "Hook per repository") → `board.base_branch` (root
  tasks only) → for a repo task the default of `origin`
  (`git symbolic-ref --short refs/remotes/origin/HEAD`) → the repo's current branch.
  `base_branches` is set by `submit_plan` / `set_board` and merged key by key (an
  empty string or null removes a key).
- Repo tasks: before creating the worktree the app runs `git fetch origin
  <base>` (30 s timeout, skipped without an `origin` remote; cached in memory
  60 s per repo and base) and creates the worktree from `origin/<base>` if it
  exists, else from the local `<base>`. A failed fetch does not block: the task
  starts from the local refs and `note` gets `start warnings: ...`.
- Scheduler: touch conflicts and "one writing `isolation: none` task at a time"
  apply only between tasks with the same repo (`null` and `"x"` never conflict).
- Integration branch (`integration_branch`, also in `submit_plan` / `set_board`;
  `{date}` = local date `YYYY-MM-DD`; empty or null turns it off): before the first
  worktree task of a repo the app creates that branch (from the base the task would
  use, after the fetch) with a worktree `<repoDir>/.worktrees/integration`. If the
  branch exists and was not created by this board, `-2`, `-3`, ... is appended. The
  resolved `{ branch, path, base }` is stored in `board.integrations[repo]` (`"."` =
  root) and reused after restarts; changing the template discards it. Worktree tasks
  of that repo without an explicit `base_branch` start from the integration branch
  (local ref), so dependents see what the orchestrator merged there. The orchestrator
  merges and tests in that worktree. The app never pushes, never removes the
  integration worktree and never runs checkout/switch in the main checkout.
- Repo discovery: `detect_repos(root)` lists depth-1 subfolders (not hidden) with a
  `.git` dir or file, plus `"."` if the root is a repo (WSL: one `sh -c` call).
  `get_board` returns it as `repos`, computed lazily on the first `get_board` and
  cached per board until `submit_plan` / `set_board`; `get_board` also returns
  `base_branches`, `integration_branch` and `integrations` when set. Tauri command
  `workspace_detect_repos(workspace_id) -> string[]` (async, off the main thread).

#### Hook per repository (setup e pulizia eseguiti dall'app)

Impostazione del workspace `repos: { "<repo>": RepoConfig }` (chiave = cartella
relativa alla root, `"."` = la root; pagina Impostazioni > Workspace > Repository;
comando Tauri `set_repo_config(workspace_id, repo, config | null)`):

```
RepoConfig { base_branch?, container_root?, copy_ignored: [], setup: [], cleanup: [], remove_force }
```

- Validazione: `base_branch` come gli altri nomi di branch; `copy_ignored` solo percorsi
  relativi al repository (niente assoluti ne' `..`); `setup`/`cleanup` comandi non vuoti
  su una riga, al massimo 2000 caratteri, al massimo 30 per elenco.
- `base_branch`: nell'ordine della base, subito dopo `board.base_branches[repo]` e prima
  del default di `origin` (per la root, prima di `board.base_branch`). Si legge dal
  workspace al momento dell'avvio del task.
- L'orchestratore non può preparare un worktree in anticipo: il worker parte appena il
  worktree è creato e il setup del repo (`copy_ignored` + `setup`) è finito, quindi la
  preparazione per repo (es. `composer install`) va nell'hook di setup configurato dall'utente.
- Quando: subito dopo la creazione (non al riuso) del worktree del task e di quello di
  integrazione. Sotto il lock dei repository per l'integrazione (un'integrazione per
  volta), nel thread di avvio del task per i task; mai sul main thread ne' sotto il
  lock del registro.
- Ordine: (1) `copy_ignored`: per ogni percorso, solo se `git check-ignore -q` lo
  conferma ignorato e la sorgente esiste, copia da `<repo>` al worktree (file o
  cartelle, ricorsiva; WSL `cp -a`); gli altri sono saltati con un avviso. (2) i
  comandi di `setup` in ordine, con cwd = il worktree.
- Shell: WSL `wsl -d <distro> --cd <dir> -e env K=V... sh -c <comando>` (le variabili
  viaggiano come argv di `env`: `WSLENV` non porta variabili arbitrarie e non c'e'
  nessun escape); Windows `cmd /D /S /C` senza finestra.
- Variabili: `REPO` (cartella del repository), `WORKTREE`, `TASK_ID` (`integration`
  per il worktree di integrazione), `BRANCH`, `BASE`, `CONTAINER_WORKTREE`
  (`container_root` + percorso del worktree relativo a `REPO`, con `/`; vuota senza
  `container_root`).
- Tetti: 10 minuti per comando; dell'output si tengono gli ultimi 2 KB (stdout e stderr).
- Fallimento: l'avvio del task fallisce (`start_failed`, task `failed`, nota con il
  worktree, il passo, `setup 2/3: <comando>`, e la coda dell'output). Il worktree resta
  per l'ispezione ed e' registrato con `worktree.setup_done = false`: al retry si
  riusa e il setup si rifa **tutto** (scrivi comandi ripetibili, es. `rm -rf vendor &&
  cp -a ...`). Lo stesso vale per l'integrazione (`integrations[repo].setup_done`): il
  prossimo avvio di un task del repository la riprepara. Dopo un setup riuscito il
  riuso non lo ripete; un worktree trovato in cartella senza registrazione non e' nostro
  e non si prepara.
- Avvisi (file non ignorati o assenti): finiscono in `note` come `start warnings: ...`.
- Stato visibile: mentre l'app prepara (setup in corso) il task e' `running` senza
  sessione con `preparing = "<passo>"` (nella vista e in `get_board`); la UI lo mostra
  come "Preparazione…".
- Guardiano dell'avvio (5 minuti senza sessione): con un passo in corso aspetta
  10 minuti + 2 dall'ultimo battito (`preparing_since`, aggiornato a ogni passo e
  mentre un avvio aspetta il lock dei repository), poi 5 minuti dalla fine del setup per
  il resto dell'avvio: i setup lunghi non vengono uccisi, un thread morto si scopre.
- Pulizia a `done` (se `cleanup` non e' false): se il worktree esiste, prima i comandi di
  `cleanup` (cwd = worktree, stesse variabili, 2 minuti ciascuno, un errore e' un avviso
  e non blocca mai), poi `git worktree remove` (con `--force` se `remove_force`), poi
  `git branch -d`. Il worktree di integrazione non si rimuove da solo.
- Il worker: `environment` dice che il worktree e' gia' stato preparato dall'app
  (file copiati e comandi eseguiti) e dove e' montato nel container.

### Ambiente del worker

`get_task` per un task di board aggiunge `environment` (testo in inglese) con:
workspace root, cartella di lavoro, worktree/branch/base, comando di setup da
eseguire per primo, aree dichiarate (`touches`) e regole fisse: lavorare solo
nella cartella indicata, committare sul branch del task, non fare merge, push
né cambi di branch, chiudere con `complete_task` compilando i campi
strutturati. Per `isolation: none`: lavorare nella root senza creare branch e
senza toccare aree fuori da `touches`.

## Proprietà della board

- `owner` = tab dell'orchestratore che ha chiamato per ultimo un tool di
  board (`submit_plan`, `get_board`, `get_task_details`, `update_task`,
  `set_task_status`, `set_board`). I tool di board sono riservati agli orchestratori il cui tab
  appartiene al workspace della board.
- I worker dei task vengono creati con `orchestrator = owner` (o senza owner:
  gli eventi restano in una coda della board). Al cambio di owner i worker
  vivi della board vengono riassegnati e i loro eventi in coda spostati nella
  coda del nuovo owner.

## Tool MCP (orchestratore)

Nuovi (descrizioni in inglese, come gli altri):

- `submit_plan { tasks: TaskSpec[], max_parallel?, base_branch?, base_branches?, integration_branch? }` — aggiunge
  task alla board del workspace (le dipendenze possono riferirsi a task della
  stessa chiamata o già presenti), valida tutto prima di modificare (id unici,
  dipendenze esistenti, niente cicli, permessi entro il tetto, repo git se
  serve), diventa owner, esegue lo scheduler. Ritorna la board compatta (come
  `get_board`) più `added` (gli id aggiunti).
  `TaskSpec` = i campi di input del task: `id`, `title`, `task`,
  `depends_on`, `touches`, `isolation`, `base_branch`, `repo`, `agent`, `model`,
  `permission_mode`, `priority`, `held`, `orchestrator_note` (privata: get_board e
  get_task_details la restituiscono, `get_task` del worker no).
- `get_board { columns?: string[] }` — vista compatta della board (niente testo dei
  task, risparmia token):
  `{ workspace_id, rev, max_parallel, paused, base_branch, owner, updated_at,
  automation, counts, tasks }` (più, se presenti, `base_branches`,
  `integration_branch`, `integrations`, `repos`; ogni integrazione è `{ branch, path, base, setup }` con
  `setup` = `done` | `none` (nessun setup configurato per il repo, niente è stato preparato) |
  `pending` (setup non completato), più `missing: true` se la cartella del worktree non esiste
  più; compare solo dopo l'avvio del primo task con worktree del repo; le righe dei task hanno `repo`). `counts` = numero di task per colonna (solo le
  colonne non vuote); `tasks` = una riga per ogni task non `done`/`cancelled`, nell'ordine
  della board. Con `columns` (nomi tra `backlog`, `ready`, `running`, `review`, `done`,
  `failed`, `blocked`, `cancelled`; un nome sconosciuto o un non-array è un errore che
  elenca i nomi validi) le righe sono solo quelle delle colonne indicate, anche `done`.
  Riga: `id`, `title`, `column`, `attempts` e, solo se significativi, `held` (solo sui `queued`),
  `worker_status`, `external`, `waiting_on` (solo in `backlog`: dipendenze non ancora `done`),
  `priority` (se diversa da 0), `model`, `note` e `orchestrator_note` (max 200 caratteri) e `summary` (il
  riassunto del report, max 300 caratteri); il taglio aggiunge "…".
- `get_task_details { id }` — il dettaglio completo di un task: `{ task }` con tutti i campi,
  testo, worktree, `last_report` e `report` strutturato (commit, files_changed, tests,
  follow_ups, diffstat). Errore `unknown task: <id>` se non esiste. Da usare per rivedere un
  task in `review`, `failed` o `blocked`.
- `update_task { id, ...campi di TaskSpec tranne id }` — solo per task
  `queued`, `failed`, `blocked`, `cancelled`; `isolation` solo se non ha
  ancora un worktree. Rivalida dipendenze e cicli. Ritorna `{ task }` senza il testo.
- `set_task_status { id, status: "done" | "cancelled" | "queued", note?, cleanup? }`
  - `done`: da `review`, `failed`, `blocked` (non da `running`); sblocca i
    dipendenti; pulizia worktree se `cleanup != false` (il branch del task si cancella con
    `-D` se è già mergiato nel branch di integrazione del repo o nella base del worktree,
    altrimenti `-d` e un avviso se non è mergiato). Azzera `held`. Anche da `queued` (anche
    trattenuto) o `cancelled` per lavoro fatto fuori dalla board: `note` non vuota
    obbligatoria, il task diventa `external`; nessuna pulizia (non ha worktree). La UI non
    offre questa azione.
  - `cancelled`: da qualunque stato non `done`; se `running` chiude il tab.
  - `deleted`: rimuove dalla board un task mai avviato e senza dipendenti.
  - `queued` (retry): da `failed`, `blocked`, `cancelled`, `review`; senza `note` la nota
    esistente resta, una nuova la sostituisce.
  Ritorna `{ task, warnings? }` senza il testo del task. Con la board in pausa `update_task`,
  `set_task_status` e `submit_plan` aggiungono `warnings: ["the board is paused: ..."]`.
  `update_task` restituisce anche il testo (`task`) se la modifica cambia `task`.
- `set_board { max_parallel?, paused?, base_branches?, integration_branch?, close_integration?, close_integration_cleanup? }` — ritorna la board compatta.
  `close_integration` (chiave repo, `"."` = root, o array di chiavi) dimentica l'integrazione
  registrata del repo senza toccare worktree e branch (li rimuove l'orchestratore): il task
  successivo del repo crea una nuova integrazione (primo suffisso libero, es. `-3`) dalla
  base aggiornata. Con `close_integration_cleanup: true` l'app esegue anche gli hook di
  pulizia, rimuove il worktree di integrazione e cancella il branch locale solo se è
  contenuto nel suo upstream (non fa mai push). Se la cartella del worktree di integrazione è stata rimossa e il suo
  branch è già mergiato nella base del repo, l'app ne avvia una nuova da sola; se non è
  mergiato ricrea il worktree sullo stesso branch.
- `get_workflow` — oltre a `workflow`, restituisce in sola lettura `workspace_rules` (le
  "Workspace rules" dell'utente, solo se non vuote) e `repo_settings` (per chiave repo:
  `base_branch`, `container_root`, `copy_ignored`, `setup`, `cleanup`, `remove_force`, solo i
  non vuoti). Si modificano solo con `propose_settings` (approvazione dell'utente) o dalle
  impostazioni del workspace.
- `ask_user { question, options? (max 8), task? }` — ritorna `{ request }`. L'utente riceve una
  notifica di sistema e risponde dalla casella "Richieste" nella vista board (pulsanti per le
  opzioni o testo libero) oppure scarta la richiesta. Se `task` è indicato e il task è `queued`
  e non trattenuto, viene trattenuto finché la richiesta non è risolta, poi rilasciato
  in automatico. La risposta arriva come evento `user_answer` (testo = risposta, oppure
  "(dismissed by the user)"; con `request` e `task`), che sveglia l'orchestratore; se nessun
  orchestratore è vivo l'app ne apre uno. `get_board` mostra `requests_pending`
  (`[{ id, kind, task? }]`). Massimo 20 richieste pendenti per board.
- `propose_settings { reason, workspace_rules?, repos? }` — ritorna `{ request }`. `repos` è
  `{ "<chiave repo>": { base_branch?, container_root?, copy_ignored?, setup?, cleanup?,
  remove_force? } }`. Propone le Workspace rules (testo completo nuovo) e/o le impostazioni
  e gli hook per repo: l'utente vede un diff prima/dopo e approva o rifiuta (con commento
  opzionale); l'approvazione applica tutto in modo atomico. Esito come evento
  `settings_decision` (testo `approved` oppure `rejected[: commento]`). Mai permessi né
  comandi consentiti/negati dei worker. Leggere prima i valori attuali con `get_workflow`.

Esistenti, estesi:

- `get_events`: ogni evento ha `task` (id del task, se il worker è di board).
  Nuovi `kind`: `start_failed` (azionabile: l'avvio del task è fallito, testo
  con l'errore), `plan_completed` (azionabile, con i conteggi), `user_action` (non azionabile:
  l'utente ha agito dalla board, testo con l'azione).
- `complete_task` (worker): parametri opzionali `commit`, `files_changed`,
  `tests`, `follow_ups`, oltre a `status` e `summary`. Per un worker di board
  porta il task in `review` / `failed` / `blocked` e salva il report.

## Comandi ed eventi Tauri

Argomenti in camelCase lato JS (convenzione Tauri, vedi `tests/tauri-args.test.mjs`).

- `board_get(workspace_id) -> BoardView` (board vuota se non esiste).
- `board_set(workspace_id, max_parallel: Option<u8>, paused: Option<bool>) -> BoardView`.
- `board_task_action(workspace_id, task_id, action) -> BoardView`, `action` in
  `hold | unhold | retry | cancel | done` (stesse regole dei tool; `done` con
  pulizia). Ogni azione genera un evento `user_action` per l'owner.
- `board_task_set_model(workspace_id, task_id, model: Option<String>) -> BoardView`:
  cambia il modello di un task non ancora partito (stesse regole di `update_task`;
  `None` = modello di default del workspace) e genera un evento `user_action`.
- `set_worker_rules(workspace_id, rules: String) -> Workspace`: regole per i worker
  (trim, max 4000 caratteri). `get_task` le accoda a `instructions` come
  "Workspace rules (always apply): ..." per ogni worker, di board o `spawn_session`; sono
  lette a ogni `get_task`, quindi valgono per i nuovi worker. Mai nel testo `task`.
- `list_sessions`: un worker che non ha ancora chiamato `get_task` ha `starting_secs`
  (secondi dallo spawn) per distinguere un avvio lento da uno bloccato.
- `set_worktree_setup(id, command: Option<String>) -> Workspace` (trim, vuoto =
  nessuno, max 1000 caratteri).
- Evento `board-update` con payload `BoardView` a ogni modifica.

`BoardView` = Board con, per ogni task, `column` e senza nulla di nascosto (il
frontend mostra anche il testo del task nei dettagli).

`BoardView.rev` (u64, campo di primo livello) e' una revisione monotona che
cresce a ogni modifica della vista (anche `worker_status`, `last_report`,
`owner`, `automation`); contatore globale, quindi non torna indietro nemmeno
se la board sparisce e ricompare. Sta in `board-update`, nei comandi e in
`get_board`: il frontend scarta le risposte con `rev` minore di quella gia'
vista. A ogni modifica della vista si aggiorna anche `updated_at`. Alla
rimozione del workspace l'evento `board-update` porta la board vuota con una
`rev` piu' alta di ogni precedente.

Il file della board si salva al massimo una volta al secondo per board (sempre
con l'ultimo stato; il resto in coda si salva all'uscita dell'app); l'evento
`board-update` non e' coalescente.

## Vista Board (frontend)

- Dipendenze: `@xyflow/react` (React Flow 12) e `@dagrejs/dagre` per
  l'auto-layout (sinistra → destra, dipendenze come archi).
- Per workspace: un pulsante "Board" nella card del workspace in sidebar (con
  il numero di task attivi) e la scorciatoia **Ctrl+Maiusc+B** sul workspace
  attivo. La vista copre l'area dei pannelli di quel workspace; i terminali
  restano montati (regola del DOM in `App.tsx`: mai smontare un
  `TerminalView`). Non disponibile nella vista Globale.
- Nodo: id, titolo, colonna (colore), sotto-stato del worker, tentativi,
  branch, ultimo report, flag "In pausa".
- Barra: avanzamento (fatti/totali), stepper `max_parallel` (1–16),
  pausa/riprendi la board, legenda delle colonne.
- Clic su un nodo → pannello dettagli: testo del task, dipendenze, aree,
  worktree, nota, report strutturato (test, file, follow-up, diffstat) e
  azioni consentite dalla colonna: Apri terminale (torna ai terminali e mette
  a fuoco il tab del worker), Trattieni/Rilascia, Riprova, Annulla, Segna come
  fatto.
- Stato vuoto: "Nessun piano: chiedi a un orchestratore (skill orchestrate)
  di crearne uno".
- Testi UI in italiano.
