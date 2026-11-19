# Goal (proposta di design)

> **Stato: in corso.** Le tappe T1, T2, T3 e T6 sono consegnate (vedi "Tappe"); il resto non è implementato. Descrive
> come aggiungere al Tasks Manager il **goal** (obiettivo) come oggetto tracciato sopra i task, con un branch suo e
> una consegna sua. Fa fede [BOARD.MD](BOARD.MD) per tutto ciò che esiste oggi (per i goal: "Goal").

Documenti collegati: [BOARD.MD](BOARD.MD) (task, proposte, scheduler, consegna, tool MCP, review),
[DESIGN.MD](DESIGN.MD) (mappa dei moduli).

Convenzione: i testi che legge il modello (descrizioni dei tool, istruzioni, errori) sono citati **in inglese**;
tutto il resto è in italiano.

## Perimetro e decisioni prese

- **Fuori perimetro: l'Agents HUB.** Segretari, thread, `open_plan_task` e i task `plan` non cambiano e non
  ricevono goal. L'HUB continua a funzionare come oggi leggendo la `BoardView`, che guadagna solo campi nuovi.
- **`ask_user` resta com'è** (solo con l'HUB acceso). Senza HUB il worker chiede nel suo terminale e la risposta
  non viene registrata: il registro delle decisioni del goal non la contiene.
- **Ciclo di correzione automatico: opzione per goal, con un tetto di giri.** Di serie ogni proposta la decide
  l'utente, come oggi.
- **Ogni goal lavora su un branch suo.** I task del goal partono da quel branch e vi integrano il loro lavoro.
- **Il goal è l'unità di consegna.** A lavoro finito un task di consegna creato dall'app porta tutto il lotto
  sulla base. Di serie parte con un pulsante; per goal si può rendere automatico.
- **Un goal senza review si consegna come gli altri:** stesso branch, stesso task di consegna, senza verifica.

## 1. Perché

Oggi la board traccia task, non obiettivi. L'obiettivo esiste in modo implicito: nel testo "Goal" ripetuto nelle
specifiche e nei `depends_on` della review. Le conseguenze:

1. Nessuno, utente o agente, può chiedere "a che punto è l'obiettivo X, cosa manca, cosa è stato deciso".
2. **Modifiche diverse si mescolano.** Con due o tre Intake aperti su modifiche diverse, tutti i worker partono
   dalla stessa base (quasi sempre `main`) e vi consegnano: il lavoro di una modifica entra in quello delle altre.
3. **La consegna del lotto è a mano.** Finiti i task, l'utente deve ricordarsi di chiedere a un agente la PR e
   il merge dell'insieme, e di farsi dire cosa è stato fatto.
4. La review verifica il lavoro quando è già sulla base.
5. `failed` copre casi diversi (il worker ha rinunciato, l'utente ha fermato, l'app si è riavviata, l'avvio è
   fallito), distinti solo da una nota in testo libero.
6. Un task `queued` che dipende da un task `failed` aspetta senza fine, e la vista dice solo "attende X".
7. Il ciclo review → correzioni → nuova review è tenuto insieme dal solo `source_task` della proposta: non c'è un
   contatore dei giri né un punto in cui l'obiettivo risulta chiuso.
8. I `follow_ups` dei report li legge solo il reviewer; poi si perdono.
9. Un task `work` che dipende da un altro riparte dalla sola specifica, scritta prima che il predecessore
   esistesse: i report delle dipendenze arrivano solo ai reviewer (`reviewed_tasks`).
10. Ciò che aspetta l'utente è sparso: proposte, falliti, "Richiede attenzione", ognuno nella sua sezione.

## 2. Principi

- **Lo scheduler non decide nulla di nuovo.** Il goal raggruppa, isola e racconta; l'ordine di partenza resta
  quello della coda e delle dipendenze.
- **La fase di un goal è derivata** dai suoi task e dalle sue proposte con una funzione pura. Nessun LLM la
  scrive, nessun tool la imposta.
- **Il flusso di un goal è lineare:** eseguire → verificare → consegnare → chiuso. Dentro "eseguire" i task
  formano il solito grafo di dipendenze.
- **Due livelli soltanto:** goal e task. Niente gerarchie, niente workflow configurabili.
- **Un LLM non alza mai la propria autonomia.** Correzioni automatiche e consegna automatica le imposta solo
  l'utente, dall'UI.
- **La base riceve solo lotti interi.** Niente di un goal arriva sulla base prima della sua consegna.
- **Il goal è facoltativo.** Un task senza goal resta un task sciolto e si comporta come oggi, consegna compresa.

## 3. Modello dati

La board guadagna `goals` (schema della board v2, vedi "Migrazione").

```jsonc
{
  "id": "export-ordini",        // stesso formato degli id dei task, unico fra i goal della board
  "title": "Export degli ordini in Excel",   // 1..=80
  "request": "...",             // la richiesta dell'utente, il più possibile con le sue parole; 1..=4000
  "done_when": ["..."],         // criteri di riuscita dell'obiettivo; max 20, 1..=300 ciascuno
  "decisions": [                // registro delle decisioni; max 50, le più vecchie cadono
    { "at": 0, "source": "intake", "text": "..." }   // source: intake | user | app; text 1..=500
  ],
  "branches": {                 // un branch per repository toccato; chiave = `repo` del task ("" = la root)
    "": { "branch": "goal/export-ordini", "base": "main" }
  },
  "auto_fix": { "max_rounds": 0 },   // 0..=3; 0 = ogni proposta la decide l'utente (default)
  "auto_deliver": false,        // true = la consegna parte da sola appena il goal è pronto
  "round": 1,                   // 1 = il lavoro iniziale; +1 a ogni proposta di correzione accettata
  "created_at": 0,
  "closed_at": null,            // presente = chiuso
  "outcome": null,              // solo se chiuso: delivered | abandoned
  "review": null,               // solo se chiuso: passed | issues | none (esito dell'ultima review)
  "report": null,               // solo se chiuso e consegnato: { summary, commits, pr_urls }
  "follow_ups": []              // solo se chiuso: vedi "Follow-up"
}
```

Task: campi nuovi, tutti omessi se assenti.

```jsonc
{
  "goal": "export-ordini",      // id del goal; assente = task sciolto
  "round": 1,                   // giro del goal in cui il task è entrato in coda
  "fail_reason": "worker_failed" // solo per i task failed, vedi "Motivo del fallimento e blocco"
}
```

`kind` guadagna `deliver` (vedi "Consegna del goal"): come `plan`, lo crea solo l'app.

Proposal: un campo nuovo, uno dei due.

```jsonc
{ "goal": { "id", "title", "request", "done_when", "decisions"? } }   // goal nuovo, creato all'accettazione
{ "goal_id": "export-ordini" }                                         // task da aggiungere a un goal aperto
```

Regole:

- Un task appartiene al più a un goal. Tutti i task di una proposta hanno lo stesso goal.
- **Un task di un goal dipende solo da task dello stesso goal** (`task '<id>': depends on '<x>', which belongs to
  another goal`): con branch separati non troverebbe il codice da cui dipende. Un task sciolto non dipende da task
  di un goal aperto, per lo stesso motivo.
- Un goal chiuso non si riapre: il lavoro successivo è un goal nuovo.
- Limiti: 50 goal aperti per board; i goal chiusi oltre i 100 più recenti vanno in
  `<app_data>/boards/archive/<workspace_id>.goals.jsonl`, come i task.
- I task di un goal **aperto** non si archiviano (si aggiunge alle eccezioni di `take_archivable`).

## 4. Come nasce un goal

| Origine | Goal |
|---|---|
| Proposta dell'Intake con `goal` | creato quando l'utente accetta; i task ricevono `goal` e `round: 1` |
| Proposta dell'Intake con `goal_id` | i task entrano nel goal esistente, con il `round` corrente |
| Proposta di un reviewer (`source_task` = una review) | **ereditato dall'app** dal task di review: il reviewer non indica nulla |
| Task creato a mano | facoltativo: il dialog "Nuovo task" offre i goal aperti |
| Task `plan` (HUB) | nessuno |

Una proposta dell'Intake senza `goal` né `goal_id` resta valida e crea task sciolti: così i file e le sessioni
esistenti non si rompono. La skill `task-intake` dirà di indicare sempre un goal, anche per un task solo: è il
goal che dà alla modifica il suo branch e la sua consegna.

Scartare una proposta con `goal` nuovo non crea nulla. Accettarla non esegue git: i branch nascono all'avvio dei
task (vedi "Branch del goal").

## 5. Fase e attesa (derivate)

La vista di un goal (`GoalView`) aggiunge al goal due valori calcolati.

**`phase`**, lineare:

| Fase | Quando |
|---|---|
| `executing` | almeno un task `work` del goal non è `done` |
| `verifying` | tutti i task `work` sono `done` e una review del goal è `queued` o `running` |
| `deciding` | nessun task aperto, ma una proposta del goal è pendente |
| `ready` | nessun task aperto, nessuna proposta pendente, consegna non ancora partita |
| `delivering` | il task di consegna è `queued`, `running` o `failed` |
| `closed` | `closed_at` presente |

**`waiting`**, l'elenco di ciò che aspetta l'utente (vuoto = il goal procede da solo):

| Voce | Quando |
|---|---|
| `proposal` | una proposta del goal è pendente e non sarà accettata in automatico |
| `round_limit` | la proposta pendente è di correzione e i giri automatici sono finiti |
| `deliver` | il goal è `ready` e la consegna non parte da sola |
| `failed` | un task del goal è `failed` (anche quello di consegna) |
| `attention` | un task del goal ha `attention: true` |

**Chiusura.** L'app chiude il goal (scrive `closed_at`, `outcome`, `review`, `report`) in due soli casi:

| Situazione | `outcome` |
|---|---|
| il task di consegna è `done` | `delivered` |
| l'utente ha chiuso il goal a mano | `abandoned` |

"Chiudi goal" a mano elimina i task `queued` e `failed` del goal con la stessa regola tutto-o-niente di
`delete_cascade` (errore se uno è `running`) e scarta le sue proposte pendenti. I branch del goal **restano**: il
dettaglio li elenca con "Elimina branch", perché l'app non butta mai lavoro da sola.

La chiusura è l'unico stato del goal scritto su disco: serve a sopravvivere all'archiviazione dei suoi task.

## 6. Branch del goal

Ogni goal ha, per ogni repository in cui lavora, un branch `goal/<goal-id>`.

**Creazione.** La fa l'app, all'avvio del primo task del goal in quel repository, nel percorso di avvio che già
esegue git fuori dal lock. La base si risolve con la regola di oggi (BOARD.MD, "Branch base") e si scrive in
`goal.branches`: da quel momento non cambia. In modalità `pr` il branch parte da `origin/<base>` dopo il fetch,
altrimenti dal `<base>` locale.

**Task del goal.** Girano **sempre in un worktree**, in ogni modalità di consegna, come già fanno i `plan`:
`<repoDir>/.worktrees/<task-id>`, branch `task/<task-id>` creato da `goal/<goal-id>`. Hook `setup` e
`copy_ignored` come oggi. Conseguenze:

- la copia di lavoro dell'utente non viene mai toccata dai task di un goal;
- i task di un goal non occupano il repository nello scheduler e valgono fino a `max_parallel`, anche nelle
  modalità che oggi ammettono un solo worker per repository. Le dipendenze restano l'unico vincolo d'ordine;
- due goal non si vedono: ogni task trova nella sua base solo il lavoro dei task `done` del suo goal.

**Integrazione del task.** Le istruzioni di consegna dei task di un goal non dipendono dalla modalità del
workspace e sono una sola variante:

> Commit your work on `task/<id>`. Rebase it on `goal/<goal-id>` (resolve conflicts, repeat the verification).
> Fast-forward `goal/<goal-id>` to your commit without checking it out. If the fast-forward is refused because the
> goal branch moved, rebase and try again. Never push, never open a pull request, never touch `<base>`: the goal is
> delivered as a whole later.

L'avanzamento deve essere solo fast-forward e senza checkout del branch del goal (per esempio un push locale
`task/<id>:goal/<goal-id>`); il comando esatto e il suo comportamento quando l'utente ha il branch del goal in
checkout vanno verificati in implementazione. Il passo E2E dei task marcati `e2e` resta dov'è oggi: dopo il rebase,
prima dell'avanzamento.

**Pulizia del task.** A `done`: worktree rimosso e `task/<id>` cancellato dopo aver verificato che è contenuto in
`goal/<goal-id>`. Un task `failed` conserva worktree e branch per il retry, come oggi.

**Review.** Il worktree della review parte da `goal/<goal-id>`: lo stato integrato da verificare è quel branch e
il diff del goal è `<base>...goal/<goal-id>`. Le istruzioni del reviewer lo dicono al posto di `origin/<base>`.
La review avviene quindi **prima** che il lavoro arrivi sulla base, giri di correzione compresi.

**Cartelle senza git.** Un goal in una cartella senza git non ha branch né consegna: i suoi task lavorano come
oggi (modalità libera) e il goal passa da `ready` a chiuso (`delivered`, senza `report`) senza task di consegna.

**Cosa non cambia.** I task sciolti: worktree o checkout, base e consegna secondo la modalità del workspace.

## 7. Consegna del goal

Quando il goal diventa `ready` l'app può creare il **task di consegna**: `kind: "deliver"`, uno per goal, creato
solo dall'app (mai da `propose_tasks`, `task_create`, `update_task`), che entra direttamente in coda in cima.

**Quando parte.**

| Situazione | Cosa succede |
|---|---|
| `auto_deliver: true` e l'ultima review è `passed`, o il goal non ha review | il task di consegna si crea da solo |
| `auto_deliver: false` (default) | `waiting: deliver`; l'utente preme **Consegna** |
| l'ultima review è `issues` e le correzioni sono state scartate o mancano | mai da solo: il pulsante è **Consegna comunque** |

`auto_deliver` lo cambia solo l'utente, dalla card della proposta e dal dettaglio del goal.

**Cosa fa il worker.** Gira in un worktree sul branch `goal/<goal-id>` e riceve da `get_task` il goal (richiesta,
criteri, decisioni) e i report di tutti i suoi task. Per ogni repository in `goal.branches`:

1. riallinea `goal/<goal-id>` sulla base aggiornata e risolve i conflitti; se non riesce, annulla il rebase e
   chiude `failed` dicendo quali file sono in conflitto;
2. ripete la verifica dell'obiettivo (i comandi "Verify" delle specifiche, letti con `get_task_details`) e, se
   un task del goal era marcato `e2e`, la suite E2E con il blocco di oggi;
3. porta il lotto sulla base secondo la **modalità di consegna del workspace**, che per i goal si applica qui e
   una volta sola:

   | Modalità | Passo finale |
   |---|---|
   | `pr` | push di `goal/<goal-id>`, una PR verso `<base>` con il titolo del goal e il riepilogo dei task, merge con `merge_method`, cancellazione del branch remoto |
   | `local_merge`, `commit` | fast-forward del `<base>` locale; se il checkout dell'utente lo impedisce, `failed` con il motivo |
   | `push` | fast-forward del `<base>` locale, poi `git push` |

4. chiude con `complete_task`: riepilogo di cosa è stato consegnato, commit e link delle PR.

**Dopo.** Con il task di consegna `done` l'app chiude il goal (`delivered`), copia il report in `goal.report`,
cancella i branch `goal/<goal-id>` locali e notifica "Goal consegnato: <titolo>". Il riepilogo è ciò che oggi
l'utente deve chiedere a mano. Se il task fallisce il goal resta `delivering` con `failed` in `waiting`;
"Riprova" lo rilancia sullo stesso branch.

**Chi lo esegue.** Un Bot con `work` fra i `kinds`: General di serie, modificabile nel dettaglio del goal. Le
istruzioni sono proprie del `kind` (come per reviewer e planner), non una skill.

## 8. Giri e correzione automatica

Il flusso di oggi resta: il reviewer con `verdict: issues` propone i task di correzione e una nuova review
(vedi BOARD.MD, "Task di review"). Cambia cosa fa l'app quando arriva quella proposta.

1. La proposta eredita il goal della review.
2. Se `round - 1 < auto_fix.max_rounds` e la proposta è **ammissibile**, l'app la accetta da sola, con la stessa
   validazione dell'accettazione manuale. I task entrano con `round + 1` e il goal avanza di un giro.
3. Altrimenti la proposta resta pendente: `waiting` riporta `round_limit` (giri finiti) o `proposal`.

Una proposta è ammissibile all'accettazione automatica solo se:

- viene dal reviewer di una review di quel goal;
- contiene solo task `work` e `review`, nei repository già usati dal goal;
- non usa nessun Bot spento di serie (`enabled_by_default: false`: Git Operator, Release), anche se l'utente lo
  ha acceso;
- la validazione riesce. Se fallisce, la proposta resta pendente con l'errore, come oggi sulla card.

`auto_fix.max_rounds` lo cambia solo l'utente: sulla card della proposta al momento di accettare ("Correzioni
automatiche: no / 1 / 2 / 3 giri") e nel dettaglio del goal. Non è un campo di `propose_tasks` né di
`propose_settings`. Ogni accettazione automatica aggiunge una riga a `decisions` con `source: "app"` ("Round 2
accepted automatically: 3 tasks"), così resta scritto cosa è partito senza l'utente.

I giri di correzione lavorano sullo stesso branch del goal: la base non vede nulla finché il goal non è
consegnato. Con `auto_fix` e `auto_deliver` attivi un goal va dalla proposta accettata alla base senza altri
interventi, entro il tetto dei giri.

Una review `failed` (non eseguita) non avanza nulla: il goal riporta `failed` in `waiting`.

## 9. Motivo del fallimento e blocco

`fail_reason` sul task, scritto dalla transizione che lo porta a `failed`:

| Valore | Transizione di oggi |
|---|---|
| `worker_failed` | `complete_task` con `status: "failed"` |
| `worker_exited` | tab chiuso o agente uscito senza report |
| `stopped` | utente: Ferma |
| `interrupted` | riavvio dell'app |
| `start_failed` | git, setup, precondizione, skill non più esistente |

La `note` resta, per il dettaglio. Il campo si azzera con "Riprova".

`blocked_by: string[]` nella vista del task (`TaskView` e righe di `list_tasks`): per un task `queued`, i task
`failed` da cui dipende, direttamente o no. È calcolato con l'indice delle dipendenze inverse che già serve a
`cascade`. Diverso da `waiting_on`, che resta l'elenco delle dipendenze non `done`. Badge: "bloccato da X".

Vale per tutti i task, anche sciolti. Un goal con un task bloccato ha già `failed` in `waiting`.

## 10. Passaggio di consegne

`get_task` di un task `work` guadagna due campi, omessi se vuoti.

- `dependencies`: per ogni dipendenza diretta `done`, la stessa forma di `reviewed_tasks`
  (`{ id, title, summary, commit, pr_url, files_changed, follow_ups, truncated? }`), con gli stessi limiti.
- `goal`: `{ title, request, done_when, decisions }` del goal del task.

Riga aggiunta alle istruzioni comuni, solo se uno dei due campi c'è:

> Read `dependencies` and `goal` before starting. The tasks you depend on are delivered and already in your base:
> build on what they did, do not redo it. The decisions listed in `goal` are final.

Il reviewer e il worker di consegna ricevono lo stesso `goal` in `get_task`.

Il registro `decisions` lo alimentano:

- l'Intake, con le assunzioni e le scelte dell'utente al momento della proposta (campo `decisions` del `goal`);
- l'utente, con una nota aggiunta dal dettaglio del goal;
- l'app, con una riga per ogni giro accettato in automatico.

## 11. Follow-up

Alla chiusura l'app raccoglie in `goal.follow_ups` i `follow_ups` dei report dei task del goal e i `findings`
delle review con `verdict: passed` (i suggerimenti), senza doppioni:

```jsonc
{ "text": "...", "source_task": "export-csv", "state": "open" }   // open | dismissed | promoted
```

Si copiano perché i task verranno archiviati. Dall'UI ogni voce si può **scartare** o **mandare all'Intake**
(apre il tab dell'Intake con la voce e il riferimento al goal come richiesta iniziale; lo stato diventa
`promoted`). Nessuna voce diventa un task da sola.

## 12. Tool MCP

| Metodo | `worker` | `reviewer` | `planner` | `session` |
|---|:-:|:-:|:-:|:-:|
| `list_goals` | no | no | sì | sì |
| `get_goal` | sì (solo `deliver`) | sì | sì | sì |
| `get_task_details` | sì (solo `deliver`) | sì | sì | sì |

- `list_goals { open_only? }` → righe `{ id, title, phase, round, waiting, tasks: { done, total } }`.
- `get_goal { id }` → il goal con `phase`, `waiting`, `branches` e i suoi task (`id`, `title`, `status`, `kind`,
  `round`).
- `propose_tasks` accetta `goal` o `goal_id` (vedi "Modello dati"). Errori: `unknown goal: <id>`, `goal <id> is
  closed: propose a new goal`, `goal id already in use: <id>`, più quello sulle dipendenze fra goal.
- `list_tasks`: ogni riga guadagna `goal`, `fail_reason` e `blocked_by`.
- `update_task` non cambia il goal di un task.

Nessun tool chiude un goal, avvia la consegna, cambia `auto_fix` o `auto_deliver`, o scrive la fase.

## 13. Interfaccia

**Pannello Task.** In cima una sezione sola, **Serve a te**, che sostituisce "Proposte" e "Falliti":

- proposte (di task e di impostazioni), con il goal a cui appartengono e, per quelle di correzione, il giro;
- goal pronti, con **Consegna** (o **Consegna comunque**);
- task che richiedono attenzione;
- task falliti, con il motivo e i task che bloccano;
- follow-up aperti dei goal chiusi di recente.

Sotto, **Goal in corso**: titolo, fase, giro se maggiore di 1, avanzamento "3/5". Poi "In corso" e "In coda"
come oggi; i task mostrano il goal come etichetta.

**Card della proposta** con goal nuovo: titolo e richiesta del goal, branch che verrà creato, e le due scelte
dell'utente: correzioni automatiche (no / 1 / 2 / 3 giri) e consegna automatica (no / sì).

**Tasks Manager.** Filtro per goal in Kanban e Lista; raggruppamento per goal nella Lista. **Dettaglio del
goal** nel pannello laterale: richiesta, criteri, decisioni (con "Aggiungi nota"), branch e base per repository,
task per giro, esito delle review, correzioni e consegna automatiche, rapporto di consegna con i link delle PR,
"Consegna", "Chiudi goal".

## 14. Migrazione

- Board v1 → v2: aggiunge `goals: []`. Nessun goal ricostruito dai task esistenti: restano sciolti.
- `fail_reason` assente sui task già `failed`: la vista lo tratta come sconosciuto, nessuna deduzione dalla nota.
- I pacchetti degli agenti cambiano (skill `task-intake`, istruzioni di worker e reviewer): prontezza `outdated`
  fino all'aggiornamento, come per ogni modifica alle skill.
- Il riallineamento delle worktree (BOARD.MD, "Worktree") deve conoscere i branch `goal/*`: non sono orfani
  finché il goal è aperto o abbandonato con i branch conservati.

## 15. Cosa non si fa

- Workflow definiti dall'utente, con rami o condizioni.
- Un agente che decide le transizioni di un goal.
- Goal annidati, goal che dipendono da altri goal, priorità, scadenze, assegnatari.
- Modifiche ai quattro stati dei task, ad `ask_user`, all'HUB e alla consegna dei task sciolti.

## 16. Tappe

Ogni tappa è consegnabile da sola e lascia l'app coerente.

| Tappa | Contenuto | Dipende da |
|---|---|---|
| T1 | `fail_reason` e `blocked_by` (modello, vista, badge, `list_tasks`) — **consegnata** | — |
| T2 | Goal: modello, `propose_tasks` con `goal`/`goal_id`, eredità dalle review, fase, chiusura a mano, archivio, `list_goals`/`get_goal`, skill `task-intake` — **consegnata** | — |
| T3 | Interfaccia: "Serve a te", goal in corso, filtro e dettaglio del goal — **consegnata** | T1, T2 |
| T4 | Branch del goal: creazione, worktree dei task dal branch, integrazione, review sul branch, pulizia | T2 |
| T5 | Consegna del goal: `kind: deliver`, pulsante, `auto_deliver`, rapporto, chiusura `delivered` | T3, T4 |
| T6 | Passaggio di consegne: `dependencies` e `goal` in `get_task`, registro delle decisioni — **consegnata** | T2 |
| T7 | Correzione automatica con tetto di giri | T3, T4 |
| T8 | Follow-up alla chiusura | T3 |

Fra T2 e T5 un goal si chiude solo a mano, o da solo quando tutti i suoi task sono `done` (regola provvisoria,
sostituita in T5 dalla chiusura alla consegna). T4 e T5 conviene rilasciarle insieme: con il branch ma senza la
consegna, il lotto resterebbe sul branch del goal.

Note sulla consegna di T2 (il contratto è in BOARD.MD, "Goal"):

- lo schema del file della board passa alla versione 3 (il passo `v2_to_v3` aggiunge `goals: []`; la "v1 → v2"
  di "Migrazione" è il primo passo dei goal nella numerazione dei goal, non in quella del file);
- nella fase, una review del goal `failed` lascia il goal in `verifying` (con `failed` in `waiting`), non in
  `ready`; `ready` esiste nel tipo ma in T2 non dura, perché la chiusura provvisoria scatta subito;
- un goal rimasto senza task (tutti eliminati) e senza proposte si chiude da solo `abandoned`;
- la proposta di un worker (reviewer o planner) non può indicare `goal` né `goal_id`: il goal del reviewer lo
  eredita l'app, i task `plan` non ne hanno;
- un goal chiuso non accetta note dell'utente.

## 17. Punti aperti

1. `auto_fix.max_rounds` e `auto_deliver` hanno un valore di serie per workspace, o si scelgono sempre goal per
   goal?
2. Il tetto di 3 giri automatici è giusto, o serve anche un tetto sul numero di task per giro?
3. La consegna merita un Bot e una skill dedicati, o bastano le istruzioni del `kind` con General?
4. In modalità `pr`, il branch del goal va pubblicato sul remoto durante il lavoro (visibilità, copia di
   sicurezza) o solo alla consegna?

Deciso: un goal rimasto indietro rispetto alla base, o che si sovrappone a un altro goal, **si riallinea solo
alla consegna**; i conflitti li risolve il worker di consegna. Nessun indicatore di ritardo, nessun "Riallinea"
anticipato, nessun avviso dell'Intake sulle sovrapposizioni.
