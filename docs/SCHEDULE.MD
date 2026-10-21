# Esecuzione automatica e workflow per workspace (v0.7, fase 2)

Si appoggia alla board (`docs/BOARD.MD`). Due funzioni:

1. **Finestre di esecuzione**: fasce orarie globali in cui i workspace
   "automatici" lavorano da soli. A inizio finestra l'app avvia un
   orchestratore e lo scheduler riparte; fuori finestra non parte nessun nuovo
   task (es. per evitare le peak hours 13:00–20:00 lasciando il PC acceso di notte).
2. **Workflow per workspace**: un `workflow.md` che dà all'orchestratore di
   quel workspace un'identità e un metodo su misura, creato con un wizard
   (skill di Claude Code) che intervista l'utente.

## Finestre di esecuzione

### Configurazione (globale)

File `<app_data>/schedule.json` (scrittura atomica), serde `snake_case`:

```jsonc
{
  "enabled": false,
  "windows": [ { "start": "20:00", "end": "13:00" } ],  // ora locale, HH:MM
  "keep_awake": true                                    // impedisce lo standby durante il lavoro
}
```

- Una finestra copre `[start, end)` in ora locale; se `start > end` attraversa
  la mezzanotte; `start == end` non è ammesso. Max 12 finestre; possono
  sovrapporsi (vale l'unione).
- `enabled: false` = nessun effetto: i workspace automatici si comportano come
  quelli normali.
- Ora locale: crate `chrono` (feature `clock`), unica dipendenza nuova.

### Per workspace

Nuovo campo `auto_orchestrate: Option<AutoOrchestrate>` del Workspace
(`#[serde(default, skip_serializing_if = "Option::is_none")]`):

```jsonc
{ "enabled": true, "agent": "claude-code", "model": null }
```

Comando Tauri `set_auto_orchestrate(id, config: Option<AutoOrchestrate>) -> Workspace`.
I permessi dell'orchestratore automatico sono il tetto del workspace
(`worker_permission`), come per i worker: per lavorare di notte senza
nessuno davanti serve un livello che non apra dialoghi.

### Gate dello scheduler

Per un workspace automatico con lo schedule abilitato lo scheduler avvia task
solo se `!board.paused && window_open`. Il gate della finestra è separato dalla
pausa dell'utente: aprire o chiudere una finestra non cambia mai `paused`.
I workspace non automatici ignorano le finestre.

### Timer

Un thread dell'app controlla lo stato ogni 30 s (funzione pura
`window_open(schedule, minuti_locali) -> bool` e `next_change(...)`, testate).
Alle transizioni, e al cambio di configurazione:

**Apertura** (per ogni workspace automatico):
- lo scheduler della board riparte;
- se la board ha lavoro (task `queued` non trattenuti oppure task `review` da
  integrare) e non c'è un owner con tab vivo, l'app apre un **orchestratore
  automatico**: un tab nel workspace (evento `automation-spawn` con
  `role: "orchestrator"`, metadati automatici, non persistito) con il comando
  dell'agente e il prompt di avvio `ORCHESTRATOR_BOOTSTRAP_PROMPT` (inglese,
  ASCII sicuro per la riga di comando, come `BOOTSTRAP_PROMPT`): sei
  l'orchestratore automatico di questo workspace per questa finestra, carica
  la skill orchestrate, chiama `get_workflow` e `get_board`, porta avanti il
  piano (integra i task in review, gestisci i fallimenti), poi chiudi il turno.
- Un orchestratore con tab vivo già owner della board (es. un tab aperto
  dall'utente) non viene duplicato.
- **Un solo orchestratore per workspace**: l'orchestratore automatico non si
  apre finché c'è un orchestratore manuale vivo (tab dell'avvio integrato
  "Orchestratore", registrato col comando `automation_orchestrator_open` prima
  di aprirlo, anche se non ha ancora chiamato nessun tool della board). Il
  comando fallisce ("Un orchestratore è già attivo in questo workspace") se il
  workspace ha già l'orchestratore automatico o un altro manuale, e il tab non
  si apre. La registrazione cade alla chiusura/uscita del tab. Entrambi
  mostrano il badge **ORCH** nell'intestazione.

**Chiusura**:
- lo scheduler non avvia più task; i worker in corso finiscono il loro task;
- all'orchestratore automatico l'app digita `WINDOW_CLOSED_TEXT` (inglese):
  la finestra è chiusa, non partiranno nuovi task, integra quelli che
  completano e poi chiudi il turno;
- l'orchestratore automatico entra in **drain**: quando è idle al prompt
  (hook `finished`), nessun task della board è `running` e la sua coda non ha
  eventi azionabili, l'app chiude il suo tab. Alla finestra successiva ne
  parte uno nuovo con contesto pulito (piano e stato sono nella board).
- Un orchestratore aperto dall'utente non viene mai chiuso.

### Note di implementazione

- La riconciliazione e' a livello, non a fronte: gira ogni 30 s (la prima
  10 s dopo l'avvio, quando la webview ha registrato i listener) e subito a
  ogni modifica di configurazione. Copre quindi anche l'avvio dell'app dentro
  una finestra e le modifiche a finestra aperta.
- Se l'utente chiude l'orchestratore automatico durante una finestra, non
  viene riaperto fino alla finestra successiva; un tab che non compare viene
  ritentato al massimo 3 volte per finestra.
- Chiusura dell'utente contro uscita/crash: `automation_tab_closed` arriva
  per entrambe, quindi non basta. Il segnale e' lato backend: il thread di
  lettura del pty, se a EOF la sessione e' ancora in mappa (nessun
  `pty_close`), chiama `pty_exited` PRIMA dell'evento `Exit` al frontend; in
  piu' l'hook `session-end` sul tab dell'orchestratore automatico (agente
  uscito ma shell viva: il tab si chiude d'ufficio). Un'uscita cosi' non e'
  `dismissed`: consuma uno dei 3 tentativi per finestra e il giro successivo
  riapre se c'e' ancora lavoro. Limiti: `/exit` o `/clear` digitati
  dall'utente nel tab contano come uscita; se Claude emette `session-end`
  mentre il pty viene chiuso dall'utente, la chiusura puo' passare per uscita.
- Drain massimo: 30 minuti dall'inizio del drain (`DRAIN_MAX_MS`), poi il tab
  dell'orchestratore automatico si chiude comunque e lo standby non e' piu'
  tenuto dai task ancora in corso di quel workspace fino alla finestra
  successiva (i task non si interrompono).
- Gli effetti `KeepAwake` portano un numero d'ordine: uno vecchio applicato in
  ritardo da un altro thread si scarta.
- Disattivare lo schedule o il flag automatico del workspace a finestra
  aperta manda l'orchestratore automatico in drain (riceve
  `WINDOW_CLOSED_TEXT` e il tab si chiude), anche se i task continuerebbero a
  partire.
- Se la finestra si riapre durante un drain, il drain termina prima e poi
  parte un orchestratore nuovo.
- L'orchestratore automatico ha `parent_tab_id` vuoto, nessun `cwd` e titolo
  "Orchestratore automatico"; il frontend segnala la chiusura del suo tab con
  `automation_tab_closed` come per i worker (fallback: pty assente, con 60 s
  di tolleranza dopo l'avvio).
- `automation.enabled` in `BoardView` e' lo stato globale dello schedule, non
  il flag del workspace (che vale gia' per il solo fatto che `automation` non
  e' `null`).

### Standby

Con `keep_awake` l'app impedisce lo standby del sistema (Windows:
`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` da un thread
dedicato, rilasciato quando non serve più) finché almeno un workspace
automatico ha la finestra aperta con lavoro, o task in esecuzione/drain.

### Stato esposto

- `schedule_get() -> ScheduleState` e `schedule_set(schedule) -> ScheduleState`
  (validazione: formato `HH:MM`, start != end, max 12).
- `ScheduleState = { schedule, window_open, next_change_at /* unix ms o null */ }`.
- Evento `schedule-state` con lo stesso payload a ogni transizione o modifica.
- `get_board` (MCP, vista compatta) e `BoardView` includono
  `automation: { enabled, window_open, next_change_at } | null` per i workspace
  automatici.

## Workflow per workspace

### File

`<app_data>/workflows/<workspace_id>.md` (testo, max 50000 caratteri),
rimosso con il workspace. Lo legge l'orchestratore, quindi è in inglese.

### Tool MCP (orchestratore del workspace)

- `get_workflow {}` → `{ "workflow": "<markdown>" | null }`.
- `save_workflow { content }` → salva (sovrascrive) il workflow del workspace
  del chiamante; lo usa il wizard.
- Le istruzioni MCP dell'orchestratore e la skill orchestrate dicono di
  chiamare `get_workflow` prima di pianificare: il workflow del workspace ha
  la precedenza sul metodo generico della skill (dove non lo contraddice la
  sicurezza: niente push o merge vietati dal workflow, ecc.).

### Comandi Tauri

- `workflow_get(workspace_id) -> Option<String>`.
- `workflow_save(workspace_id, content: Option<String>)` (None o vuoto = elimina).

### Wizard (skill `workflow`)

Seconda skill del plugin Claude Code: `plugins/cli-companion-notify/skills/workflow/SKILL.md`
(`/cli-companion-notify:workflow`), spedita come `orchestrate` (include_str).
Testo per il modello in inglese; l'intervista si fa nella lingua dell'utente,
con lo strumento di domande a scelta dell'agente (in Claude Code
AskUserQuestion), poche domande per volta, proponendo default ricavati dal
repo (legge README, CLAUDE.md, package/Cargo, CI, git log/branch).

Argomenti dell'intervista: identità e obiettivi del progetto, branch base e
strategia di branch, politica di merge (merge/squash/rebase, chi approva),
comandi di build/test/lint e regressione obbligatoria dopo ogni merge,
convenzioni di commit, dimensione e granularità dei task, parallelismo,
modelli per tipo di task, aree da non toccare, quando decidere da solo e
quando fermarsi a chiedere all'utente, cosa fare di notte (finestre
automatiche) senza l'utente, formato e lingua del report finale, setup dei
worktree.

Output: `workflow.md` con sezioni fisse (Identity, Goals, Branching & merge,
Verification, Task design, Parallelism & models, Boundaries, Escalation,
Unattended runs, Reporting). Lo mostra, chiede conferma, lo salva con
`save_workflow`. Se esiste già, parte da quello e chiede cosa cambiare.

Altri coding agent: lo stesso testo potrà essere spedito nel loro formato di
comandi; per ora solo Claude Code.

## UI

- **Impostazioni globali**: sezione "Esecuzione automatica": attiva/disattiva,
  elenco fasce (ora inizio/fine, aggiungi/rimuovi), "Impedisci lo standby",
  stato attuale ("Finestra aperta fino alle 13:00" / "Prossima apertura alle 20:00").
- **Impostazioni del workspace**: "Orchestratore automatico" (attiva, agente,
  modello) e "Workflow": stato (presente/assente), editor di testo con salva,
  pulsante "Crea con il wizard" che apre un nuovo terminale nel workspace con
  `claude "/cli-companion-notify:workflow"` (solo se Claude Code è
  installato per quell'ambiente).
- **Board**: badge per i workspace automatici ("Automatico · finestra aperta
  fino alle 13:00" / "in attesa della finestra delle 20:00").
- Il tab dell'orchestratore automatico ha i metadati automatici (tooltip
  "Orchestratore automatico") come i worker.
