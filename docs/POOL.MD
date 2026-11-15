# Pool predefinita: segretari, Bot e skill

Analisi (nessuna modifica al codice) della **pool predefinita** che cli-companion dovrebbe offrire out of the box a
un **utente medio, non tecnico**, che usa l'Agents HUB e i worker per fare sviluppare e gestire un progetto. Copre il
percorso intero: la richiesta scritta in chat a un segretario, l'instradamento verso un task, la scelta del Bot che
lo esegue, la skill che il Bot carica, il modello e l'effort. Lo stato attuale è quello del codice al momento
dell'analisi (branch `main`, commit `fe7acec`); ogni affermazione ha il riferimento al file da cui viene. I punti
che all'inizio non si erano potuti verificare nel codice sono stati verificati dopo (`pool-verify-unknowns`).

Documenti collegati: [HUB.MD](HUB.MD) (segretari, motore, pianificazione dal thread), [BOARD.MD](BOARD.MD) (task,
kind, Bot, skill del plugin, review e pianificazione), [DESIGN.MD](DESIGN.MD) (mappa dei moduli).

Convenzione: i testi che leggerà il modello (prompt dei segretari, `task` dei Bot, SKILL.md) sono citati come bozza
**in inglese**; tutto il resto è in italiano.

> **Stato di adozione.** Le decisioni D1–D9 (§8) sono prese: l'utente ha adottato **tutte le raccomandazioni**.
> D1 (a) esito "Answer" dei task `plan` + Bot dedicati; D2 (a) `Standard` + effort `low` ora, fascia `Light` (T10) da
> rivalutare dopo l'uso; D3 (a) Operatore opt-in; D4 (b) passo che aggiorna il `task` solo se identico al vecchio
> preset; D5 (a) Bot rischiosi seeded spenti; D6 (a) nessun Bot "feature" separato; D7 (a) nessun segretario
> dedicato, Coordinatore esteso; D8 (b) avviso «versione aggiornata»; D9 (b) solo `code-answer` e `bug-fix` nella
> prima release. Il blocco **must** è consegnato: T2 (skill `code-answer`, `bug-fix`), T1 (esito "Answer"), T3 (Bot
> `quick-answer`, `bug-fixer`, migrazione v4), T4 (`task` esclusivi, migrazione v5), T5 (Coordinatore e Analista,
> avviso «versione aggiornata»), T11 (documentazione: [BOARD.MD](BOARD.MD), "Skill del plugin e Bot" e
> "Agente e modello di un task"; [HUB.MD](HUB.MD), "Segretari predefiniti" e "Pianificazione dal thread"). Restano da
> fare i task should/could (T9, T10): la seconda tappa di T2 ha consegnato `code-research`, `test-qa`,
> `docs-writer` e `safe-refactor`, la terza `git-ops` e `release-prep`, la seconda tappa di
> T3 i Bot `researcher`, `test-writer`, `docs-writer` e `refactorer` (accesi, migrazione v6 di `bots.json`), la terza
> i Bot `git-operator` e `release` (spenti di serie con `enabled_by_default`, migrazione v7; D5). Le sezioni 1–8 restano l'analisi originale: dove il
> codice è cambiato fa fede la documentazione collegata.

---

## 1. Stato attuale

### 1.1 Gli attori e dove stanno

| Attore | Cosa è | Dove è definito | Con quale modello parla |
|---|---|---|---|
| **Segretario** | un bot con un nome (`@mario`) che risponde nei thread dell'HUB | `src-tauri/src/hub/settings/presets.rs` (preset), `secretary.rs` (dato e tool), `secretaries.rs` (elenco, seeding) | **un solo** endpoint compatibile OpenAI per tutta l'app (`provider.base_url` + `provider.model`, vedi [HUB.MD](HUB.MD), "Segretari e provider"): spesso un modello locale (llama-server, Ollama) |
| **Bot** (Bot-agente) | la configurazione con cui parte un coding agent: agente, modello, effort, skill, `kinds` | `src-tauri/src/bots/presets.rs`, `bots/agent.rs`, migrazioni in `bots/migrations.rs` | il coding agent del workspace (Claude Code, OpenCode) con il modello risolto dalla fascia |
| **Skill del plugin** | uno SKILL.md spedito nel pacchetto di ogni agente che dichiara `plugin-skills` | `src-tauri/src/agents/presets.rs`, testi in `src-tauri/src/agents/presets/<id>.md` | quello del Bot che la carica |
| **Skill utente** | skill globali create dall'utente (con Skill Wizard) | `src-tauri/src/user_skills.rs`, tool MCP in `src-tauri/src/automation/skills.rs` | quello del Bot che la carica |
| **Task** | lavoro in coda, di un `kind` (`work`, `review`, `plan`) | `src-tauri/src/board/review.rs` (`TaskKind`), regole del piano in `board/plan.rs` | snapshot di agente, modello, effort e skill del Bot alla creazione (`board/queue.rs`, `check_specs`) |

Il segretario **non conosce il codice**: le regole comuni del motore lo dicono esplicitamente ("You do not know the
code of the projects and you never decide technical questions", `ENGINE_INSTRUCTIONS` in
`src-tauri/src/hub/engine/context.rs`) e i suoi tool leggono solo board e thread. Tutto ciò che richiede di leggere
o cambiare il codice passa da un task eseguito da un coding agent.

### 1.2 Segretari predefiniti (4)

`PRESETS: [SecretaryPreset; 4]` in `src-tauri/src/hub/settings/presets.rs`, prompt in
`src-tauri/src/hub/settings/presets/<id>.md`.

| Id | Nome | Seeded | Tool | Mansione |
|---|---|---|---|---|
| `coordinator` | `mario` | sì | `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message`, `open_plan_task`, `decide_proposal`, `relay_answer`, `set_thread_state` | trasforma un obiettivo in un task `plan`, presenta e decide le proposte, inoltra le risposte ai worker |
| `analyst` | `giulia` | sì | `thread_read`, `post_message`, `open_plan_task` | intervista l'utente su un'idea vaga, riassume (Goal / Done when / Constraints / Out of scope) e apre il `plan` dopo conferma |
| `reporter` | `luca` | sì | `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message` | stato del progetto, sola lettura |
| `operator` | `ugo` | **no** (opt-in) | lettura + `task_action`, `board_control`, `request_task_action`, `request_task_update`, `set_thread_state` | agisce sulla coda su richiesta esplicita; le azioni irreversibili e le modifiche passano da una scheda di conferma |

Regole fissate dai test di `presets.rs`: ogni preset elenca i tool in modo esplicito (mai `SecretaryTool::ALL`), i
preset seeded non hanno tool che cambiano la board (`only_the_presets_that_do_not_change_the_board_are_seeded`), i
prompt iniziano con "Your job is", sono ASCII e sotto `SYSTEM_PROMPT_MAX / 4` (2000 caratteri), e chi ha
`open_plan_task` o `request_task_update` dice come scegliere il Bot ("the bots you are given", "kinds") senza nominare
`general`.

**Seeding** (`secretaries::seed_presets`): avviene **solo** quando l'HUB si accende e l'elenco dei segretari è vuoto.
Chi ha già dei segretari non riceve mai un preset nuovo, e un prompt di preset cambiato non raggiunge i segretari già
salvati finché l'utente non usa «Ripristina predefinito» (`secretaries::restore_preset`).

### 1.3 Bot predefiniti (8)

`PRESETS: [BotPreset; 8]` in `src-tauri/src/bots/presets.rs`.

| Id | Nome | `task` | Modello | Effort | Skill | `kinds` |
|---|---|---|---|---|---|---|
| `general` | General | General-purpose coding agent: new features and any change no specialized bot covers. | `Default` (decide la CLI) | — | — | `[work]` |
| `intake` | Intake | Turns requests into queue tasks. | `Tier(Strong)` | high | `task-intake` | `[]` |
| `setup` | Setup | Configures the workspace and the worker rules. | `Tier(Standard)` | medium | `project-setup` | `[]` |
| `skill-wizard` | Skill Wizard | Creates and manages global skills. | `Tier(Standard)` | medium | `skill-wizard` | `[]` |
| `reviewer` | Reviewer | Verifies the integrated result of finished tasks and proposes fixes. | `Tier(Strong)` | high | — | `[review]` |
| `planner` | Planner | Plans changes to the project: explores the repository read-only and proposes queue tasks. | `Tier(Strong)` | high | — | `[plan]` |
| `quick-answer` | Quick Answer | Answers a quick question about the code or its git history, read-only, in a few lines. Changes nothing and proposes nothing. | `Tier(Standard)` | low | `code-answer` | `[plan]` |
| `bug-fixer` | Bug Fixer | Fixes a reported bug: reproduces it, adds a failing regression test, fixes the root cause and verifies. | `Tier(Strong)` | high | `bug-fix` | `[work]` |

Vincoli fissati dai test (`presets_are_well_formed`): nessun preset usa `ModelChoice::Id` (deve valere con ogni
agente), `task` ASCII non vuoto, skill esistente fra quelle del plugin, `kinds` normalizzati. `general` è obbligatorio,
non cancellabile né disattivabile (`ensure_general`). `bots/validate.rs`: nome al massimo 24 caratteri
(`NAME_MAX_CHARS`), `task` al massimo 300 (`TASK_MAX_CHARS`). I nomi dei Bot non possono coincidere con quelli dei
segretari (`ensure_free` in `bots/agent_bots.rs`).

**Migrazioni** (`bots.rs`: `[adopt_envelope, add_agent_bots, add_task_kinds, add_answer_and_fix_bots]`, file alla
`schema_version` 4): ogni passo è congelato e additivo, aggiunge in fondo i Bot il cui id manca e gira una volta sola,
quindi un Bot cancellato non ricompare. I passi 2→3 e 3→4 danno un nome libero ("Reviewer 2", "Quick Answer 2") se
il nome è già preso fra i Bot. I passi non conoscono i nomi dei segretari: sono funzioni pure del solo `bots.json`,
che si carica prima di `hub_settings.json`. Per questo, all'avvio, caricati entrambi i file,
`bots::rename_bots_named_like_secretaries` (`bots/secretary_names.rs`) dà il primo nome libero a ogni Bot che porta
il nome di un segretario e salva solo se ne ha rinominato uno: vale anche per i Bot delle migrazioni future con un
nome di una parola ("Researcher").

**Ripristino**: `agent_bots::restore_preset` riporta `task`, modello, effort, skill e `kinds` di un Bot al preset;
`agent_bots::restore_presets` ricrea i preset che mancano. Un preset aggiunto a `PRESETS` diventa quindi
ripristinabile anche per chi lo aveva cancellato.

### 1.4 Fasce, effort e agenti

- **Due sole fasce** (`ModelTier` in `src-tauri/src/agents/contracts/models.rs`): `Standard` ("lavoro ben definito") e
  `Strong` ("progettazione, modifiche rischiose, bug difficili"). Il modello di una fascia è il **primo** suggerito di
  quella fascia (`tier_models` in `src-tauri/src/agents/models.rs`).
  - Claude Code (`agents/claude_code/models.rs`): Standard = il primo standard della lista (oggi il modello di fascia
    media), Strong = il modello di punta; il modello piccolo e veloce è anch'esso `Standard` ma, venendo dopo, **non
    è mai scelto da una fascia**.
  - OpenCode (`agents/opencode/models.rs`): Standard = un modello "mini", Strong = il modello grande.
  - Conseguenza: **non esiste una fascia "economica/veloce"** raggiungibile senza nominare un modello, e
    `ModelChoice::Id` è ammesso solo con un agente fissato (`bots/resolve.rs`, `resolve_model`).
- **Effort**: `Low`, `Medium`, `High`, `XHigh`, `Max` (`agents/contracts/effort.rs`). Claude Code li accetta tutti
  (`agents/claude_code/effort.rs`); **OpenCode nessuno** (`effort: None` in `agents/opencode/mod.rs`): l'effort si
  scarta con un avviso (`resolve_bot`), quindi su OpenCode costo e qualità di un Bot dipendono solo dalla fascia.
- **Skill**: entrambi gli agenti dichiarano `plugin_skills` e `user_skills`; un agente che non li dichiara fa
  scartare la skill con un avviso (`skill_obstacle` in `bots/resolve.rs`) in un tab interattivo, mentre per un task
  il Bot con una skill non caricabile è rifiutato all'accettazione ([BOARD.MD](BOARD.MD), "Agente e modello di un task").
- **Permessi dei worker**: sono del **workspace** (`worker_permission`, `worker_agents[<agente>].allow/deny`, campi
  sensibili delle proposte di impostazioni), non del Bot. Un Bot "in sola lettura" lo è solo per istruzioni (skill o
  `kind`), **non per enforcement**; l'unico isolamento garantito dall'app è il worktree del task `plan`.

### 1.5 Come un task finisce su un Bot

```
utente ──@segretario──▶ motore del segretario (hub/engine)
                          │ open_plan_task { goal, bot }      ← il SEGRETARIO sceglie fra i Bot con `plan`
                          ▼
                       task `plan` (snapshot del Bot) ──▶ planner (role "planner", planner_instructions.md)
                          │ propose_tasks [{ bot, model?, kind? }]  ← il PIANIFICATORE sceglie i Bot dei task
                          ▼
                       proposta nel thread ──«Rispondi: sì»──▶ decide_proposal ← decide l'UTENTE
                          ▼
                       task `work` (+ `review` di obiettivo) ──▶ worker / reviewer
                          │ la review può proporre correzioni (propose_tasks) ← sceglie il REVIEWER
                          ▼
                       eventi nel thread ──▶ il segretario titolare riassume
```

Chi decide cosa:

| Decisione | Chi | Su quale base | Codice |
|---|---|---|---|
| Quale segretario risponde | **l'utente**, con la menzione `@nome` | nessun instradamento automatico fra segretari | `hub/engine/mentions.rs` |
| Se aprire lavoro | il segretario (solo con `open_plan_task`) | il prompt del preset | `hub/settings/presets/*.md` |
| Quale Bot pianifica | il segretario | elenco dei Bot abilitati con `plan` nel system prompt, riga `- id: nome - task` (task tagliato a 320 caratteri) | `hub/engine/bot_choice.rs` (`describe`, `check`) |
| Come si divide il lavoro, quale Bot esegue ogni task, override di fascia, review | il pianificatore | `get_workspace_settings.bots` (solo abilitati e `ready`, `kinds` che includono il kind del task, poi il `task` del Bot) | `src-tauri/src/board/prompts/planner_instructions.md` |
| Se il lavoro parte | l'utente (accetta la proposta) | — | `decide_proposal`, `proposal_decide` |
| Agente, modello, effort, skill effettivi | la **configurazione** del Bot, risolta sull'agente del workspace | snapshot alla creazione | `bots/resolve.rs`, `board/queue.rs` |
| Cambio di Bot di un task in coda | l'utente, o l'Operatore con scheda di conferma | `kinds` del Bot | `request_task_update` |
| Comportamento del ruolo (worker, reviewer, planner) | **solo il `kind`** del task | `Role::of_worker` | `src-tauri/src/automation/roles.rs` |

Un Bot con skill la fa caricare al worker: `get_task` antepone alle istruzioni "Before starting, load the skill …;
the instructions of this task take precedence over the skill where they conflict."
(`src-tauri/src/board/texts/skill.rs`, `automation/task_skill.rs`, `agents/worker_skill.rs`). Una skill **specializza**
il lavoro ma non può scavalcare istruzioni del ruolo e di consegna.

### 1.6 Lacune, sovrapposizioni e rischi

1. **Le domande non hanno un percorso.** Il solo tool che apre lavoro è `open_plan_task`, e il pianificatore ha due
   esiti ammessi: una proposta o "Questions for the user:" (`planner_instructions.md`). Una domanda come «dove si
   gestisce il login?» o «cosa è cambiato questa settimana?» oggi diventa una pianificazione da fascia `strong` /
   effort `high` che **non può rispondere** (o risponde violando le istruzioni) e rischia di proporre task inutili.
   È la lacuna più grave per l'utente non tecnico, che fa soprattutto domande. Nota positiva verificata: un `plan`
   concluso senza proposta riporta nel thread il **report intero**, diviso in parti se serve
   (`plan_without_proposal` in `src-tauri/src/hub/events/texts.rs`), quindi il canale per restituire una risposta
   esiste già; manca solo l'esito ammesso.
2. **Un solo Bot `work` generico.** `general` ha `task` "General-purpose coding agent." e modello `Default`: il
   pianificatore lo usa per tutto (feature, bug, test, docs, refactor) e l'unica leva di qualità è l'override di
   `model`. Nessuna istruzione specializzata per riprodurre un bug, scrivere test, documentare per un non tecnico,
   rifattorizzare senza cambiare comportamento.
3. **Un solo Bot `plan`.** La scelta del segretario è finta: c'è sempre e solo `planner`. Il prompt del Coordinatore
   ("pick … the one whose task fits the work") è pronto per più pianificatori, la pool no.
4. **Tre Bot con skill ma senza `kinds`** (`intake`, `setup`, `skill-wizard`): voluti (skill interattive), ma per
   l'utente dell'HUB sono invisibili, perché il segretario vede solo Bot con `plan` e il pianificatore li salta. Il
   setup del workspace e la creazione di skill non sono raggiungibili dalla chat: vanno aperti come tab.
5. **Nessuna fascia economica** (§1.4): il Bot di una domanda veloce costa almeno come un Bot `Standard`.
6. **Effort ignorato su OpenCode**: due Bot che differiscono solo per effort su OpenCode sono identici.
7. **Git e release non hanno un posto.** Le istruzioni di consegna (`board/texts.rs`) dicono al worker come
   committare e integrare; un'operazione git chiesta dall'utente (pulire branch, risolvere un conflitto, preparare
   un tag) non ha né Bot né regole di sicurezza oltre allo standard dei worker. Il pianificatore vieta a sé stesso i
   comandi git che cambiano stato, ma nulla guida un worker che li deve usare.
8. **Nessun instradamento fra segretari.** L'utente deve sapere chi chiamare: `@luca` per lo stato, `@mario` per il
   lavoro, `@giulia` per le idee vaghe. Un utente medio scriverà quasi sempre allo stesso nome: il Coordinatore deve
   saper reggere da solo i casi comuni (stato, domanda, lavoro) senza rimandare.
9. **Rischio di rotta sui `task` dei Bot.** Il `task` è l'unico segnale di scelta, sia per il segretario sia per il
   pianificatore: con più Bot dello stesso `kind`, testi vaghi o sovrapposti producono scelte casuali.
10. **I preset non raggiungono chi ha già i file**: i segretari si creano solo su elenco vuoto, i prompt cambiati
    solo con «Ripristina predefinito», i `task` cambiati dei Bot solo con il ripristino del Bot. Qualunque piano di
    adozione deve tenerne conto (§7).

---

## 2. Persona di riferimento e catalogo delle richieste

### 2.1 Persona

**Marta**, titolare di una piccola attività, ha un gestionale web fatto da un freelance e ora lo fa evolvere con
cli-companion. Sa cosa vuole ottenere, non sa leggere il codice, conosce git solo di nome ("salvare", "tornare
indietro", "pubblicare"). Scrive in italiano, a frasi brevi, spesso nello stesso thread e quasi sempre allo stesso
segretario. Si fida di ciò che le viene mostrato, quindi l'azione distruttiva deve essere impossibile senza un clic
esplicito. Ha un budget: preferisce che una domanda costi poco e che il modello forte si usi dove serve.

Conseguenze di progetto:

- **un punto d'ingresso** che regge i casi comuni (Coordinatore), gli altri segretari come specialisti facoltativi;
- **risposte senza modifiche** come cittadine di prima classe (domande, ricerche, stato);
- **specializzazione nei Bot**, non nei segretari (i segretari non conoscono il codice, per progetto);
- **sicurezza per costruzione**: le operazioni rischiose sono Bot disattivati di serie, con skill che chiedono conferma.

### 2.2 Catalogo delle richieste

Frequenza stimata per la persona (A = più volte al giorno, M = settimanale, B = occasionale); rischio = danno
possibile se gestita male (B basso, M medio, A alto).

| # | Tipo | Esempi di Marta | Freq. | Rischio | Note |
|---|---|---|---|---|---|
| R1 | Stato del progetto | «a che punto siamo?», «cosa è fallito ieri?» | A | B | sola lettura della board |
| R2 | Domanda veloce sul codice | «dove si cambia il testo della mail di benvenuto?», «usiamo già Stripe?» | A | B | sola lettura, risposta breve |
| R3 | Domanda su git / storia | «cosa è cambiato questa settimana?», «chi ha toccato la pagina ordini?» | M | B | `git log`/`diff` in sola lettura |
| R4 | Ricerca / R&D / esplorazione | «si può aggiungere il pagamento a rate? quanto è difficile?», «confronta due librerie di PDF» | M | B | report lungo, nessuna modifica; può concludersi con una proposta |
| R5 | Sviluppo feature | «aggiungi l'export in Excel degli ordini» | A | M | il caso centrale |
| R6 | Correzione bug | «il totale del carrello è sbagliato con lo sconto» | A | M | riprodurre prima di correggere |
| R7 | Refactoring | «il codice degli ordini è un groviglio, sistemalo» | B | A | il comportamento non deve cambiare |
| R8 | Test / QA | «aggiungi i test sul checkout», «verifica che il login funzioni» | M | B | spesso dopo R5/R6 |
| R9 | Documentazione | «scrivi una guida per i miei dipendenti», «aggiorna il README» | M | B | lingua del lettore, non tecnica |
| R10 | Review | «controlla il lavoro fatto oggi» | M | B | già coperta dal Reviewer |
| R11 | Operazioni git | «elimina i branch vecchi», «risolvi il conflitto», «torna alla versione di ieri» | B | **A** | rischio di perdita di lavoro |
| R12 | Release / deploy | «prepara la versione 1.4», «pubblica» | B | **A** | effetti esterni, spesso irreversibili |
| R13 | Gestione della coda | «riprova quello fallito», «metti in pausa», «cancella quel task» | M | M | Operatore, con conferma per l'irreversibile |
| R14 | Setup / configurazione | «fai in modo che i worker installino le dipendenze», «cambia il branch base» | B | M | proposte di impostazioni (Project Setup) |
| R15 | Creazione di skill | «insegna agli agenti le nostre regole di stile» | B | B | Skill Wizard |
| R16 | Idea vaga | «vorrei qualcosa per fidelizzare i clienti» | M | B | va chiarita prima di pianificare |

---

## 3. Pool di segretari proposta

Principio confermato: i segretari si dividono per **modo di lavorare con l'utente** (come oggi), non per dominio
tecnico. La diversificazione per dominio va nei Bot (§4), dove c'è chi conosce il codice.

### 3.1 `coordinator` — Coordinatore (`mario`) — **da modificare**, seeded

- **Ruolo**: il punto d'ingresso unico. Risponde da sé sullo stato (R1), apre una **domanda** o una **ricerca** con il
  Bot giusto (R2–R4), apre la pianificazione per il lavoro (R5–R12), rimanda agli specialisti solo ciò che non può
  fare (R13 → Operatore, R14/R15 → tab dei Bot interattivi, R16 lunga → Analista, ma con una domanda può gestirla).
- **Tool**: invariati (già ha lettura + `open_plan_task` + `decide_proposal` + `relay_answer` + `set_thread_state`).
  Nessun tool che cambia la coda: il test sui preset seeded resta valido.
- **Modifica del prompt** (bozza, aggiunta alla mansione; resta sotto i 2000 caratteri togliendo ridondanze):

  > Sort each request before acting. A question about the state of the work: answer it from the board yourself. A
  > question about the code, its history or a technical option, with nothing to change: open a planning task with
  > the bot whose task says it answers questions, or researches, if the user wants a deep analysis. A change to the
  > project: open a planning task with the bot whose task says it plans changes. Always pick from the bots you are
  > given (their kinds include plan), by their task. To stop, delete or edit a task, or to change the queue, tell
  > the user to ask a secretary that operates the queue.

- **Giustificazione**: lacuna 1 e 8 di §1.6. È il segretario che l'utente medio userà per tutto; il routing per
  `task` del Bot sfrutta il meccanismo esistente (`bot_choice.rs`) senza tool nuovi.
- **Sovrapposizione**: con l'Analista sulle idee vaghe (lui fa al massimo una domanda; l'Analista intervista); con il
  Cronista sullo stato (il Cronista è la versione che non può aprire nulla, utile da dare a chi vuole solo leggere).

### 3.2 `analyst` — Analista (`giulia`) — invariato nella mansione, ritocco del routing, seeded

- **Ruolo**: R16 (idee vaghe) e i lavori grossi che meritano un brief prima della pianificazione.
- **Tool**: invariati (`thread_read`, `post_message`, `open_plan_task`).
- **Modifica**: una frase per il caso "la richiesta è in realtà una domanda": aprire la pianificazione con il Bot
  che risponde alle domande invece di intervistare. Senza `decide_proposal`, resta il rimando a «Rispondi».
- **Giustificazione**: il valore (domande su obiettivo, criteri, esclusioni, mai tecniche) è specifico e non
  duplicato dal Coordinatore.

### 3.3 `reporter` — Cronista (`luca`) — invariato, seeded

- **Ruolo**: R1, solo lettura. È anche il segretario "sicuro" da lasciare a chi deve solo seguire (es. un socio).
- **Giustificazione**: costo minimo (nessun task), nessun rischio. Il suo prompt è già completo
  (`presets/reporter.md`).

### 3.4 `operator` — Operatore (`ugo`) — invariato, opt-in

- **Ruolo**: R13. Ritenta, riordina, pausa, parallelismo; fermare/eliminare/chiudere/modificare con scheda di conferma.
- **Seeded**: **no**, come oggi. Le regole del repository (test `only_the_presets_that_do_not_change_the_board_are_seeded`)
  e la persona (fiducia in ciò che vede) vanno nella stessa direzione: la si aggiunge da «Aggiungi da modello».
- **Modifica facoltativa**: nessuna nel prompt; nella UI di primo avvio dell'HUB, un suggerimento a crearlo (vedi §8,
  decisione D3).

### 3.5 Nuovo segretario per le domande veloci? — **valutato e sconsigliato** come preset seeded

Si è valutato un preset `scout` ("Esploratore", nome `sara`, tool `thread_read`, `post_message`, `board_status`,
`task_details`, `open_plan_task`) che apre **solo** domande e ricerche.

- **Contro**: (a) non sarebbe garantito dal codice: `open_plan_task` accetta qualunque Bot con `plan`
  (`bot_choice::check`), quindi il limite "solo Bot di domanda" starebbe nel prompt; (b) aggiunge un nome da
  ricordare, contro la lacuna 8; (c) il modello del segretario è lo stesso per tutti, quindi non costa meno del
  Coordinatore; (d) un preset seeded nuovo non arriva a chi ha già segretari (§1.2).
- **Pro**: un segretario che non può mai far partire lavoro di modifica, da dare a un utente che deve solo chiedere.
- **Raccomandazione**: non aggiungerlo ora; il Coordinatore modificato copre R2–R4. Se in futuro servirà il limite
  vero, la strada giusta è nel codice (un tool `open_question_task` che accetta solo Bot marcati come "risposta", vedi
  §7, task T9), non un preset.

### 3.6 Come il segretario instrada

L'instradamento resta **per `task` del Bot**, l'unico segnale che motore e pianificatore già usano. Perché funzioni:

- i `task` dei Bot dello stesso `kind` devono essere **mutuamente esclusivi** e iniziare con il verbo dell'esito
  ("Answers…", "Researches…", "Plans…", "Fixes…");
- il prompt del segretario nomina la **categoria** ("the bot whose task says it answers questions"), mai l'id, come
  richiede il test `presets_that_open_or_change_work_say_how_to_choose_the_bot`;
- se l'utente ha cancellato il Bot giusto, il segretario sceglie il pianificatore generico: degrado accettabile,
  nessun errore.

---

## 4. Pool di Bot proposta

Tutti con `agent: None` (agente del workspace), modello per **fascia** (mai per nome), skill del plugin. "Seeded"
indica se la migrazione li crea; "spento" che li crea con `enabled: false` (campo esistente di `AgentBot`): visibili e
pronti, ma esclusi dal segretario (`bot_choice::describe` filtra gli abilitati) e dal pianificatore (le istruzioni
vietano i Bot non abilitati).

### 4.1 Tabella riassuntiva

| Id | Nome | Kinds | Fascia | Effort | Skill | Stato | Priorità |
|---|---|---|---|---|---|---|---|
| `general` | General | `[work]` | Default | — | — | esistente, `task` da precisare | must |
| `planner` | Planner | `[plan]` | Strong | high | — | esistente, `task` da precisare | must |
| `reviewer` | Reviewer | `[review]` | Strong | high | — | invariato | — |
| `intake` | Intake | `[]` | Strong | high | `task-intake` | invariato | — |
| `setup` | Setup | `[]` | Standard | medium | `project-setup` | invariato | — |
| `skill-wizard` | Skill Wizard | `[]` | Standard | medium | `skill-wizard` | invariato | — |
| `quick-answer` | Quick Answer | `[plan]` | Standard | low | `code-answer` | **nuovo**, seeded acceso | must |
| `researcher` | Researcher | `[plan]` | Strong | high | `code-research` | **nuovo**, seeded acceso | should |
| `bug-fixer` | Bug Fixer | `[work]` | Strong | high | `bug-fix` | **nuovo**, seeded acceso | must |
| `test-writer` | Test Writer | `[work]` | Standard | medium | `test-qa` | **nuovo**, seeded acceso | should |
| `docs-writer` | Docs Writer | `[work]` | Standard | medium | `docs-writer` | **nuovo**, seeded acceso | should |
| `refactorer` | Refactorer | `[work]` | Strong | high | `safe-refactor` | **nuovo**, seeded acceso | could |
| `git-operator` | Git Operator | `[work]` | Strong | medium | `git-ops` | **nuovo**, seeded **spento** | could |
| `release` | Release | `[work]` | Strong | high | `release-prep` | **nuovo**, seeded **spento** | could |
| (esempio) | Security Reviewer | `[review]` | Strong | high | skill utente | **non predefinito**: esempio in documentazione | — |
| (esempio) | Feature (stack X) | `[work]` | Default | — | skill utente con le convenzioni del progetto | **non predefinito** | — |

Totale predefiniti: da 6 a 14 (12 accesi). Il limite di leggibilità è il system prompt del segretario: solo i Bot
con `plan` entrano nel suo elenco (3 righe), quindi la crescita dei Bot `work` non pesa sul segretario; pesa su
`get_workspace_settings.bots` del pianificatore, che è un coding agent con contesto ampio.

### 4.2 Bot esistenti da modificare

**`general`** — resta il ripiego e il Bot delle feature (R5). Un Bot "feature" separato è **sconsigliato**: sarebbe
identico a General e renderebbe ambigua la scelta.

- `task` proposto: `General-purpose coding agent: new features and any change no specialized bot covers.`
- Modello `Default` invariato: rispetta la scelta dei worker del workspace (`worker_agents[<agente>].model`), che è
  la leva che l'utente ha già.

**`planner`** — il `task` va reso esclusivo rispetto ai due nuovi Bot di `plan`.

- `task` proposto: `Plans changes to the project: explores the repository read-only and proposes queue tasks.`

### 4.3 Bot nuovi

**`quick-answer` — Quick Answer** (R2, R3)

- `task`: `Answers a quick question about the code or its git history, read-only, in a few lines. Changes nothing and proposes nothing.`
- Kinds `[plan]`: è l'unico `kind` che oggi (a) un segretario può aprire, (b) gira in un worktree isolato in sola
  lettura (`board/plan.rs`, `TaskKind::isolated`; [BOARD.MD](BOARD.MD), "Task di pianificazione") e (c) restituisce
  il report intero al thread. **Richiede** il nuovo esito "Answer" nelle istruzioni del pianificatore (§7, T1).
- Fascia `Standard`, effort `low`: leggere qualche file e rispondere non richiede il modello forte; è la richiesta più
  frequente, quindi è dove il costo conta di più. Su OpenCode l'effort si scarta: resta il modello "mini".
- Rischio: basso (sola lettura, worktree). Rischio residuo: una domanda che in realtà chiede una modifica; la skill
  dice di rispondere e di suggerire all'utente di chiedere la modifica, senza proporla.

**`researcher` — Researcher** (R4)

- `task`: `Researches a technical question in depth, read-only: options, feasibility, risks and effort, in a structured report. May propose tasks only if the user asked for them.`
- Kinds `[plan]`, fascia `Strong`, effort `high`: il valore è nell'analisi (confronti, rischi, stime), dove il
  modello forte ripaga. Esito: "Answer" con un report strutturato, oppure (se la richiesta lo chiedeva) una proposta
  come il Planner.
- Giustificazione della separazione da `quick-answer`: costo (la stessa domanda fatta a un `strong/high` costa
  diverse volte tanto) e forma dell'esito (tre righe contro un report). Separazione dal `planner`: l'esito atteso
  è conoscenza, non task.

**`bug-fixer` — Bug Fixer** (R6)

- `task`: `Fixes a reported bug: reproduces it, adds a failing regression test, fixes the root cause and verifies.`
- Kinds `[work]`, fascia `Strong`, effort `high`: le istruzioni del pianificatore già prescrivono `strong` per i
  "hard bugs"; un Bot dedicato rende la scelta esplicita invece che un override di `model`.
- Rischio: medio (cambia codice), mitigato dalla review di obiettivo che il pianificatore aggiunge per "the fix of a
  hard bug".

**`test-writer` — Test Writer** (R8)

- `task`: `Adds or improves automated tests for existing behaviour, following the project's test conventions; changes production code only to make it testable.`
- Kinds `[work]`, fascia `Standard`, effort `medium`: lavoro ben definito, ripetitivo. Coerente con lo standard dei
  worker (`board/prompts/engineering_standard.md`, punti 7 e 8) che la skill non deve ripetere.

**`docs-writer` — Docs Writer** (R9)

- `task`: `Writes or updates documentation (README, guides, comments) for the stated audience and language; does not change behaviour.`
- Kinds `[work]`, fascia `Standard`, effort `medium`. Il punto di valore è la regola "scrivi per il lettore
  dichiarato" (la persona chiede guide per non tecnici), che il Bot generico non ha.

**`refactorer` — Refactorer** (R7)

- `task`: `Restructures existing code without changing its behaviour, in small verified steps, with tests run before and after.`
- Kinds `[work]`, fascia `Strong`, effort `high`: il rischio della persona è alto (non può accorgersi di una
  regressione), quindi qualità massima. Priorità "could": richiesta rara.

**`git-operator` — Git Operator** (R11) — seeded **spento**

- `task`: `Performs git maintenance asked by the user (branch cleanup, conflict resolution, reverting a change) safely: never rewrites shared history or force-pushes.`
- Kinds `[work]`, fascia `Strong` (gli errori costano lavoro perso), effort `medium` (le operazioni sono brevi).
- Perché spento: è l'unico Bot che deve usare comandi git che cambiano stato fuori dal flusso di consegna. Il worker
  gira già in un worktree con istruzioni di consegna (`board/texts.rs`); operazioni come "elimina i branch vecchi"
  toccano il repository condiviso. Verificato nel codice il caso del task che non produce commit: `complete_task` non
  richiede `commit` (`board/report.rs`, `parse_report_fields`), il diffstat "no committed changes"
  (`automation/worktrees.rs`) è solo informativo, l'esito è Done e worktree e branch si puliscono alla chiusura. Nelle
  consegne local merge, commit e push il flusso si conclude pulito; in modalità **pr** invece `gh pr create` fallisce
  ("No commits between ...") se il worker segue alla lettera i passi di consegna (`board/texts.rs`, `delivery_steps`
  e `GIT_DONE`), che non prevedono il caso senza commit. Per Git Operator il caso è coperto dalla skill `git-ops`
  (`agents/presets/git-ops.md`): senza un commit proprio il worker salta commit, push, PR e merge e chiude
  `completed` senza `commit`. Resta aperto per gli altri Bot: una riga per il task senza commit nei passi di
  consegna generici (miglioramento a parte).
- Le domande su git in sola lettura (R3) **non** vanno qui ma a `quick-answer`.

**`release` — Release** (R12) — seeded **spento**

- `task`: `Prepares a release: version bump, changelog and release notes from the merged work; never publishes, tags remotely or deploys without the user's explicit confirmation.`
- Kinds `[work]`, fascia `Strong`, effort `high`. Spento perché gli effetti (pubblicazione, deploy) sono esterni e
  irreversibili e dipendono dal progetto; l'utente lo accende quando lo configura (eventualmente con una skill
  utente con i comandi del suo progetto, vedi §5.3).

### 4.4 Esempi non predefiniti

Da documentare (es. in [BOARD.MD](BOARD.MD) o nella UI dei Bot come "idee"), non da creare:

- **Security Reviewer** (`[review]`, Strong/high, skill utente con la checklist dell'organizzazione): una seconda
  review indipendente; predefinirlo raddoppierebbe il costo di ogni review.
- **Bot su un altro agente** (es. il Reviewer su OpenCode per una review indipendente): già previsto da
  `planner_instructions.md` ("a Bot that uses that agent") e dai Bot derivati (`bots/derived.rs`).
- **Bot di stack** (una skill utente con le convenzioni del progetto): l'utente lo crea con Skill Wizard.

### 4.5 Compatibilità con più agenti

- Nessun preset nomina un modello (`ModelChoice::Tier`), quindi la pool vale con Claude Code e OpenCode; il test
  `presets_are_well_formed` lo garantisce già.
- Su OpenCode l'effort si scarta (§1.4): `quick-answer` (Standard/low) e `test-writer` (Standard/medium) diventano
  equivalenti per costo; accettabile, perché la differenza di esito sta nella skill.
- Le skill nuove vanno spedite nel pacchetto di entrambe le integrazioni (`plugin_skills` di
  `agents/claude_code/plugin_skills.rs` e `agents/opencode/plugin_skills.rs`): avviene da sé per ogni voce di
  `agents::presets::PRESETS` ([BOARD.MD](BOARD.MD), "Skill del plugin e Bot"); verificato nel codice che una skill
  nuova richiede l'aggiornamento del pacchetto su entrambe le integrazioni: la versione del pacchetto dipende dal
  testo di tutte le skill (`version_of` in `agents/opencode/plugin.rs`, `plugin_version` in
  `agents/claude_code/plugin.rs`), quindi la prontezza diventa `Outdated` finché l'utente non lo aggiorna (per
  OpenCode le skill registrate a runtime sono `SKILL_IDS` dell'`index.ts` generato). Nel frattempo l'AgentGate
  (`board/agent_gate.rs`) tiene i task dell'agente in `queued` con il motivo visibile; l'aggiornamento si fa dal
  dialogo dei plugin, che riavvia l'app. Dopo l'aggiornamento vanno riavviate solo le sessioni interattive già
  aperte: i worker partono già con il pacchetto nuovo. All'accettazione (`board/agent.rs`, `resolve_with_bot`) si
  rifiuta un Bot solo se l'agente non sa caricarne la skill (skill sconosciuta alla build), non se la skill non è
  ancora installata.
- Le fasce sono due: se si vuole una fascia economica vera per `quick-answer` serve una terza fascia (`Light`/`Fast`)
  nel contratto (§8, D2).

---

## 5. Pool di skill proposta

### 5.1 Vincoli comuni (verificati nei test di `agents/presets.rs`)

- id kebab-case = nome della cartella e del frontmatter `name`; `description` obbligatoria;
- testo **ASCII**, fine riga LF, in inglese;
- **nessun nome di agente o modello** (test `skills_name_no_agent_and_no_model`, con elenco di nomi vietati);
- le skill dei worker non ripetono lo standard dei worker (`engineering_standard.md`) né le istruzioni di consegna: le
  istruzioni del task hanno la precedenza in caso di conflitto (`texts/skill.rs`);
- **rischio di attivazione spontanea**: le skill del plugin compaiono anche nelle sessioni interattive dell'utente
  (sono nell'elenco delle skill dell'agente come `cli-companion-notify:<id>`). Una descrizione generica ("Use when
  fixing a bug") farebbe caricare `bug-fix` in qualunque sessione. Ogni `description` delle skill da worker deve
  quindi dire che vale **solo per un task di cli-companion il cui Bot la nomina**, come già fanno le skill
  esistenti con "Only works inside a cli-companion terminal".

Struttura comune delle bozze: `## When this applies`, `## Rules`, `## Workflow`, `## Report` (cosa mettere in
`complete_task.summary`).

### 5.2 Skill nuove

**`code-answer`** — per `quick-answer` (task `plan`)

- Scopo: rispondere a una domanda breve leggendo il minimo indispensabile.
- Bozza:

  ```markdown
  ---
  name: code-answer
  description: Answer a short question about this repository or its git history, read-only, for a non-technical user. Only for a cli-companion planning task whose bot names this skill.
  ---
  ## When this applies
  The task asks a question. Nothing has to change in the project.
  ## Rules
  - Read-only: never edit files; git only to read (log, show, diff, blame, status).
  - Do not propose tasks. If the question really asks for a change, answer what you found and
    suggest, in one line, that the user asks for the change.
  - Read the fewest files that answer the question; stop when you can answer.
  ## Report
  Finish with the answer outcome of your instructions: at most 10 lines, plain words, in the language
  of the question; name files only when the user needs them; say what you could not verify.
  ```

**`code-research`** — per `researcher` (task `plan`)

- Scopo: indagine tecnica con esito strutturato (R4), anche R&D di fattibilità.
- Regole chiave: sola lettura; esperimenti solo con build/test i cui output sono ignorati da git (stessa regola di
  `planner_instructions.md`); fonti esterne solo se l'agente ha accesso alla rete e citandole; **report** con
  sezioni `Question`, `Findings`, `Options` (pro, contro, effort relativo), `Recommendation`, `Open points`; proposta
  di task solo se la richiesta la chiede.

**`bug-fix`** — per `bug-fixer` (task `work`)

- Regole chiave: riprodurre prima di toccare il codice (test che fallisce, o passi documentati se non automatizzabile);
  correggere la causa, non il sintomo; la regressione resta come test; se il bug non si riproduce, `ask_user` o
  chiusura `failed` con i passi provati, **mai** una correzione alla cieca; nel report: causa, correzione, test.

**`test-qa`** — per `test-writer` (task `work`)

- Regole chiave: seguire framework e convenzioni esistenti, nessun framework nuovo senza che la specifica lo chieda
  (coerente con le regole E2E di `planner_instructions.md`: "Without a suite, write no scenarios and do not propose to
  set one up"); test deterministici; un test che rivela un bug non si "aggiusta": si segnala in `follow_ups`.

**`docs-writer`** — per `docs-writer` (task `work`)

- Regole chiave: individuare lettore e lingua dalla specifica (default: la lingua della richiesta); per un lettore non
  tecnico niente gergo e passi numerati; verificare ogni comando o percorso citato nel repository; non cambiare
  codice se non nei commenti.

**`safe-refactor`** — per `refactorer` (task `work`)

- Regole chiave: test verdi prima di iniziare (se mancano sul codice toccato, scriverli prima); passi piccoli, ognuno
  verificato; nessun cambio di comportamento, API pubblica o formato dei dati salvo richiesta; nel report l'elenco
  dei passi e la prova dell'equivalenza.

**`git-ops`** — per `git-operator` (task `work`)

- Regole di sicurezza (bloccanti):
  - mai `push --force`/`--force-with-lease` su branch condivisi, mai riscrivere la storia di `main` o del branch base;
  - mai `reset --hard`, `clean -fd`, `branch -D`, `stash drop`, eliminazione di branch remoti **senza** una conferma
    ottenuta con `ask_user` che elenca esattamente cosa si perde;
  - prima di ogni operazione distruttiva, un riferimento di salvataggio (tag o branch locale `backup/<data>`)
    citato nel report;
  - git in un worktree del task, non nel checkout dell'utente, salvo che la specifica lo chieda; mai `stash` sullo
    stack condiviso (stesso rischio documentato per i worktree);
  - nel report: comandi eseguiti, stato prima e dopo, come tornare indietro.

**`release-prep`** — per `release` (task `work`)

- Regole chiave: version bump e changelog dai commit integrati; comandi di build e pubblicazione **solo** quelli
  documentati dal progetto; pubblicazione, tag remoti e deploy solo dopo `ask_user` con il riepilogo; senza
  istruzioni di rilascio nel repository, chiudere `failed` chiedendo di configurarle (eventualmente con una skill
  utente).

### 5.3 Skill esistenti da modificare

- **`task-intake`** (`agents/presets/task-intake.md`): la sezione sui Bot cita `general` / `reviewer` / `planner`
  ([BOARD.MD](BOARD.MD), "Skill del plugin e Bot"); va generalizzata a "scegli fra i Bot con il `kind` giusto per
  `task`", come fa già `planner_instructions.md`, così i Bot nuovi vengono usati anche dall'Intake interattivo.
- **`skill-wizard`** (`agents/presets/skill-wizard.md`): prima di creare una skill, confrontare la richiesta con le
  skill del plugin (elencate dall'app) e suggerire di usare o specializzare un Bot esistente invece di duplicare
  (es. "le nostre regole sui bug" = skill utente da dare a un Bot derivato da Bug Fixer, non una copia di `bug-fix`).
  **Fatto (T8)**: `list_skills` restituisce solo le skill dell'utente; le skill del plugin (nome e descrizione)
  sono nel campo `get_workspace_settings.plugin_skills` (`automation/settings.rs`), che la skill legge insieme ai
  `bots` (sezione "Do not duplicate a plugin skill").

### 5.4 Come evitare duplicazioni con le skill utente

- Spazi dei nomi separati: le skill del plugin si caricano come `cli-companion-notify:<id>`, quelle dell'utente con il
  loro nome (`worker_skill_invocation`), quindi non collidono mai nel caricamento.
- Un Bot ha **una** skill (`AgentBot.skill: Option<SkillRef>`): per combinare regole del progetto e skill del plugin
  l'utente crea una skill utente che le include; Skill Wizard lo suggerisce (§5.3).
- I preset non dipendono mai da skill utente (test `presets_are_well_formed`: la skill deve essere del plugin).

---

## 6. Matrice di instradamento

Segretario = quello che l'utente probabilmente menziona; tra parentesi l'alternativa. "Plan→Work" = un task `plan` che
propone task `work`, eseguiti dopo l'accettazione.

| # | Richiesta | Segretario | Tipo di task | Bot | Skill | Fascia / effort |
|---|---|---|---|---|---|---|
| R1 | Stato del progetto | Cronista (Coordinatore) | nessuno | — | — | modello del segretario |
| R2 | Domanda veloce sul codice | Coordinatore (Analista) | `plan`, esito Answer | `quick-answer` | `code-answer` | Standard / low |
| R3 | Domanda su git / storia | Coordinatore | `plan`, esito Answer | `quick-answer` | `code-answer` | Standard / low |
| R4 | Ricerca / R&D / esplorazione | Coordinatore (Analista) | `plan`, esito Answer (o proposta se chiesta) | `researcher` | `code-research` | Strong / high |
| R5 | Sviluppo feature | Coordinatore (Analista se vaga) | Plan→Work (+ review se ≥2 task) | `planner` → `general` (+ `reviewer`) | — | Strong/high → Default (+ Strong/high) |
| R6 | Correzione bug | Coordinatore | Plan→Work + review | `planner` → `bug-fixer` + `reviewer` | `bug-fix` | Strong/high → Strong/high |
| R7 | Refactoring | Coordinatore | Plan→Work + review | `planner` → `refactorer` + `reviewer` | `safe-refactor` | Strong/high → Strong/high |
| R8 | Test / QA | Coordinatore | Plan→Work | `planner` → `test-writer` | `test-qa` | Strong/high → Standard/medium |
| R9 | Documentazione | Coordinatore | Plan→Work | `planner` → `docs-writer` | `docs-writer` | Strong/high → Standard/medium |
| R10 | Review | Coordinatore | Plan→Review (o review di obiettivo automatica) | `planner` → `reviewer` | — | Strong/high |
| R11 | Operazioni git | Coordinatore | Plan→Work | `planner` → `git-operator` (se acceso; altrimenti il pianificatore chiede di accenderlo) | `git-ops` | Strong/high → Strong/medium |
| R12 | Release / deploy | Coordinatore | Plan→Work | `planner` → `release` (se acceso) | `release-prep` | Strong/high → Strong/high |
| R13 | Gestione della coda | Operatore (opt-in) | nessuno (azione o scheda di conferma) | — | — | modello del segretario |
| R14 | Setup / configurazione | nessuno: il segretario indica il tab | sessione interattiva | `setup` | `project-setup` | Standard / medium |
| R15 | Creazione di skill | nessuno: il segretario indica il tab | sessione interattiva | `skill-wizard` | `skill-wizard` | Standard / medium |
| R16 | Idea vaga | Analista (Coordinatore con una domanda) | intervista → Plan→Work | `planner` → come R5 | — | come R5 |

Note:

- R11 e R12 con il Bot spento: il pianificatore, per le sue istruzioni ("If the Bot you would need is disabled or not
  ready, do not propose: complete with the question for the user"), chiude con una domanda che nomina il Bot. È il
  comportamento voluto: l'utente accende consapevolmente il Bot rischioso.
- R14 e R15 restano fuori dall'HUB per scelta (le skill sono interattive e propongono impostazioni da approvare).
  Il Coordinatore deve saperlo dire: oggi nessun prompt di segretario lo menziona (§7, T6).

---

## 7. Piano di adozione

### 7.1 Cosa succede a chi ha già i file

| Elemento | Utente nuovo | Utente esistente |
|---|---|---|
| Bot nuovi | creati dal passo di migrazione v4 (il file nuovo passa da tutti i passi) | creati **una volta** dal passo v4 se l'id manca; nomi liberi come "Researcher 2" se presi |
| `task` cambiati di `general` / `planner` | dal preset | aggiornati **una volta** dal passo v5 solo se il `task` è ancora il vecchio testo del preset; un testo personalizzato resta (§8, D4) |
| Segretari (prompt cambiati) | dal seeding | **invariati** finché l'utente non usa «Ripristina predefinito» |
| Skill del plugin nuove | nel pacchetto dell'agente | al prossimo aggiornamento del pacchetto (prontezza `outdated`) |
| Task in coda | — | invariati (snapshot del Bot) |

### 7.2 Task di implementazione (ordinati)

Ogni task è pensato per diventare una specifica per un worker. Dimensione: S (< mezza giornata di worker), M, L.

**T1 — Esito "Answer" dei task di pianificazione** · must · M · nessuna dipendenza

- `src-tauri/src/board/prompts/planner_instructions.md`: terzo esito di `complete_task`: se la richiesta è una
  domanda (o la skill del Bot lo chiede), nessuna proposta, `summary` che inizia con "Answer:".
- `src-tauri/src/hub/events/texts.rs` (`plan_without_proposal`): titolo diverso per un report che inizia con
  "Answer:" (es. «Risposta: <titolo>») invece di «Pianificazione conclusa senza proposta».
- `src/lib/plan.ts`: `planQuestions` non deve trattare le risposte come domande; eventuale `planAnswer` e badge
  «Risposta» nel dettaglio.
- Test: `hub/events/texts.rs`, `src-tauri/src/automation/tests/plan.rs`, test del frontend di `plan.ts`.
- Rischio: le istruzioni del pianificatore sono condivise da tutti i Bot `plan`; l'esito Answer non deve diventare
  una scorciatoia del Planner per non proporre (regola: solo se la richiesta non chiede modifiche).

**T2 — Skill del plugin nuove** · must (`code-answer`, `bug-fix`), should/could (le altre) · M · nessuna dipendenza

- Un file per skill in `src-tauri/src/agents/presets/<id>.md` (bozze in §5.2) e una voce in
  `agents::presets::PRESETS`.
- I test di `agents/presets.rs` (ASCII, nomi vietati, frontmatter) coprono già i vincoli; aggiornare i test che
  contano le skill (es. `bots/skills.rs` usa `PLUGIN_SKILLS.len()`: si adatta da solo).
- Verificare il pacchetto delle due integrazioni (cartelle `skills/<id>/` del plugin Claude Code e comandi di
  OpenCode) e la prontezza `outdated` dopo l'aggiornamento.

**T3 — Bot nuovi: preset e migrazione v4** · must · M · dipende da T2 (le skill devono esistere: test
`presets_are_well_formed`)

- `src-tauri/src/bots/presets.rs`: nuove voci in `PRESETS` (array a dimensione fissa: da 6 al nuovo numero), con i
  campi di §4.3; aggiornare `the_presets_come_in_order`, `each_preset_has_the_model_effort_and_skill_of_its_job`,
  `each_preset_runs_only_the_task_kinds_of_its_job`.
- `BotPreset` non ha un campo `enabled`: per i Bot spenti serve un campo `enabled_by_default: bool` (o equivalente)
  usato da `BotPreset::bot()`, oppure si accettano tutti accesi (§8, D5).
- `src-tauri/src/bots/migrations.rs`: passo congelato `add_domain_bots` (3→4) che aggiunge i Bot con JSON scritto per
  esteso (non da `PRESETS`, per la regola dei passi congelati), salta gli id presenti e usa `free_name`.
  **Attenzione**: `free_name` di `migrations.rs` confronta solo i nomi dei Bot, mentre `agent_bots::ensure_free`
  vieta anche i nomi dei segretari; un segretario chiamato `researcher` renderebbe il Bot "Researcher" non salvabile
  dall'editor (`ensure_free` confronta in minuscolo). I nomi con uno spazio ("Quick Answer", "Bug Fixer") non
  possono mai essere handle di segretario; quelli di una parola ("Researcher", "Release", "Refactorer") sì: il passo
  deve ricevere anche i nomi dei segretari (oggi non li legge) oppure i preset devono usare nomi con uno spazio.
- `src-tauri/src/bots.rs`: aggiungere il passo all'elenco; `bots/tests.rs`: `schema_version` 4, fixture v3 → v4,
  test "un Bot cancellato non ricompare", test `the_steps_match_the_presets_of_the_current_release`.
- Bindings ts-rs: nessun tipo cambia se non si aggiunge un campo a `AgentBot`; con `enabled_by_default` solo il
  preset cambia (Rust interno), non `AgentBot.ts`.

**T4 — `task` più esclusivi per `general` e `planner`** · must · S · dipende da T3 (stesso file e stessi test)

- Testi di §4.2 in `bots/presets.rs` e decisione D4 per gli utenti esistenti (nessun passo, o un passo che aggiorna il
  `task` solo se è ancora identico al vecchio testo del preset: sicuro, perché un testo uguale al vecchio preset non
  è una personalizzazione).

**T5 — Prompt del Coordinatore e dell'Analista** · must · S · dipende da T1 e T3 (il routing nomina categorie di Bot
che devono esistere)

- `src-tauri/src/hub/settings/presets/coordinator.md` e `analyst.md` (bozza in §3.1); test di `presets.rs`
  (lunghezza, ASCII, "the bots you are given", "kinds", niente `` `general` ``).
- Avviso nella UI dei segretari quando il prompt di un segretario nato da preset differisce dal preset corrente
  (proposta: «È disponibile una versione aggiornata: Ripristina predefinito»), così gli utenti esistenti ricevono il
  routing. Richiede di confrontare `Secretary.system_prompt` con `SecretaryPreset::prompt_text()`: un campo
  calcolato nella vista o un confronto nel frontend con `HubSecretaryPreset.system_prompt` (già esposto da
  `presets::views`). Nessun binding nuovo nel secondo caso.

**T6 — Il segretario indica i tab per setup e skill** · should · S · dipende da T5

- Una frase nel prompt del Coordinatore: per configurare il workspace o creare una skill, aprire dal pannello dei
  terminali il Bot Setup o Skill Wizard. Nessun tool nuovo.

**T7 — `task-intake` generico sui Bot** · should · S · dipende da T3

- `src-tauri/src/agents/presets/task-intake.md`: scelta del Bot per `kind` e `task`, come `planner_instructions.md`;
  aggiornare la sezione di [BOARD.MD](BOARD.MD).

**T8 — Skill Wizard e skill del plugin** · could · M · dipende da T2

- Esporre le skill del plugin al Wizard (campo in `get_workspace_settings` o elenco nel testo della skill) e regola
  anti-duplicazione di §5.3.

**T9 — Tool `open_question_task` con limite nel codice** · could · L · dipende da T1, T3

- Solo se si vuole un segretario che non possa mai avviare modifiche (§3.5): un flag sul Bot (es. `answers_only`) o
  un `kind` dedicato, con `bot_choice::check` che lo verifica. Impatta `AgentBot` (binding `AgentBot.ts`),
  migrazione, UI dei Bot. Da non fare prima di aver misurato l'uso di T1–T5.

**T10 — Fascia economica** · could · L · indipendente

- `ModelTier::Light` (o `Fast`) nel contratto `agents/contracts/models.rs`, voci suggerite per ogni integrazione,
  `tier_models` con tre fasce, bindings ts-rs di `ModelTier`, UI dell'editor dei Bot, `planner_instructions.md`
  (testo sulle fasce). Poi `quick-answer` passa a `Light`.

**T11 — Documentazione** · must · S · dopo T1–T5

- [BOARD.MD](BOARD.MD) ("Skill del plugin e Bot", "Agente e modello di un task") e [HUB.MD](HUB.MD) ("Segretari
  predefiniti", "Pianificazione dal thread") aggiornati; esempi non predefiniti di §4.4.

Ordine consigliato: T2 → T1 → T3 → T4 → T5 → T11 (il nucleo "must"), poi T6, T7, T8, e solo dopo un periodo d'uso T9
e T10. Ogni passo lascia l'app coerente: le skill senza Bot sono innocue, l'esito Answer senza `quick-answer` non è
mai scelto dal Planner, i Bot senza prompt aggiornati sono comunque scelti dal pianificatore.

### 7.3 Rischi aperti

- **Qualità della scelta del Bot da parte di un modello locale**: il segretario può girare su un modello piccolo
  (provider OpenAI-compatibile locale); con tre Bot `plan` la scelta deve restare banale (`task` esclusivi, verbo
  iniziale). Da provare con il modello che si consiglia nella documentazione.
- **Attivazione spontanea delle skill da worker** nelle sessioni interattive (§5.1): descrizioni da rivedere con cura
  e da provare in una sessione normale.
- **Esito Answer abusato** dal Planner per evitare di proporre (T1): regola stretta e test sulle istruzioni.
- **Task `work` senza commit in modalità pr**: chiuso per `git-operator` (la skill `git-ops` fa saltare commit,
  push, PR e merge e chiudere `completed` senza `commit`); aperto per i testi di consegna generici in modalità pr
  (`board/texts.rs`), dove `gh pr create` fallisce se un altro Bot chiude senza commit seguendo i passi alla lettera
  (§4.3). Local merge, commit e push si concludono puliti.
- **Costo**: 12 Bot accesi non costano nulla finché non sono scelti, ma `bug-fixer` e `refactorer` sono `strong/high`
  dove prima il pianificatore poteva scegliere `general` con il modello del workspace: il costo medio di un bug sale.
  È voluto (qualità), ma va detto nella documentazione.

---

## 8. Decisioni aperte

| # | Decisione | Opzioni | Raccomandazione |
|---|---|---|---|
| D1 | Come rispondere alle domande sul codice | (a) esito "Answer" dei task `plan` + Bot `quick-answer`/`researcher`; (b) nuovo `TaskKind` `ask`; (c) tool nuovo del segretario che lancia un agente senza task | **(a)**: riusa worktree in sola lettura, report intero nel thread, scelta del Bot esistente; (b) tocca `TaskKind`, ruoli, migrazioni dei Bot e UI per un guadagno solo di chiarezza; (c) rompe il principio "tutto passa dalla board" |
| D2 | Fascia economica per le domande | (a) restare su `Standard` + effort `low`; (b) aggiungere `Light` (T10) | **(a) ora, (b) dopo** aver misurato quante domande arrivano: (b) cambia un contratto condiviso da tutte le integrazioni |
| D3 | Operatore seeded? | (a) opt-in come oggi; (b) seeded | **(a)**, con un suggerimento nella UI al primo avvio («Vuoi un segretario che gestisca la coda?»): il test sui preset seeded esprime una regola di sicurezza che vale ancora |
| D4 | `task` aggiornati per chi ha già `general` e `planner` | (a) nessuna migrazione (solo ripristino); (b) passo che aggiorna il testo solo se identico al vecchio preset | **(b)**: nessuna personalizzazione persa e routing corretto per tutti; senza, gli utenti esistenti avrebbero due Bot `plan` con testi sovrapposti |
| D5 | Bot rischiosi (`git-operator`, `release`) | (a) seeded spenti; (b) non predefiniti, solo esempi; (c) seeded accesi | **(a)**: visibili e pronti, ma l'utente li accende di proposito; il pianificatore spiega perché servono. Richiede `enabled_by_default` nel preset (T3). Se si vuole evitare il campo: **(b)** |
| D6 | Bot "feature" separato da General | (a) no, General è il Bot delle feature; (b) sì | **(a)**: due Bot uguali rendono casuale la scelta |
| D7 | Segretario dedicato alle domande veloci | (a) no, Coordinatore esteso; (b) preset opt-in `scout`; (c) preset seeded | **(a)** (motivi in §3.5); rivalutare con T9 se serve un limite vero |
| D8 | Aggiornare i segretari esistenti | (a) solo «Ripristina predefinito»; (b) avviso "versione aggiornata" nella UI (T5); (c) migrazione automatica dei prompt | **(b)**: (c) sovrascriverebbe prompt che l'utente può aver personalizzato, e oggi nulla distingue un prompt intatto da uno modificato se non il confronto testuale |
| D9 | Quante skill nuove nella prima release | (a) tutte le 7; (b) solo `code-answer` e `bug-fix`, le altre dopo | **(b)** come prima tappa, perché coprono le due richieste più frequenti (R2, R6) e il rischio di attivazione spontanea (§5.1) si verifica su due skill prima che su sette |
