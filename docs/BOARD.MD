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

Ogni task esposto (`get_board`, comandi Tauri, evento `board-update`) ha un
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

`held` è un flag mostrato a parte ("In pausa"), non una colonna.

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
l'evento `plan_completed`, una volta sola per transizione.

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
- `submit_plan` con task `worktree` fallisce se la root non è un repo git.
- **Setup**: nuovo campo del workspace `worktree_setup: Option<String>` (es.
  `npm ci`), impostato nelle impostazioni del workspace. Non lo esegue
  l'app: viene scritto nell'ambiente del task restituito da `get_task` come
  primo passo da eseguire.
- **Diffstat**: a `complete_task` l'app calcola `git diff --stat <base>...HEAD`
  più il numero di file non committati (`git status --porcelain`), troncato a
  4000 caratteri, e lo salva in `report.diffstat`.
- **Pulizia**: `set_task_status done` (con `cleanup`, default true) esegue
  `git worktree remove <path>` (senza `--force`) e `git branch -d task/<id>`
  (safe). Gli errori non bloccano il passaggio a `done`: tornano come
  `warnings` nel risultato e in `note`.

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
  board (`submit_plan`, `get_board`, `update_task`, `set_task_status`,
  `set_board`). I tool di board sono riservati agli orchestratori il cui tab
  appartiene al workspace della board.
- I worker dei task vengono creati con `orchestrator = owner` (o senza owner:
  gli eventi restano in una coda della board). Al cambio di owner i worker
  vivi della board vengono riassegnati e i loro eventi in coda spostati nella
  coda del nuovo owner.

## Tool MCP (orchestratore)

Nuovi (descrizioni in inglese, come gli altri):

- `submit_plan { tasks: TaskSpec[], max_parallel?, base_branch? }` — aggiunge
  task alla board del workspace (le dipendenze possono riferirsi a task della
  stessa chiamata o già presenti), valida tutto prima di modificare (id unici,
  dipendenze esistenti, niente cicli, permessi entro il tetto, repo git se
  serve), diventa owner, esegue lo scheduler. Ritorna la board.
  `TaskSpec` = i campi di input del task: `id`, `title`, `task`,
  `depends_on`, `touches`, `isolation`, `base_branch`, `agent`, `model`,
  `permission_mode`, `priority`, `held`.
- `get_board { include_task?: bool }` — la board con `column` per task; il
  testo `task` solo con `include_task: true` (risparmia token).
- `update_task { id, ...campi di TaskSpec tranne id }` — solo per task
  `queued`, `failed`, `blocked`, `cancelled`; `isolation` solo se non ha
  ancora un worktree. Rivalida dipendenze e cicli.
- `set_task_status { id, status: "done" | "cancelled" | "queued", note?, cleanup? }`
  - `done`: da `review`, `failed`, `blocked` (non da `running`); sblocca i
    dipendenti; pulizia worktree se `cleanup != false`.
  - `cancelled`: da qualunque stato non `done`; se `running` chiude il tab.
  - `queued` (retry): da `failed`, `blocked`, `cancelled`, `review`.
- `set_board { max_parallel?, paused? }`.

Esistenti, estesi:

- `get_events`: ogni evento ha `task` (id del task, se il worker è di board).
  Nuovi `kind`: `start_failed` (azionabile: l'avvio del task è fallito, testo
  con l'errore), `plan_completed` (azionabile), `user_action` (non azionabile:
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
