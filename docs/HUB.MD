# Agents HUB

> **Temporaneamente disabilitato.** L'integrazione dell'Agents HUB è momentaneamente disabilitata: era un'implementazione
> sperimentale e non è più usata. Il codice resta, ma `hub::settings::HUB_AVAILABLE` è `false`: l'HUB risulta spento
> ovunque (avvio, feed, segretari, notifiche, connettori) anche se `hub_settings.json` dice `enabled: true`, e
> `hub_set_enabled(true)` restituisce un errore. Il file delle impostazioni non viene toccato. Per riattivarla basta
> mettere la costante a `true` (`src-tauri/src/hub/settings/availability.rs`).

L'**Agents HUB** è una modalità opzionale di cli-companion: uno spazio stile Slack con un **canale per
workspace** più un canale **Generale**, e dentro ogni canale dei **thread** di messaggi. Nel backend ci sono
l'interruttore globale, il modello dati, la persistenza, i comandi Tauri, l'evento verso il frontend e gli
**eventi della board** nei thread (vedi "Eventi della board") e la **configurazione dei segretari** (provider del
modello, lingua, sessioni, elenco dei segretari, API key: vedi "Segretari e provider"); nel frontend la vista (vedi
"Interfaccia"), le **impostazioni dei segretari** e la loro presenza nella conversazione (menzioni, "sta scrivendo",
chi è sveglio) e il **motore dei segretari**, che li fa rispondere nei thread (vedi "Motore dei segretari"), aprire
task di pianificazione e decidere le proposte quando l'utente risponde al loro evento (vedi "Pianificazione dal
thread").

Il codice sta in `src-tauri/src/hub.rs` e `src-tauri/src/hub/`. Le dipendenze e il posto del modulo nella mappa
sono in [DESIGN.MD](DESIGN.MD), "`hub/`".

**Pianificato**: i **connettori** (Fonti come Gmail, che portano dati esterni nei thread, e Canali di contatto come
Telegram, per notifiche e conversazioni dall'esterno) sono progettati in [CONNECTORS.MD](CONNECTORS.MD), che indica
anche i comportamenti di questo documento destinati a cambiare.

## Interruttore

`hub_settings.json` nella cartella dati dell'app: `enabled` è `false` di default, nella busta di `store/`
(`{schema_version, data}`, prima versione con `adopt_envelope`). La struttura ha `#[serde(default)]`: i campi
della tappa 2 (`provider`, `language`, `session`, `secretaries`, vedi "Segretari e provider") e `notifications` (acceso di default, vedi "Notifiche") si sono aggiunti
senza migrazione e un file della tappa 1 (`{ "enabled": true }`) si legge senza perdere `enabled`.

Con l'HUB **spento** (default):

- non si crea nessun file (né `hub_settings.json` finché l'utente non cambia l'impostazione, né `threads/`);
- i comandi di scrittura (`hub_create_thread`, `hub_send`, `hub_set_thread_state`, `hub_delete_thread`) rispondono «L'Agents HUB è spento: attivalo nelle
  impostazioni.»;
- le letture (`hub_list_threads`, `hub_get_thread`) funzionano e non creano file;
- gli eventi della board non si registrano: nessuna istantanea in memoria, nessuna scrittura;
- board, task e il resto dell'app non cambiano. Rispegnerlo non tocca i thread su disco;
- le impostazioni (provider, API key, lingua, sessioni, segretari) si leggono e si scrivono comunque: la pagina
  delle impostazioni le mostra anche a HUB spento.

## Segretari e provider

I **segretari** sono bot con un nome che l'utente sveglia con `@nome` in un thread e che parlano con un modello
tramite un endpoint compatibile OpenAI. Qui la **configurazione** (codice in `hub/settings/` e
`hub/api_key.rs`); il motore che li fa parlare (vedi "Motore dei segretari") legge questi dati. Tutto è in `hub_settings.json` tranne la API key.

| Campo | Default | Regole |
|---|---|---|
| `provider.base_url` | `""` | un solo endpoint per tutta l'app: `http://` o `https://` (es. `http://127.0.0.1:8080/v1` per llama-server, `http://127.0.0.1:11434/v1` per Ollama), un host, porta facoltativa 1..65535, niente credenziali (`user@`), frammento, spazi o caratteri di controllo, al massimo 2048 caratteri; i `/` finali si tolgono. Vuoto = non configurato |
| `provider.model` | `""` | al massimo 128 caratteri dopo il trim, niente caratteri di controllo. Vuoto = non configurato |
| `language` | `it` | lingua delle risposte dei segretari: `it`, `en`, `es`, `fr`, `de` (`HubLanguage`). Il motore aggiunge al system prompt "Always reply to the user in Italian" (nome inglese da `Language::english_name`); i messaggi evento restano in italiano |
| `session.idle_minutes` | 20 | 1..240: minuti di inattività dopo cui la conversazione interna di un segretario si chiude |
| `session.max_steps` | 8 | 1..20: passi di tool calling al massimo per risveglio |
| `secretaries` | `[]` | al massimo 20 |

Il provider è configurato (`Provider::is_configured`) quando indirizzo e modello ci sono entrambi; i due campi
si possono salvare anche uno alla volta.

**Segretario** (`HubSecretary`):

| Campo | Regole |
|---|---|
| `name` | handle della menzione: trim e minuscolo, poi 1..24 caratteri tra `a-z`, `0-9` e `-`, senza `-` iniziale o finale; unico |
| `image` | facoltativa, `{ png, color }` come le immagini dei bot (`bots::validate_image`: PNG base64, al massimo 64×64 pixel e 32 KB, colore `#rrggbb`) |
| `system_prompt` | testo libero letto dal modello (di solito in inglese), al massimo 8000 caratteri, a capo e tabulazioni ammessi. Vuoto = segretario **non configurato**: esiste ma il motore non lo fa rispondere (`Secretary::is_configured`) |
| `tools` | tool del motore che può usare (`HubSecretaryTool`): `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message`, `open_plan_task`, `decide_proposal`, `relay_answer`, `task_action`, `board_control`, `request_task_action`, `request_task_update`, `set_thread_state`; senza ripetizioni, nell'ordine canonico. Il limite vero di cosa fa sta qui, non nel prompt. Un tool nuovo non si aggiunge da solo ai segretari esistenti: lo sceglie l'utente |
| `preset` | facoltativo: id del preset da cui il segretario è nato (vedi "Segretari predefiniti"); assente se creato da zero o in un file scritto prima, e un valore illeggibile vale assente. Resta anche se l'utente modifica il segretario |
| `channels` | `{ "mode": "all" }` (Generale e ogni workspace, anche quelli futuri) oppure `{ "mode": "only", "channels": [...] }` con `general` o id di workspace esistenti (`hub/channel.rs`), da 1 a 200, senza ripetizioni |

### Segretari predefiniti

Quattro preset (`hub/settings/presets.rs`, prompt in `hub/settings/presets/<id>.md`, in inglese). Sono divisi per il modo di
lavorare con l'utente, non per competenza tecnica: il segretario non conosce il codice. Non ripetono le regole comuni,
che il motore aggiunge a ogni system prompt (`ENGINE_INSTRUCTIONS`): descrivono solo la mansione.

| Id | Etichetta | Nome | Tool | Mansione |
|---|---|---|---|---|
| `coordinator` | Coordinatore | `mario` | `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message`, `open_plan_task`, `decide_proposal`, `relay_answer`, `set_thread_state` | **punto d'ingresso unico**: smista ogni richiesta prima di agire. Una domanda sullo stato del lavoro la risponde da sé dalla board (la panoramica mostra tutti i workspace); una domanda sul codice, sulla sua storia o su un'opzione tecnica, senza nulla da cambiare, apre un task di pianificazione con il Bot il cui compito dice che risponde alle domande (di serie `Quick Answer`), o con quello che fa ricerca se l'utente vuole un'analisi approfondita (nessuno di serie: lo crea l'utente); una modifica al progetto: chiarisce l'obiettivo (al massimo una domanda) e apre la pianificazione con il Bot che pianifica le modifiche (di serie `Planner`). Sceglie sempre fra i Bot ricevuti (`kinds` con `plan`), dal compito: il prompt descrive le categorie di Bot e non ne nomina gli id. Per fermare, eliminare o modificare un task, o cambiare la coda, rimanda a un segretario che gestisce la coda. Per configurare il workspace o le regole dei worker, o per creare e gestire skill, che la chat non raggiunge, dice di aprire dal pannello dei terminali il Bot Setup o Skill Wizard. Presenta le proposte e le decide solo se l'utente accetta o rifiuta con chiarezza, riferisce fallimenti e domande dei worker |
| `analyst` | Analista | `giulia` | `thread_read`, `post_message`, `open_plan_task` | trasforma un'idea vaga in una richiesta completa: chiede obiettivo, criteri di completamento, vincoli ed esclusioni (mai domande tecniche sul codice), poi riassume in Goal / Done when / Constraints / Out of scope e apre la pianificazione solo dopo la conferma, scegliendo il Bot come il Coordinatore. Se la richiesta è in realtà una domanda sul codice, senza nulla da cambiare, non intervista: apre la pianificazione con la domanda come goal e il Bot che risponde alle domande. Senza `decide_proposal`, se arriva una proposta la presenta e dice di rispondere sul messaggio della proposta |
| `reporter` | Cronista | `luca` | `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message` | tiene informato l'utente: cosa è fatto, cosa gira, cosa aspetta lui, cosa è fallito; nel Generale una riga per workspace, prima quelli che aspettano l'utente; spiega i fallimenti dalle note dei task senza ipotesi. Non apre task e non decide: per nuovo lavoro rimanda a un segretario che può aprirli |
| `operator` | Operatore | `ugo` | `board_status`, `board_overview`, `task_details`, `thread_read`, `post_message`, `task_action`, `board_control`, `request_task_action`, `request_task_update`, `set_thread_state` | agisce sulla coda solo su richiesta esplicita dell'utente: ritenta i task falliti, porta un task in testa o prima di un altro, mette in pausa o riprende la board e ne regola `max_parallel`. Prima di cambiare un task lo legge; se la richiesta è ambigua chiede; dopo una modifica la dice in una riga. Fermare, eliminare o segnare fatto non lo fa mai da solo: lo chiede con `request_task_action`; titolo, specifica, dipendenze, modello e Bot di un task in coda o fallito li cambia solo con `request_task_update` (schede di conferma, vedi "Azioni che chiedono conferma"). Non apre lavoro nuovo. **Non si crea da solo** (`seeded: false`): lo sceglie l'utente da «Aggiungi da modello» |

Ogni preset ha una lista esplicita di tool, mai `SecretaryTool::ALL`: un tool nuovo, soprattutto se modifica la board,
entra in un preset solo con una scelta esplicita (un test fissa le liste). Il Coordinatore non ha `task_action` né
`board_control`. I prompt dei preset con `open_plan_task` o `request_task_update` spiegano come scegliere il Bot (un
test lo verifica); un segretario già salvato tiene il suo prompt finché l'utente non usa «Ripristina predefinito»,
ma riceve comunque dal motore l'elenco dei Bot e la descrizione dei tool, che chiedono `bot`.

Tutti hanno `channels` = tutti i canali.

- **Creazione automatica.** Quando l'HUB passa da spento ad acceso (`hub_set_enabled`) e l'elenco dei segretari è vuoto,
  si creano i preset con `seeded: true` (Coordinatore, Analista, Cronista: nessuno cambia la board) con i nomi
  suggeriti (`secretaries::seed_presets`, pura). Con l'elenco non vuoto non si
  aggiunge nulla, quindi chi ha già i suoi segretari non ne trova di nuovi e nessuna migrazione tocca quelli esistenti. Un
  preset che non si può creare (nome già preso, tetto di 20) si salta senza errore.
- **Aggiungi da modello.** Nella sezione Segretari, accanto a «Nuovo segretario», si sceglie un preset: l'editor si
  apre precompilato (nome suggerito, prompt, tool, tutti i canali) e si salva con il flusso normale; il segretario
  ricorda il preset (`hub_create_secretary` con `preset`). Se il nome suggerito è già preso l'editor lo dice subito.
- **Ripristina predefinito.** Nell'editor di un segretario nato da un preset, con conferma, riporta prompt e tool al
  testo del preset (`hub_restore_secretary_preset`, `secretaries::restore_preset`); nome, canali e immagine restano. Un
  segretario creato da zero non ha il pulsante.
- **Versione aggiornata.** Un prompt di preset cambiato in una release non raggiunge da solo i segretari già salvati
  (nessuna migrazione dei prompt: sovrascriverebbe quelli personalizzati). Per farlo sapere, la riga del segretario
  (`SecretaryRow`) e l'editor, accanto a «Ripristina predefinito» (`PresetRestore`), mostrano «È disponibile una
  versione aggiornata: Ripristina predefinito» quando il prompt salvato differisce da quello del preset da cui il
  segretario è nato (`updatedPresetOf` in `src/lib/hubPresets.ts`, confronto nel frontend con
  `HubSecretaryPreset.system_prompt`, nessun binding nuovo). Il confronto è testuale: anche un prompt personalizzato a
  mano mostra l'avviso, e ripristinare ne perde le modifiche.

**Nomi riservati.** I nomi dei segretari non devono mai diventare nomi delle sessioni dei terminali (`@mario` non
può essere un terminale): il frontend li legge da `hub_get_settings` e li esclude quando sceglie il nome di una
sessione (`pickBotName` in `src/lib/bots.ts`; la pool di `bots.rs` e `bots.json` non cambiano). Dettagli in
"Interfaccia", "Nomi riservati".

**Compatibilità in lettura.** `hub_settings.json` non deve finire in un backup per un valore che questa versione
non conosce (`hub/settings/tolerant.rs`): una lingua sconosciuta vale `it`, un tool sconosciuto (l'enum crescerà
con `relay_answer`, `ask_user`) si salta, una forma di `channels`
sconosciuta vale "nessun canale" (mai più permessi di quelli scelti), un segretario illeggibile si salta; il resto
del file resta. Ogni elemento saltato lascia un avviso nel log. Al salvataggio successivo i valori saltati non
vengono riscritti.

### API key

La API key del provider sta nel **portachiavi del sistema operativo** (Credential Manager su Windows, Keychain su
macOS, Secret Service su Linux), servizio `cli-companion`, conto `hub-provider-api-key`, con il crate `keyring`
(4.x, sola feature `v1`, MIT/Apache-2.0). L'accesso passa dal trait `secrets::SecretStore` (`OsKeyring` in
produzione, `MemorySecrets` nei test): un altro backend non tocca l'HUB.

- Non finisce mai nei file JSON, nel log né nei DTO: il frontend riceve solo `api_key_set: bool` (`HubSettingsView`)
  e la chiave si legge solo nel backend (`ApiKey::read`, per il client del modello). Gli errori del portachiavi
  descrivono il guasto senza citarne il contenuto.
- Validazione: trim, 1..1024 caratteri ASCII visibili (finisce in un'intestazione HTTP).
- La presenza si chiede al portachiavi la prima volta e poi resta in memoria, aggiornata da `hub_set_api_key` e
  `hub_clear_api_key`. Se il portachiavi non risponde, `api_key_set` vale `false` (errore nel log) e le
  impostazioni restano leggibili.

## Modello dati

Tipi Rust in `hub/model.rs`, esportati con ts-rs con il prefisso `Hub` (`HubThread`, `HubMessage`...).

**Canale** (`hub/channel.rs`): sul filo e su disco è una stringa, `"general"` oppure l'id di un workspace. Il nome
`general` è riservato; l'id di un workspace deve essere un nome di file sicuro (`validate::valid_workspace_id`,
al massimo 128 caratteri). In Rust è `enum Channel { General, Workspace(String) }`.

**Thread** (`HubThread`):

| Campo | Note |
|---|---|
| `id` | `thr-<millisecondi>-<n>` |
| `channel` | il canale del file che lo contiene |
| `title` | 1..120 caratteri, una riga |
| `state` | `open` \| `closed` |
| `task_ids` | task collegati (un task appartiene a un solo thread), al massimo 200 |
| `proposal_ids` | proposte collegate ancora da decidere, dalla più vecchia (una proposta appartiene a un solo thread; un thread può averne più d'una pendente). Decisa (accettata o scartata), una proposta si toglie dopo averla instradata; al massimo `PROPOSALS_PER_THREAD_MAX` (= `MAX_PENDING_PROPOSALS`, 20): oltre si tolgono le più vecchie, cioè proposte decise mentre l'HUB era spento. I file scritti prima hanno `proposal_id` (una stringa o `null`), letto come elenco di al più una proposta (`model/legacy.rs`) |
| `owner` | segretario **titolare** del thread: quello che vi ha aperto l'ultimo task di pianificazione (`open_plan_task`), `null` altrimenti. Alcuni eventi della board lo svegliano (vedi "Risvegli da evento"). Resta finché un altro segretario apre una pianificazione nel thread; non va confuso con il titolare della *sessione* ("Menzioni e risveglio"), che dura quanto la sessione. Facoltativo nei file scritti prima (`#[serde(default)]`) |
| `created_at`, `updated_at` | millisecondi Unix |
| `summary` | riassunto del thread (al massimo 10 righe, 1500 caratteri), scritto da un segretario alla chiusura della sua sessione; `null` finché nessuna sessione si è chiusa. Aggiornarlo non cambia `updated_at`. Facoltativo nei file scritti prima (`#[serde(default)]`) |
| `messages` | in ordine di arrivo |

I messaggi stanno dentro il thread: si leggono e si potano sempre insieme al loro thread, e il file del canale
resta uno solo. L'elenco (`hub_list_threads`) restituisce i riassunti, senza i messaggi.

**Messaggio** (`HubMessage`): `id` (`msg-<millisecondi>-<n>`), `author`, `kind`, `body`, `reply_to`, `event`,
`attachments`, `origin`, `created_at`.

- `author`: `{ "type": "user" }` | `{ "type": "system" }` | `{ "type": "bot", "name": "mario" }` (un segretario:
  `name` è il suo handle) | `{ "type": "external", "connector": "gmail", "instance": "…", "name": "Anna" }` (un
  elemento di una fonte esterna: contenuto non fidato, vedi "Risvegli da elementi esterni"). Le menzioni svegliano i
  segretari solo nei messaggi `user`; un elemento `external` sveglia solo tramite la regola che lo ha consegnato.
  Per un autore `external` `connector` e `instance` sono id validi (`valid_ref_id`) e `name` è ripulito (i
  caratteri di controllo diventano spazi) e dev'essere di 1..120 caratteri (`text::external_name`); un messaggio
  `external` richiede sempre un `origin` dello stesso connettore e della stessa istanza.
- `kind`: `text` | `event`, ricavato da `event` (presente = `event`).
- `reply_to`: id di un messaggio **dello stesso thread**; un id che non c'è è rifiutato. Se poi il messaggio
  citato viene potato, la UI lo tratta come assente.
- `event` (`HubEvent`): dati strutturati, accanto al testo a template del messaggio, letti come dati (non come
  testo) dai segretari (`thread_read` e contesto del risveglio): `kind` (`task_created`, `task_started`, `task_done`, `task_failed`,
  `task_attention`, `task_question`, `task_question_answered`, `task_deleted`, `proposal_created`,
  `proposal_accepted`, `proposal_rejected`, `action_request`, `action_request_resolved`), `workspace`
  (id del workspace della board), `task_id`, `proposal_id`, `status` (stato del task nella board), `summary`
  (riassunto del report, al massimo 1000 caratteri più «…»), `summary_chars` (lunghezza in caratteri del riassunto
  intero, presente solo se `summary` è accorciato: il testo intero è nel report del task), `verdict` (`passed` |
  `issues`, esito di una review), `note` (nota dell'app sul task, accorciata come `summary`), `note_chars` (come
  `summary_chars`, per la nota; i due campi mancano nei file scritti prima e con un testo corto),
  `question` (`HubQuestion`: `id` della domanda sulla board, `text` al massimo 2000 caratteri, `options` al
  massimo 5 da 80 caratteri; negli eventi `task_question` e `task_question_answered`), `request`
  (`HubActionRequest`, negli eventi `action_request` e `action_request_resolved`; vedi "Azioni che chiedono conferma").
  In `proposal_created` `task_id` è il task che ha fatto la proposta (il `plan` del pianificatore o la review),
  se c'è: il soggetto resta la proposta.
  Ogni evento deve nominare il suo soggetto: `task_id` per gli eventi dei task, `proposal_id` per quelli delle
  proposte. `workspace` deve essere l'id di un workspace (non `general`); `summary` e `note` al massimo 8000
  caratteri. Nuovi tipi e campi si aggiungono come varianti e campi facoltativi (compatibile in lettura).
- `attachments` (`HubAttachment[]`): gli allegati, solo nei messaggi `user` e `external` (vuoto negli altri e nei file scritti
  prima, `#[serde(default)]`). Ogni allegato porta solo i metadati: `id` (`att-<millisecondi>-<n>`), `name` (il
  nome originale ripulito, vedi "Allegati"), `mime` (dedotto dalle firme dei primi byte, poi dall'estensione;
  `application/octet-stream` se ignoto: serve alla UI per l'anteprima, non filtra nulla) e `size` (byte). Nessun
  percorso: si ricava da cartella dati, canale, thread, `id` e `name`, e i byte non finiscono mai nel JSON. Con
  almeno un allegato `body` può essere vuoto.
- `origin` (`HubMessageOrigin`, facoltativo): da dove viene un messaggio arrivato da fuori dell'app, cioè un
  elemento di una fonte (autore `external`) o un messaggio dell'utente scritto in un canale di contatto (autore
  `user`). Porta `connector`, `instance` (id validi) ed `external_id`, l'id dell'elemento nel servizio esterno
  (1..256 caratteri, senza caratteri di controllo; non si ripulisce perché è la chiave del dedupe). Assente nei
  messaggi scritti nell'app e nei file scritti prima (`serde(default)`). Con `instance` ed `external_id` la
  consegna riconosce un elemento già consegnato in un thread (vedi "API interna").

**Riassunto** (`HubThreadSummary`): `id`, `channel`, `title`, `state`, `task_count`, `message_count`,
`waiting_user`, `owner` (il titolare del thread, `null` se non ne ha), `created_at`, `updated_at`; elenco dal più recente.

`waiting_user` (`hub/waiting.rs`) è **calcolato**, mai salvato, così non può divergere dai messaggi: per ogni
soggetto (task o proposta) conta l'ultimo evento, e il thread aspetta l'utente se almeno uno è `task_failed`,
`task_attention`, `task_question` o `proposal_created`. Un task fallito e poi ripartito, una domanda risposta
(`task_question_answered`) o una proposta decisa non aspettano più.
Gli eventi potati dal limite dei messaggi non contano più.

## File

| File | Contenuto |
|---|---|
| `<app_data>/hub_settings.json` | `HubSettings` |
| `<app_data>/threads/<workspace_id>.json` | i thread del canale del workspace |
| `<app_data>/threads/general.json` | i thread del Generale |
| `<app_data>/hub_attachments/staging/<attachment_id>/<nome>` | allegato caricato e non ancora inviato |
| `<app_data>/hub_attachments/channels/<canale>/<thread_id>/<attachment_id>/<nome>` | allegato di un messaggio |

Mai nella repository dell'utente. I file dei thread contengono `{ "threads": [...] }` nella busta di `store/`
(migrazioni, backup datati dei file corrotti, scrittura atomica, avvisi `store-warning`); la catena di migrazioni è
in `hub/files.rs`. I campi facoltativi hanno `#[serde(default)]`: un file scritto prima che un campo esistesse
resta leggibile. Un thread di un altro canale trovato nel file (file spostato a mano) viene escluso con un avviso
nel log.

**Caricamento pigro**: all'avvio si legge solo `hub_settings.json`. Un canale si legge dal disco la prima volta che
un comando lo chiede e resta in memoria (`HubState`, con il suo `JsonFile`) fino alla chiusura: con l'HUB spento
l'avvio non fa lavoro in più, e la protezione di `store/` contro la riscrittura di un file che non si è potuto
leggere vale per tutti i salvataggi successivi.

**Salvataggio**: ogni modifica si applica a una copia, si salva e solo poi si pubblica in memoria; se fallisce
nulla cambia; un'operazione che non tocca nessun thread non scrive (`HubState::update_when`). Un solo lock per
tutti i canali, tenuto anche durante il salvataggio: le scritture sono rare (messaggi dell'utente, eventi della
board) e non girano mai sul main thread né sotto un lock della board.

**Rimozione di un workspace**: `workspaces::remove_workspace`, dopo la board, chiama `hub::forget_workspace`, che
toglie il canale dalla memoria, butta l'istantanea della sua board e cancella `threads/<workspace_id>.json`
(nessun errore se non esiste) e poi, fuori dal lock, `hub_attachments/channels/<workspace_id>/`. `board` non dipende da `hub`; `automation` solo dall'host reale (vedi "Eventi della
board"). I comandi e gli eventi della board verificano, sotto il lock dell'HUB, che il workspace del canale
esista ancora: un id inventato o un workspace appena rimosso non lasciano file orfani.

### Allegati

Codice in `hub/attachments/` (`name`, `mime`, `layout`, `garbage` puri; `storage` per il disco; `commands`). I
file stanno solo in `<app_data>/hub_attachments/`, mai nella repository dell'utente; una cartella per allegato,
così il nome resta quello ripulito e l'allegato si sposta o si cancella con la sua cartella. Ogni componente del
percorso si valida (`layout`): canale come sopra, thread e id dell'allegato con `text::valid_ref_id` (lettere,
cifre, `-`, `_`), nome uguale alla sua versione ripulita: nessun percorso può uscire dalla cartella degli allegati,
nemmeno da un file dei thread modificato a mano.

- **Nomi** (`name::clean`): solo l'ultimo componente del percorso; `<>:"/\|?*` e i caratteri di controllo
  diventano `_`; via gli spazi iniziali e i punti e spazi finali; i nomi riservati di Windows (`CON`, `PRN`, `AUX`,
  `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`, anche con le cifre in apice, con o senza estensione) prendono un `_` davanti;
  l'estensione resta. Vuoto o più lungo di 120 caratteri: rifiutato.
- **Caricamento in due fasi.** Il frontend carica il file nello staging (`hub_stage_attachment_file` da un
  percorso, `hub_stage_attachment_bytes` dai byte) e riceve `HubAttachment` con l'id; poi `hub_send` riceve gli
  id. La copia nello staging gira senza il lock dell'HUB; il limite di dimensione vale anche per un file che
  cresce durante la copia.
- **Invio** (`append::append_user_message`): validati gli id (al massimo 5, senza doppioni), controllati canale e
  thread, gli allegati si spostano (`rename`, atomico: due invii con lo stesso id non possono prenderlo entrambi)
  da `staging/` alla cartella del thread e solo poi si salva il messaggio; se il salvataggio fallisce tornano
  nello staging e l'utente può riprovare con gli stessi id. Senza allegati `hub_send` fa esattamente quello di prima.
- **Pulizia.** `HubState::update_when`, dopo ogni salvataggio riuscito, confronta i thread di prima e di dopo
  (`attachments::garbage`, puro) e, fuori dal lock, cancella le cartelle degli allegati dei messaggi potati
  (`MESSAGES_MAX`) e dei thread spariti (thread chiusi potati, thread eliminati con `hub_delete_thread`): vale per qualunque operazione, compresi gli eventi
  della board che spingono fuori un messaggio dell'utente. Un salvataggio fallito non cancella nulla. La rimozione
  del workspace cancella la cartella del canale. Gli errori di cancellazione vanno nel log e non bloccano.
- **Staging orfano**: all'avvio, in background, si tolgono le voci dello staging precedenti all'avvio (il
  frontend perde gli id alla chiusura e non deve conservarli); a ogni caricamento quelle più vecchie di 24 ore.
  Senza staging non si crea nulla.
- **Lettura**: `hub_read_attachment` e `hub_attachment_path` trovano l'allegato solo tra quelli dei messaggi del
  thread; la funzione Rust `attachments::commands::attachment_path(app, channel, thread_id, attachment_id)`
  restituisce il percorso assoluto, ed è quella da usare per gli agenti.
- **Percorsi per gli agenti** (`listing::describe_with_path`, puro): pianificatore e worker ricevono i percorsi
  nella forma del loro ambiente, da `AttachmentPaths { root, env }`. `env` è il `TargetKind` del workspace del
  canale (`attachments::commands::agent_paths`; i task di un thread sono sempre del workspace del suo canale):
  Windows lascia il percorso dell'host, WSL lo traduce con `platform::windows_to_wsl_path` (`/mnt/c/...`). Un
  percorso che WSL non raggiunge diventa «(path unavailable)», mai inventato.
- **Accesso dei worker.** La cartella degli allegati è fuori dal repository in cui parte il worker: senza un
  permesso Claude Code chiederebbe all'utente di leggerla (anche in plan mode) e un worker automatico si
  fermerebbe. Con l'HUB acceso l'avvio di ogni worker (`TauriHost::worker_permissions_file`) concede la **sola
  lettura** della sola cartella del canale del workspace, `hub_attachments/channels/<workspace_id>/`
  (`attachments::workers_readable_dir`: mai lo staging, gli altri canali o il resto della cartella dati, che
  contiene impostazioni e API key), tramite il contratto `WorkerPermissions` dell'agente (vedi
  [DESIGN.MD](DESIGN.MD), avvio dei worker): con Claude Code `Read(//<cartella>/**)` consentito e
  `Edit(//<cartella>/**)` vietato nel file `--settings`. Limiti: un worker avviato con l'HUB spento non ha il
  permesso (se poi riceve un allegato, chiede); per OpenCode su Windows la corrispondenza dei percorsi nativi nelle
  regole `external_directory` non è documentata da OpenCode.

## Limiti

Assunzioni di questa tappa, tutte in costanti di `hub/text.rs`, `hub/threads.rs` e `hub/attachments.rs`
(`hub/attachments/name.rs` per il nome):

| Limite | Valore | Comportamento |
|---|---|---|
| Testo di un messaggio | 1..8000 caratteri dopo il trim (0..8000 con almeno un allegato) | rifiutato se vuoto o troppo lungo; ammessi a capo e tabulazioni, non gli altri caratteri di controllo |
| Allegati di un messaggio | 5 | rifiutato oltre; solo i messaggi dell'utente e dei mittenti esterni |
| Dimensione di un allegato | 10 MB | rifiutato al caricamento |
| Nome di un allegato | 1..120 caratteri dopo la pulizia | rifiutato (vedi "Allegati") |
| Allegato nello staging | 24 ore | tolto, insieme a quelli di un avvio precedente |
| Titolo di un thread | 1..120 caratteri dopo il trim | rifiutato; una riga sola |
| Messaggi per thread | 500 | si tolgono i più vecchi |
| Thread chiusi per canale | 200 | alla chiusura si tolgono i chiusi aggiornati da più tempo (mai quello appena chiuso); gli aperti non si potano |
| Task collegati a un thread | 200 | rifiutato oltre |
| Nome di un bot autore | 1..24 caratteri | come i nomi della pool di `bots.rs` |
| Nome di un mittente esterno | 1..120 caratteri dopo la pulizia | i caratteri di controllo diventano spazi, il resto oltre il limite si taglia con «…»; vuoto = rifiutato |
| `external_id` di un'origine | 1..256 caratteri | rifiutato se vuoto, troppo lungo o con caratteri di controllo |
| Testo di una consegna esterna | fino a 8000 caratteri | non si rifiuta: si ripulisce e si taglia con «… (troncato)», lasciando posto alla nota sugli allegati non importati |
| Testo lungo dell'app o di un segretario (risposta finale, `post_message`, domande del pianificatore) | parti da 8000 caratteri, al massimo 8 (`text::parts::MAX_PARTS`) | non si taglia: diventa più messaggi consecutivi, vedi sotto |

**Testi lunghi divisi in parti** (`hub/text/parts.rs`, puro). Un testo dell'app o di un segretario che supera un
messaggio non si taglia: `parts::split` lo divide in messaggi consecutivi dello stesso autore, scritti insieme sotto
lo stesso lock (`append::append_parts`: nessun altro messaggio in mezzo, un solo `hub-update`, una sola notifica di
sistema per la risposta di un segretario, sulla prima parte). Ogni parte si apre con «(parte i/n)» su una riga sua
e sta in 8000 caratteri; il taglio cade preferibilmente tra due paragrafi, poi tra due righe, poi tra due parole,
mai a metà di un carattere, e se si può fuori da un blocco di codice Markdown (```` ``` ```` o `~~~`): un blocco
tagliato si chiude alla fine della parte e si riapre (con lo stesso linguaggio) nella successiva. Lo spazio bianco
del taglio passa all'inizio della parte successiva, dopo l'etichetta: togliendo le etichette e concatenando le parti
si ritrova il testo esatto (se nessun blocco di codice è stato tagliato). Una parte tiene al massimo 7718 caratteri
del testo (il resto è riservato a etichetta, blocco riaperto e avviso) e almeno 6718 se non è l'ultima, quindi un
testo fino a `LONG_BODY_MAX` = 53744 caratteri ci sta sempre (più del report di 50000 di un `plan`). Oltre le 8
parti l'ultima finisce con un avviso esplicito: «… (testo interrotto: mancano N caratteri, oltre il limite di 8
messaggi)» per la risposta di un segretario, «… (mancano N caratteri: il testo intero è nel report del task, nel
Tasks Manager)» per le domande del pianificatore. Mai un taglio senza avviso.

Un thread chiuso accetta ancora messaggi (per esempio gli eventi dei suoi task) e resta chiuso.

**Chiudere e riaprire a mano.** Oltre alla chiusura automatica (task conclusi, proposta scartata) l'utente chiude e
riapre un thread con `hub_set_thread_state`, dal pulsante «Chiudi thread» / «Riapri thread» nell'intestazione della
conversazione (`ThreadStateButton`, in entrambe le viste), e un segretario con il tool `set_thread_state` (vedi
"Tool"). Cambia solo `state` e `updated_at` (`ChannelThreads::set_state`), e persiste al riavvio. Riaprire non
sveglia nessuno e non cambia cosa succede a un messaggio dell'utente in un thread chiuso. Se lo stato è già quello
chiesto non si riscrive il file, non c'è `hub-update` e non c'è avviso. Chiudere non cancella nulla e non ferma i task:
un evento di un task lo riapre, come sempre (`events::route`).

**Eliminare a mano.** `hub_delete_thread` (`hub/thread_delete.rs`) toglie definitivamente un thread, aperto o chiuso,
in qualunque canale (anche il Generale) e anche con task collegati (`ChannelThreads::remove_thread`). Si cancellano:
il thread nel file del canale, la cartella dei suoi allegati (dopo il salvataggio, come per i thread potati: vedi
"Allegati"), le sessioni dei segretari su quel thread (senza riassunto), la sua coda e l'esecutore in corso
(`SecretaryEngine::forget_thread`, poi `hub-secretary-status`) e la notifica raggruppata ancora in attesa
(`HubNotifier::discard_thread`). Dopo il salvataggio, fuori da ogni lock dell'HUB, gli ascoltatori del ciclo di vita
(`HubLifecycle::thread_removed`, registrati con `set_lifecycle_listener`) sanno del thread eliminato: il runtime dei
connettori ne elimina le sotto-chat nei canali di contatto (docs/CONNECTORS.MD §10, «Sotto-chat»); devono essere rapidi,
il lavoro di rete gira nei loro task. Un lavoro del motore già oltre l'ultimo punto di attesa non ricrea il thread: le sue
scritture falliscono con «thread non trovato» e finiscono nel log. Gli altri thread del canale non cambiano. Restano
i task della board, intatti con worktree e terminali: un loro evento successivo (per esempio la fine del task) non
trova più un thread e ne apre uno nuovo, come per un task che non l'ha mai avuto. Le regole dei connettori che
consegnano nel thread si disattivano da sole («thread non più disponibile», come per un thread potato; vedi
docs/CONNECTORS.MD). Solo dopo il salvataggio riuscito parte l'evento `hub-thread-removed`; un salvataggio fallito
non cambia nulla. Errori: HUB spento, canale di un workspace che non esiste, «thread non trovato».

## API interna

Logica pura in `ChannelThreads` (`hub/threads.rs`, `hub/threads/lifecycle.rs`), senza orologio né generatore di
id: tempo e id li passa il chiamante. La usano i comandi e gli eventi della board (`hub/events/route.rs`).

| Operazione | Funzione |
|---|---|
| Creare un thread | `create_thread(channel, id, title, now)` |
| Aggiungere un messaggio | `add_message(thread_id, NewMessage { id, author, body, reply_to, event, origin }, now)` |
| Aggiungere un messaggio con allegati | `add_message_with_attachments(thread_id, new, attachments, now)` (solo `user` ed `external`) |
| Allegato di un thread | `attachment(thread_id, attachment_id)` |
| Chiudere / riaprire | `set_state(thread_id, ThreadState::Closed \| Open, now)` |
| Eliminare un thread | `remove_thread(thread_id)` (qualunque stato; torna il thread tolto) |
| Collegare task | `link_tasks(thread_id, task_ids, now)` (tutto o niente) |
| Collegare / togliere una proposta | `link_proposal(thread_id, proposal_id, now)` (accanto alle altre), `unlink_proposal(proposal_id)` (decisa) |
| Rendere titolare un segretario | `set_owner(thread_id, name, now)` |
| Thread di un task / di una proposta | `thread_of_task(task_id)`, `thread_of_proposal(proposal_id)` |
| Elenco | `summaries()` |

Gli errori sono in italiano (li legge l'utente); un'operazione che fallisce non cambia nulla.

Per i canali di contatto e i connettori (docs/CONNECTORS.MD) ci sono quattro porte, tutte bloccanti (fuori dal main
thread) e senza dipendere da `connectors`:

| Porta | Funzione |
|---|---|
| Messaggio dell'utente | `hub::send::send_user_message(app, UserMessage { channel, thread_id, body, reply_to, attachment_ids, origin })`: la stessa strada di `hub_send` (`append_user_message`, poi risposta diretta a un worker o `SecretaryEngine::on_message`); controlla da sé l'HUB acceso. Con `origin` il messaggio non notifica, come quelli dell'app; se il thread ha già un messaggio con la stessa origine (`instance`, `external_id`) non scrive, non emette `hub-update`, non sveglia nessuno e restituisce quello già salvato (gli allegati tornano nello staging) |
| Messaggio dell'utente con allegati come byte | `hub::send::send_user_files(app, message, files)`: mette i file nello staging con gli stessi limiti dell'app, aggiunge al testo la nota `Allegati non importati: a.pdf (troppo grande), b.exe (nome non valido)` (nomi ripuliti e tagliati; il testo si accorcia per farle posto) e invia; se l'invio fallisce o il messaggio è un doppione cancella i file dallo staging. I file oltre 5, oltre 10 MB o con un nome inutilizzabile si saltano senza fermare il messaggio |
| Messaggio di un mittente esterno | `hub::ingress::deliver_external(app, ExternalDelivery { channel, target, author, body, files, origin, wake }) -> DeliveryOutcome { thread_id, message_id, duplicate }` |
| Destinatario delle notifiche | `hub::set_notice_sink(app, sink)` con un `NoticeSink` (vedi "Notifiche") |

`deliver_external` (`hub/ingress.rs`; parte pura in `hub/ingress/`: `check`, `body`, `apply`) lavora così:

- errore con l'HUB spento, il canale (o il thread indicato) mancante, un autore che non è `external` o un'origine
  di un altro connettore o istanza, un testo vuoto senza allegati;
- `target`: `Existing(thread_id)` (un thread chiuso si **riapre**, come fanno gli eventi della board) oppure
  `New { title }` (thread creato nello stesso salvataggio; titolo ripulito su una riga, «Senza titolo» se vuoto);
- **dedupe**: se il thread di destinazione (per `New`, un thread qualunque del canale) ha già un messaggio con lo
  stesso `(instance, external_id)` non scrive nulla (nemmeno il thread nuovo), non notifica e non sveglia nessuno e
  ritorna `duplicate: true` con il thread e l'id di quel messaggio. Lo controlla
  una lettura prima di preparare gli allegati e di nuovo dentro il salvataggio (corsa tra due consegne): in quel caso
  gli allegati preparati si cancellano;
- **testo**: ripulito dai caratteri di controllo e tagliato a 8000 caratteri totali con il suffisso «… (troncato)»,
  con la nota sugli allegati non importati su una riga a parte in fondo;
- **allegati**: i `files` si preparano nello staging fuori dal lock dell'HUB, poi si spostano nella cartella del
  thread (`Claimed`) e si salvano con il messaggio; se non si salva, tornano nello staging e si cancellano;
- dopo il salvataggio: un `hub-update`, la notifica `External { name }` (vedi "Notifiche") e il risveglio:
  `wake` è `None`, `Owner` (il titolare del thread; senza titolare solo una riga di log) o `Secretary(nome)`, e
  chiama `SecretaryEngine::on_external` (vedi "Risvegli da elementi esterni"). Mai `on_message` né le menzioni: il
  testo esterno è un dato non fidato.

## Comandi Tauri

Validano gli argomenti per primi e fanno l'I/O in `crate::blocking`. Wrapper nell'oggetto `api` di
`src/lib/tauri.ts`.

| Comando | Argomenti | Ritorna |
|---|---|---|
| `hub_get_settings` | — | `HubSettingsView` (`HubSettings` + `api_key_set`) |
| `hub_set_enabled` | `enabled` | `HubSettingsView` salvate (come `bots`: niente evento); spegnere butta le istantanee delle board; accendere con l'elenco dei segretari vuoto crea i preset `seeded` (vedi "Segretari predefiniti") |
| `hub_set_provider` | `base_url`, `model` | `HubSettingsView` |
| `hub_set_api_key` | `key` | `HubSettingsView` (la chiave va nel portachiavi) |
| `hub_clear_api_key` | — | `HubSettingsView`; nessun errore se non c'era |
| `hub_set_language` | `language` | `HubSettingsView` |
| `hub_set_notifications` | `enabled` | `HubSettingsView`; notifiche di sistema dei thread (vedi "Notifiche") |
| `hub_set_session` | `idle_minutes`, `max_steps` | `HubSettingsView` |
| `hub_create_secretary` | `name`, `system_prompt`, `tools`, `channels` (`null` = tutti), `preset` (`null` = da zero) | `HubSettingsView`; nuovo segretario in fondo, senza immagine; `preset` sconosciuto rifiutato |
| `hub_list_secretary_presets` | — | `HubSecretaryPreset[]`: id, etichetta, descrizione, nome suggerito, prompt e tool dei quattro preset (vedi "Segretari predefiniti") |
| `hub_restore_secretary_preset` | `name` | `HubSettingsView`; riporta prompt e tool al preset del segretario, rifiutato se non nasce da un preset |
| `hub_update_secretary` | `name`, `new_name`, `system_prompt`, `tools`, `channels` | `HubSettingsView`; sostituisce i campi, l'immagine resta; rinomina rifiutata se `new_name` è di un altro segretario |
| `hub_delete_secretary` | `name` | `HubSettingsView` |
| `hub_set_secretary_image` | `name`, `png`, `color` | `HubSettingsView` |
| `hub_remove_secretary_image` | `name` | `HubSettingsView` |
| `hub_list_threads` | `channel` | `HubThreadSummary[]`, dal più recente |
| `hub_get_thread` | `channel`, `thread_id` | `HubThread` con i messaggi |
| `hub_create_thread` | `channel`, `title` | `HubThread` (thread libero dell'utente) |
| `hub_send` | `channel`, `thread_id`, `body`, `reply_to?`, `attachments?` (id dallo staging) | `HubMessage` dell'utente; gli allegati passano dallo staging alla cartella del thread (vedi "Allegati"); dopo il salvataggio, una risposta a una domanda di un worker ancora in attesa va al worker così com'è (vedi "Domande dei worker"), altrimenti il motore sveglia i segretari (in background) |
| `hub_set_thread_state` | `channel`, `thread_id`, `closed` (`true` chiude, `false` riapre) | `HubThreadSummary` aggiornato, più `hub-update` se lo stato è cambiato; vedi "Chiudere e riaprire a mano" |
| `hub_delete_thread` | `channel`, `thread_id` | nulla; elimina il thread (qualunque stato) e i suoi allegati, chiude le sessioni dei segretari su di esso, avvisa gli ascoltatori (`thread_removed`: i topic dei canali di contatto), poi `hub-thread-removed`; i task collegati restano sulla board (vedi "Eliminare a mano") |
| `hub_stage_attachment_file` | `path` (assoluto, dal dialog) | `HubAttachment` nello staging |
| `hub_stage_attachment_bytes` | corpo raw (`InvokeBody::Raw`), nome nell'header `x-hub-file-name` (`encodeURIComponent`) | `HubAttachment` nello staging (immagine incollata, file trascinato) |
| `hub_read_attachment` | `channel`, `thread_id`, `attachment_id` | i byte (`ArrayBuffer`), per l'anteprima |
| `hub_attachment_path` | `channel`, `thread_id`, `attachment_id` | percorso assoluto, per aprirlo con l'opener |
| `show_hub_notification` | `channel`, `thread_id`, `title`, `body` | toast cliccabile (solo Windows) per un thread, definito in `toast.rs`; al click `hub-notification-activated` (vedi "Notifiche") |
| `hub_secretary_status` | — | `HubSecretaryStatus[]`: i segretari svegli adesso (vedi "Motore dei segretari") |
| `hub_resolve_action_request` | `channel`, `thread_id`, `message_id` (la scheda), `confirm` | `HubActionRequest` aggiornata: `confirm` esegue l'azione, altrimenti annulla (vedi "Azioni che chiedono conferma"); una richiesta già decisa è un errore («Questa richiesta è già stata decisa.»), una scaduta o non più valida no |

Con l'HUB spento `hub_create_thread`, `hub_send`, `hub_set_thread_state`, `hub_delete_thread` e i due `hub_stage_attachment_*` sono rifiutati (prima di creare
cartelle); letture, `hub_read_attachment`, `hub_attachment_path` e i comandi delle impostazioni funzionano sempre.
Gli errori di validazione sono in italiano (li legge l'utente) e un comando che fallisce non cambia nulla; un
segretario che non c'è risponde «segretario non trovato». Un canale di un workspace che non esiste
risponde «canale non trovato: il workspace non esiste più».

## Evento

`hub-update`, emesso a ogni modifica di un thread (dai comandi, dagli eventi della board e dal motore dei
segretari: risposte, avvisi, riassunto), con payload
`HubThreadSummary` del thread toccato (riga in `TauriEvents`). Basta per aggiornare l'elenco dei thread e il
badge del canale senza rileggere nulla; la conversazione aperta rilegge il thread con `hub_get_thread` quando
`message_count` o `updated_at` cambiano.

`hub-thread-removed`, emesso dopo il salvataggio di un `hub_delete_thread` riuscito, con payload
`HubThreadRef { channel, thread_id }` del thread eliminato (vedi "Eliminare a mano").

`hub-secretary-status`, emesso dal motore dei segretari a ogni cambiamento delle sessioni, con payload
`HubSecretaryStatus[]` (l'elenco completo, vedi "Motore dei segretari"); `hub_secretary_status` lo legge all'apertura
della vista.

`hub-notify`, emesso dal backend con payload `HubNotice` quando una notifica è pronta (vedi "Notifiche");
`hub-notification-activated`, emesso al click su una toast dell'HUB, con payload `HubThreadRef { channel, thread_id }`.

## Interfaccia

Interruttore in Impostazioni › Generale › Agents HUB (spento di default). L'app ha due **mondi**: i *Terminali*
(workspace, Tasks Manager, Ufficio, come prima dell'HUB) e l'*Agents HUB*. Con l'HUB **acceso** una **barra dei
mondi** (`components/rail/`, 64px, `<nav aria-label="Mondi">`) sta a sinistra, sempre visibile: Terminali, HUB (con
la pillola dei thread che aspettano l'utente), Ufficio (con i bot attivi), poi Impostazioni (col pallino di
aggiornamento) e versione. Con l'HUB **spento** non c'è barra né vista né chiamate (all'avvio si legge solo
`hub_get_settings`) e il layout è quello di prima: la sidebar ha il suo piede con Impostazioni, Ufficio e versione.

Ogni zona dello schermo resta al suo posto; cambia il contenuto con il mondo:

| Zona | Mondo Terminali | Mondo Agents HUB |
|---|---|---|
| Barra del titolo | tab dei workspace (il riquadro del logo è largo 324px con la barra, 260px senza) | percorso `Agents HUB / # canale / titolo del thread` (`HubTitlePath`) |
| Colonna di sinistra (260px) | sidebar dei workspace, senza piede se c'è la barra | canali (`HubSidebar`): Generale e uno per workspace visibile, con la pillola dei thread in attesa |
| Centro | terminali, pannello Task, Tasks Manager, Ufficio | `HubOverlay`: thread del canale e conversazione |
| Barra di stato | come sempre, più «Agents HUB: N thread aspettano te» se N > 0 | «Agents HUB», N thread in attesa, conteggio dei terminali a destra (`HubStatusBar`) |

I terminali restano sempre montati e l'area di lavoro ha la stessa dimensione nei due mondi (barra e colonna hanno la
stessa larghezza); l'overlay dell'HUB li copre come l'Ufficio, e l'HUB e l'Ufficio si escludono (`setWorld`,
`setOffice`).

**Collegamenti tra i mondi.** Terminali → HUB: il bottone con il numero di thread in attesa accanto al nome del
workspace nella sidebar apre il suo canale; «Agents HUB: N thread aspettano te» nella barra di stato apre il canale del
thread in attesa più recente (altrimenti l'ultimo canale aperto). HUB → Terminali: «Terminali» nell'intestazione dei
thread (non nel Generale) mostra i terminali di quel workspace; «Apri nel Tasks Manager» nella conversazione apre il
manager; sugli eventi della board «apri il terminale del worker» (se il task ha una `session` e il tab è ancora aperto,
`workerTabOf`) mette a fuoco il tab. Ctrl+Maiusc+H passa da un mondo all'altro (solo con l'HUB acceso e nessun dialogo
aperto); non c'è una X per chiudere l'HUB, si esce dalla barra o dalla scorciatoia.

**Persistenza.** Il mondo attivo (`UiState.world`) non si salva nella sessione, come `officeOpen` e `managerOpen`:
all'avvio si ripristinano i terminali, l'HUB è spento finché le impostazioni non sono lette e partire dai Terminali
evita un lampo di vista sbagliata. Spegnere l'HUB riporta ai Terminali.

**Caricamento.** Acceso l'HUB (e quando cambia l'insieme dei canali) `useHubSync` legge l'elenco di ogni canale, perché
le pillole servono anche nel mondo Terminali; poi `hub-update` li tiene aggiornati. Le pillole contano solo i canali
dei workspace visibili (`totalWaiting`).

| Parte | Dove |
|---|---|
| Stato (interruttore, canale, filtro, riassunti per canale, thread aperto, risposta in corso) | `stores/hub.ts` |
| Mondo attivo (`world`, esclusivo con `officeOpen`) e derivazioni (`activeWorld`, `worldRailShown`, `sidebarFooterShown`) | `stores/ui.ts`, `stores/view.ts` |
| Canali dai workspace visibili | `stores/hubChannels.ts`, `hooks/useHubChannels.ts` |
| Canali, thread in attesa, percorso del titolo, controlli sui testi, decisioni di rilettura | `lib/hub.ts` |
| Resa degli eventi per tipo (etichetta, icona, tono; i tipi sconosciuti hanno la resa generica) e tab del worker | `lib/hubEvents.ts` |
| Chiamate al backend, `hub-update` e caricamento dei canali | `app/actions/hub.ts`, `hooks/useHubSync.ts` |
| Passaggi tra i mondi | `app/actions/view.ts`, `hooks/useShortcuts.ts` |
| Impostazioni `settings` e segretari svegli `secretaryStatus` (evento `hub-secretary-status`, lettura all'accensione) | `stores/hub.ts`, `app/actions/hub.ts`, `hooks/useHubSync.ts` |
| Logica pura dei segretari: menzionabili, `@` in corso, evidenziazione, stato, titolare, "sta scrivendo" | `lib/hubSecretaries.ts` |
| Pagina Impostazioni › Agents HUB: interruttore e tre accordion | `components/settings/HubPage.tsx`, `components/settings/hub/*` |
| Controlli, riepiloghi e bozza del segretario delle impostazioni; salvataggio (ogni comando ritorna le impostazioni, che finiscono nello store) | `lib/hubSettings.ts`, `app/actions/hubSettings.ts` |
| Messaggi dei segretari (avatar, badge «SEGRETARIO»), dell'utente (bolla a destra), menzioni evidenziate | `components/hub/HubMessageItem.tsx`, `components/hub/HubUserMessage.tsx`, `components/hub/SecretaryBadge.tsx`, `components/tasks/Markdown.tsx` |
| Compositore con autocompletamento di `@` | `components/hub/HubComposer.tsx`, `MentionMenu.tsx`, `useMentionAutocomplete.ts` |
| Allegati nel compositore (bozze, caricamento, incolla, trascinamento) e nei messaggi (miniature, chip, apertura) | `lib/hubAttachments.ts`, `app/actions/hubAttachments.ts`, `components/hub/useAttachmentDrafts.ts`, `useFileDrop.ts`, `AttachmentDraftChip.tsx`, `HubMessageAttachments.tsx`, `useStoredAttachment.ts`, `AttachmentIcon.tsx` |
| «sta scrivendo…», titolare del thread e sessione attiva | `components/hub/TypingIndicator.tsx`, `components/hub/ThreadOwner.tsx`, `components/hub/ThreadSession.tsx`, `components/hub/ListOwner.tsx` |
| Scheda della proposta (task espandibili, stato, «Rispondi») e link al task `plan` | `components/hub/HubProposalCard.tsx`, `HubProposalTask.tsx`, `HubEventRow.tsx`, `lib/hubProposals.ts` |
| Scheda della domanda di un worker (opzioni, «Rispondi», terminale), avviso «Risposta non consegnata», link al thread dal dettaglio del task | `components/hub/HubQuestionCard.tsx`, `HubDeliveryFailure.tsx`, `WorkerTerminalLink.tsx`, `useEventLinks.ts`, `components/tasks/DetailQuestion.tsx`, `lib/hubQuestions.ts`, `answerQuestion` e `openTaskThread` in `app/actions/hub.ts` |
| Task di pianificazione: badge «Pianificazione» e domande del pianificatore | `lib/plan.ts`, `components/tasks/PlanBadge.tsx`, `components/tasks/DetailResult.tsx` |
| Sezione SEGRETARI della colonna di sinistra | `components/hub/HubSecretaryList.tsx`, `components/hub/HubSidebar.tsx` |
| Nomi riservati ai terminali | `lib/bots.ts`, `reservedBotNames` in `stores/hub.ts`, `app/actions/tabs.ts`, `app/events.ts`, `hooks/useSessionPersistence.ts` |

Il mondo Agents HUB ha tre colonne: canali (a sinistra, nella colonna dello schermo), thread del canale (filtro
Aperti/Tutti, «+» nuovo thread) e conversazione; con la vista messenger le prime due diventano l'elenco delle chat a sinistra. Gli eventi sono righe compatte con
«Rispondi»: il compositore mostra il messaggio citato (annullabile) e invia con `reply_to`; la risposta riporta il
riferimento all'originale, che sparisce se il messaggio è stato potato. Invio invia, Maiusc+Invio va a capo, al massimo
8000 caratteri.

**Vista messenger: interfaccia.** In cima alla colonna sinistra l'interruttore segmentato **Canali | Chat**
(`HubViewSwitch`, `aria-pressed`, al posto del titolo «Canali») scrive la preferenza con `setHubViewMode`. Con
«Canali» tutto resta com'è (`HubChannelList`, `HubThreadList`, messaggi a riquadri). Con «Chat» la colonna sinistra
(`HubSidebar`) mostra `HubChatList` al posto dei canali e la colonna dei thread non c'è: `HubView` rende solo la
conversazione a tutta larghezza. `HubChatList` ha la ricerca («Cerca chat…»), il «+», il filtro Aperti/Tutti
(`HubSegmented`, lo stesso della vista per canale), il selettore del workspace (`ChannelSelect`: «Tutti i workspace»,
Generale, ogni workspace visibile) e un `<ul>` di `HubChatCard`, una per chat: avatar del segretario titolare (icona
neutra se manca), titolo (in grassetto se non letta), workspace, orario relativo, anteprima su una riga («…» mentre si
legge), pallino «Aspetta una tua risposta», pallino dei non letti; le chiuse sono attenuate, la selezionata ha
`aria-current`, l'`aria-label` riassume titolo, workspace, chiusa, non letta e attesa (`chatCardLabel`). Un clic fa
`openChat`. Stati vuoti: «Nessuna chat. Crea un thread con +» e «Nessuna chat corrisponde alla ricerca o ai filtri.».
Sotto restano i segretari (`HubSecretaryList`). Il «+» apre `NewThreadForm` con il selettore del workspace (default: il
canale della chat aperta, se no il Generale) e crea con `createChat`; nella vista per canale il modulo non ha il
selettore. La conversazione (`HubConversation`) ha nell'intestazione il nome del workspace e «Terminali»
(`WorkspaceTerminalsButton`, anche nella colonna dei thread) e mostra i messaggi dei segretari e del sistema come
bolle a sinistra (`HubBotBubble`, speculare alla bolla dell'utente); nella vista per canale restano i riquadri
(`HubBotMessage`). Eventi, proposte, domande, typing, compositore e allegati sono gli stessi. Senza chat scelta: «Scegli
una chat dall'elenco, oppure creane una con +». Badge «aspetta te» e notifiche non cambiano: le notifiche sono soppresse
solo per il thread aperto e in vista, che `openChat` imposta insieme al canale.

**Vista messenger (stato lato client).** Seconda modalità di visualizzazione dei thread, accanto a quella per canale: a
sinistra le chat (un thread = una chat) di tutti i canali visibili, a destra la conversazione scelta. È solo una
presentazione: i dati, i DTO, i comandi e il feed non cambiano e non c'è nessun feed cronologico unico dei messaggi.
La modalità (`"channels"` di default, `"messenger"`) è una preferenza globale in `HubState.viewMode`, salvata in
localStorage (`lib/hubViewMode.ts`; un valore assente o non valido vale `"channels"`, senza localStorage vale in
memoria) e cambiata da `setHubViewMode`. L'elenco delle chat è `chatList` (`lib/hubChats.ts`) sui riassunti che
`useHubSync` già tiene aggiornati: canali visibili uniti, dal più recente (a parità per id), filtro Aperti/Tutti
condiviso con la vista per canale, ricerca su titolo e nome del workspace, filtro per un solo canale. Il backend non
dà né un'anteprima né uno stato «letto», quindi si ricavano nel client. Le **anteprime** (`HubState.previews`) sono
l'ultimo messaggio di ogni chat, letto con `hub_get_thread` solo per le prime `CHAT_PREVIEW_LIMIT` (30) chat
mostrate, con al più `CHAT_PREVIEW_CONCURRENCY` (4) letture attive in tutto (un solo limitatore di modulo,
`createLimiter` in `lib/mapLimit.ts`, anche con più `loadChatPreviews` sovrapposte) e solo con la vista messenger attiva e l'HUB mostrato
(`loadChatPreviews`); un thread si rilegge solo se il suo `updated_at` è cambiato (quindi `hub-update` le tiene
aggiornate), mentre la conversazione aperta aggiorna la propria anteprima da quella in memoria. Un errore di lettura
si registra senza toast e non blocca l'elenco; spegnere l'HUB svuota la cache. I **non letti** (`HubState.lastSeen`,
`lib/hubUnread.ts`) sono i messaggi visti per thread, salvati in localStorage e potati dei thread spariti: una chat è
non letta se il suo `message_count` supera i visti. Al primo avvio, senza dati salvati, i thread esistenti si
considerano letti. Una chat si segna letta (`markOpenChatSeen`) solo quando è aperta e in vista (la stessa regola di
`threadIsInView`: finestra in primo piano, mondo HUB, quel canale e quel thread), all'apertura, a ogni messaggio
ricevuto e al ritorno del focus sulla finestra (evento `focus`, in `useHubSync`). `openChat` imposta canale e thread insieme: notifiche e click sulle notifiche (`openHubThread`,
`openTaskThread`) funzionano com'è, anche con la vista messenger. Il codice sta in `lib/hubChats.ts`,
`lib/hubUnread.ts`, `lib/hubViewMode.ts`, `stores/hubPrefs.ts`, `app/actions/hubMessenger.ts` e
`app/actions/hubChatState.ts`.

**Allegati nel compositore.** Tre vie, tutte verso lo staging (vedi "Allegati"): il pulsante «Allega» (graffetta,
accanto a Invia) apre il dialog di sistema a selezione multipla e carica da percorso (`hub_stage_attachment_file`);
incollare nella textarea file o immagini (uno screenshot) e trascinare file sul compositore caricano dai byte
(`hub_stage_attachment_bytes`; con `dragDropEnabled: false` il webview riceve i `File` senza percorso). L'incolla
allega solo se negli appunti non c'è testo (`pasteAttaches`): incollare del testo resta com'era. Durante il
trascinamento di file il compositore ha il bordo tratteggiato e «Rilascia i file per allegarli» (`useFileDrop`; altri
trascinamenti non lo toccano). Gli allegati in attesa sono chip sopra la textarea (`AttachmentDraftChip`: miniatura per
le immagini incollate o trascinate, icona per gli altri e per quelli scelti col dialog, nome, dimensione, rotellina
durante il caricamento, X «Rimuovi …»). I limiti si controllano prima di caricare (`admitFiles`, con le costanti del
backend: 5 allegati, 10 MB; la dimensione di un file del dialog la controlla il backend): i file in più o troppo grandi
restano fuori, con il messaggio nella riga d'errore, e il testo resta. Un caricamento fallito toglie il suo chip e
scrive l'errore. Invia è attivo con testo o almeno un allegato, mai durante un caricamento (`canSend`); con soli
allegati il testo può essere vuoto (`checkBody(text, allegati)`). Dopo l'invio riuscito testo e chip si svuotano; se
l'invio fallisce restano, e gli stessi id si possono reinviare. Le miniature sono Blob URL dei `File`, revocati alla
rimozione, all'invio e allo smontaggio (anche cambiando thread: gli allegati non inviati restano nello staging e il
backend li toglie dopo 24 ore).

**Allegati nei messaggi.** Sotto il testo della bolla dell'utente (`HubMessageAttachments`; senza testo solo gli
allegati): le immagini (`hasPreview`: PNG, JPEG, GIF, WebP, BMP, AVIF, ICO; non SVG) come miniature, lette con
`hub_read_attachment` in un Blob URL (l'asset protocol non è abilitato) revocato allo smontaggio; gli altri file come
chip con icona per tipo, nome e dimensione (`formatSize`: «512 B», «1,5 KB», «10 MB»). Un clic apre il file con l'app
predefinita (`hub_attachment_path`, poi `openPath` del plugin opener; la capability `opener:allow-open-path` vale solo
per `$APPDATA/hub_attachments/channels/**`). `useStoredAttachment` dipende da id e tipo, non dall'oggetto: rileggere
il thread non ricarica i file. Un allegato il cui file non c'è più (o non si legge) è un chip barrato «non
disponibile», non cliccabile, e la conversazione resta com'è.

### Impostazioni dei segretari

Impostazioni › Agents HUB (`HubPage`): l'interruttore e, sotto, tre **accordion** (`HubAccordion`: l'intestazione è un
pulsante con `aria-expanded` collegato al suo pannello `role="region"`). Ognuno mostra a destra un **riepilogo** anche
da chiuso. Le impostazioni si vedono e si salvano anche a HUB spento (`hub_get_settings` le legge all'avvio e finiscono
in `settings` di `stores/hub.ts`; ogni comando di salvataggio ritorna le impostazioni aggiornate). Finché non sono
arrivate la pagina scrive «Impostazioni non disponibili.» e gli accordion non compaiono.

| Accordion | Contenuto | Riepilogo |
|---|---|---|
| **Modello** (aperto di default) | endpoint e nome del modello (un solo modulo, «Salva» attivo solo se qualcosa è cambiato, controllo prima dell'invio); sotto, la API key | «Non configurato», «Endpoint locale» / «Endpoint remoto» / «Endpoint non valido» più «API key impostata» o «nessuna API key» |
| **Sessioni dei segretari** | minuti di inattività (1-240) e passi massimi per risveglio (1-20); l'interruttore «Notifiche di sistema» (vedi "Notifiche") | «20 min · 8 passi» |
| **Segretari** (aperto di default) | lingua delle risposte, elenco, editor, «Nuovo segretario» e «Aggiungi da modello» (disattivati oltre 20) | «3 segretari · 2 configurati · italiano» |

- **API key** (`ApiKeyField`): mostra solo se c'è (`api_key_set`), con Imposta/Cambia e Rimuovi. La chiave non si
  mostra mai e non sta in nessuno stato React né nello store: vive solo nel campo (non controllato, `type="password"`)
  finché non si invia, poi il campo si svuota, anche se il backend rifiuta. Resta nel portachiavi (vedi "API key").
  **Non c'è un pulsante «Prova la connessione»**: il backend non ha un comando per farlo; un endpoint sbagliato si
  scopre alla prima menzione, dal messaggio di sistema del segretario (vedi "Ciclo di tool calling").
- **Elenco dei segretari** (`SecretaryRow`): avatar, `@nome`, prima riga del system prompt, tool e canali ("tutti i
  canali", "Generale, sito-web" o "nessun canale"). Un segretario **non configurato** (system prompt vuoto) ha il
  bordo tratteggiato, l'avatar attenuato e la scritta «Non configurato: non risponde alle menzioni»; il pulsante è
  «Configura» invece di «Modifica». «Elimina» chiede una conferma nella riga.
- **Editor** (`SecretaryEditor`, uno solo aperto alla volta): nome (`@` fisso davanti; minuscole, cifre e `-`, fino a
  24 caratteri), immagine, system prompt (con l'avviso che lo legge il modello, quindi in inglese), tool consentiti
  (una casella per tool) e canali ("Tutti i canali, anche quelli futuri" oppure "Solo questi" con Generale e i
  workspace visibili). «Salva» invia tutto insieme; i controlli del frontend (`checkDraft`) riprendono quelli del
  backend, che resta l'ultima parola.
- **Immagine** (`SecretaryImageField`): la stessa pipeline pixel-art delle immagini dei bot (`useBotImagePicker`),
  con anteprima prima di usarla. L'immagine viaggia con comandi separati dopo il salvataggio dei campi: se non viene
  accettata il segretario è comunque salvato e l'editor lo dice («Segretario salvato, ma l'immagine no: …»).

### Conversazione

- **Messaggi** (`HubMessageItem`): un messaggio di un segretario ha l'avatar (la sua immagine, o quella
  deterministica), `@nome`, il badge **SEGRETARIO** (`SecretaryBadge`) e uno sfondo con il colore primario. Quelli
  dell'utente (`HubUserMessage`) stanno a destra, come in una chat: una bolla con sfondo `surface-2`, larga al massimo
  il 75% della conversazione, senza nome né avatar, con il riferimento della risposta e l'ora dentro la bolla.
  Segretari, sistema ed eventi restano a sinistra. Gli eventi sono righe compatte (`HubEventRow`). Quelli di un task
  concluso (`task_done`, anche di una revisione, e `task_failed`) con un riepilogo hanno sotto «Mostra report» /
  «Nascondi report» (`HubEventReport`, chiuso di default, stato locale al componente): espanso mostra il riepilogo
  intero del report (markdown) e la nota presi dal task nella board, in un'area con altezza massima e scroll interno.
  Se il task non è più nella board ripiega su `summary` e `note` dell'evento e, se `summary_chars` / `note_chars`
  dicono che erano accorciati, lo scrive («Testo accorciato: N caratteri in totale, il task non è più nella board»).
  La scelta del testo è `eventReport` in `lib/hubEvents.ts`.
- **Menzioni evidenziate**: nel testo, `@nome` di un segretario che esiste si evidenzia (`splitMentions` in
  `lib/hubSecretaries.ts`, usato da `Markdown` tramite la prop `mentions`). La grammatica è quella del backend
  (`engine/mentions.rs`): `posta@mario.it` o `@màrio` non sono menzioni; un nome che non è di un segretario resta
  testo normale. Si evidenziano anche i segretari non configurati.
- **Autocompletamento di `@`** (`HubComposer`, `MentionMenu`, `useMentionAutocomplete`): scrivendo una menzione
  compare un menu con i segretari **configurati e ammessi nel canale** (`mentionable`), il cui nome inizia con quanto
  scritto. Frecce scorrono, Invio e Tab completano (con `@nome ` e il cursore dopo), Esc chiude solo il menu. Non
  compaiono i segretari senza system prompt né quelli esclusi dal canale: come nel backend, non si sveglierebbero.
- **«<nome> sta scrivendo…»** (`TypingIndicator`): sotto i messaggi, mentre almeno un segretario ha `working` sul
  thread («mario e luisa stanno scrivendo…» se sono due). L'area `role="status"` resta nella pagina per l'annuncio
  agli screen reader.
- **Titolare del thread** (`ThreadOwner`): nell'intestazione, il segretario `owner` del thread (quello che vi ha aperto
  l'ultima pianificazione) con avatar e «· titolare». Nell'elenco dei thread ogni riga con un titolare ne mostra l'avatar
  (`ListOwner`), anche se il thread non è aperto: `owner` viaggia nel riassunto (`HubThreadSummary`) e in `hub-update`.
- **Sessione attiva** (`ThreadSession`): accanto al titolare, il segretario con la sessione che scade per ultima
  (vedi "Menzioni e risveglio"), con «sta lavorando» oppure «scade tra N min» (arrotondato per eccesso, «scade a momenti»
  a fine corsa). Niente se nessuno è sveglio; può essere un segretario diverso dal titolare.
- **Scheda della proposta** (`HubProposalCard`): l'evento `proposal_created` non è una riga ma una scheda «Proposta
  pronta · N task» con lo stato (In attesa / Accettata / Scartata, ricavato dall'ultimo evento della proposta nel thread:
  `proposalStatuses`), i task con id, modello e dipendenze, ciascuno espandibile per leggere la specifica in markdown
  (`HubProposalTask`). I task si leggono dalla board del workspace già nello store finché la proposta è pendente: l'evento
  porta solo `proposal_id`. Decisa, la proposta lascia la board e la scheda ripiega sul testo del messaggio (che elenca
  i titoli). La scheda **non decide**: «Rispondi» (in evidenza finché è pendente, con il suggerimento «Rispondi per
  accettare o scartare») cita il messaggio e la decisione passa dal segretario (`decide_proposal`).
- **Link al task**: la riga di ogni evento di un task ancora nella board ha il link «apri il task nel Tasks Manager»
  (`eventTaskOf`); per un `plan` (anche «proposta pronta» e «senza proposta») è «apri il task di pianificazione nel
  Tasks Manager». Un task eliminato o archiviato non ha link. Decisioni (`proposal_accepted` /
  `proposal_rejected`) e risvegli del titolare sono righe e messaggi del segretario come gli altri.

### Colonna SEGRETARI

Sotto i canali, `HubSidebar` mostra la sezione **Segretari** (`HubSecretaryList`): per ciascuno avatar, `@nome` e lo
stato, ricavato da `secretaryStatus` (`secretaryActivity`):

| Stato | Quando | Resa |
|---|---|---|
| **non configurato** | system prompt vuoto | avatar e nome attenuati |
| **dorme** | configurato, nessuna sessione aperta | avatar attenuato |
| **sveglio · scade tra N min** | almeno una sessione aperta; scade l'ultima | pallino verde |
| **al lavoro** | lavora su almeno un thread | pallino verde che pulsa |

Senza segretari la colonna invita a crearne uno e il collegamento «Creane uno nelle impostazioni» apre la pagina.
`secretaryStatus` si legge all'accensione (`hub_secretary_status`) e poi si sostituisce per intero a ogni
`hub-secretary-status`; con l'HUB spento l'evento si ignora e lo stato si butta. Il tempo che resta si aggiorna con
l'orologio condiviso del Tasks Manager (`useManagerNow`).

### Nomi riservati

I nomi dei segretari non diventano mai il nome di una sessione di terminale. `reservedBotNames` (`stores/hub.ts`) li
ricava dalle impostazioni e lo passano a `pickBotName` (`lib/bots.ts`) quando si apre un tab (`tabsStore.open`, da
`app/actions/tabs.ts` e `app/events.ts`) e a `assignRestoredNames` quando si ripristina la sessione
(`hooks/useSessionPersistence.ts`, che prima aspetta `loadHubSettings`): un tab ripristinato con un nome diventato
riservato ne riceve uno nuovo. Il confronto non distingue le maiuscole. **Limite**: un tab già aperto non viene
rinominato se poi nasce un segretario con quel nome; il nome cambia al prossimo avvio dell'app.

## Eventi della board

Con l'HUB acceso, ciò che succede sulla board di un workspace diventa **messaggi evento** (`author: system`,
`kind: event`) nei thread del canale di quel workspace, raggruppati per obiettivo. È la base su cui reagiranno i
bot della tappa 2. Codice in `hub/events/`.

### Aggancio

`automation::AutomationState::publish`, quando una revisione della board viene davvero pubblicata (non una
revisione vecchia scartata), dopo `emit_board` chiama `Host::board_published(view)`. `TauriHost` la inoltra a
`hub::board_published`, che con l'HUB spento non fa nulla; acceso, ricava dalla `BoardView` un'**istantanea
compatta** (`events/snapshot.rs`) e la accoda a `HubFeed` (`events/feed.rs`).

Perché una coda: `publish` tiene il lock delle pubblicazioni, così le viste arrivano nell'ordine delle revisioni.
Sotto quel lock si fanno solo l'istantanea e un invio su un canale `mpsc`; un thread dedicato
(`hub-board-events`, avviato alla prima vista con l'HUB acceso e riavviato se si ferma) riceve le istantanee
nello stesso ordine, fa il diff, instrada e salva. Il lavoro su disco dell'HUB non allunga mai il lock della
board. Il canale non è limitato: i messaggi sono istantanee compatte e arrivano al ritmo delle modifiche della
board.

### Istantanee e base di partenza

Il thread tiene in memoria l'ultima istantanea di ogni workspace: per task stato, attenzione, titolo, tipo e,
per raccontare gli eventi, riassunto del report, verdetto e nota (ripuliti dai caratteri di controllo e accorciati
a 1000 caratteri con «…», con accanto la lunghezza del testo intero se accorciati: diventa `summary_chars` /
`note_chars` dell'evento); per un task `plan` anche il riassunto intero, per riportare tutte le domande del
pianificatore; per proposta id e task proposti.

- All'**avvio dell'app** la prima vista pubblicata di un workspace fa solo da base, senza eventi (il primo giro
  dell'automazione ripubblica tutte le board): i `running` diventati `failed` per il riavvio non generano messaggi.
- All'**accensione dell'HUB** (solo il passaggio da spento ad acceso di `hub_set_enabled`) non si pubblica nulla:
  la base di ogni board è fissata subito con la sua vista corrente, senza messaggi né scritture. Così già la
  prima modifica successiva produce i suoi eventi (per esempio la prima proposta dell'Intake crea il suo thread
  con il messaggio di proposta). Le viste arrivano da una sorgente che `lib.rs` registra
  (`automation::board_views` → `hub::set_board_source`): `hub` non dipende da `automation`. Se nel frattempo è
  già arrivata una vista pubblicata, la base resta quella.
- Ogni istantanea porta la revisione della board: una vista più vecchia della base (la vista letta
  all'accensione può essere più recente di una pubblicazione ancora in volo) viene scartata senza eventi.
- Spegnere l'HUB butta le istantanee; rimuovere un workspace butta la sua.
- Gli eventi avvenuti ad app chiusa o con l'HUB spento non vengono ricostruiti.

### Diff (`events/diff.rs`, puro)

Dall'istantanea precedente e da quella nuova, in quest'ordine:

| Evento | Quando | `kind` |
|---|---|---|
| Proposta comparsa | id nuovo tra le proposte | `proposal_created` |
| Proposta accettata | sparita e almeno uno dei suoi task è comparso nella board | `proposal_accepted` |
| Proposta scartata | sparita senza task comparsi | `proposal_rejected` |
| Task creato | comparso e non viene da una proposta accettata in questo passo | `task_created` |
| Task avviato | stato diventato `running` | `task_started` |
| Task completato | stato diventato `done` | `task_done` |
| Task fallito | stato diventato `failed` | `task_failed` |
| Attenzione | `attention` da falso a vero | `task_attention` |
| Domanda posta | il task ha una domanda in attesa (`TaskView.question`, tool `ask_user` di worker e pianificatore) con un id diverso da prima | `task_question` |
| Domanda risposta | la domanda in attesa di prima non c'è più e il task è ancora `running` (risposta dal thread o nel terminale); l'evento porta la domanda risposta | `task_question_answered` |
| Task eliminato | sparito senza essere `done` | `task_deleted` |

Un task appena comparso parte da "in coda, senza attenzione" (un task accettato e già avviato dà anche
`task_started`). Nello stesso passo la risposta a una domanda viene prima della domanda successiva; un task che
finisce, fallisce o sparisce con una domanda in attesa dà solo il suo evento (la domanda non aspetta più nessuno:
vedi `waiting_user`). L'istantanea tiene la domanda in attesa ripulita dai caratteri di controllo (le opzioni
vuote dopo la pulizia si scartano). Tornare in coda (Riprova, stop, archiviazione del workspace) non è un evento: si vedrà alla
ripartenza. Un task sparito da `done` è stato archiviato e non è un evento. **Assunzione**: dalla vista non si
distingue l'archiviazione di un task `failed` (oltre i 200 task conclusi) dalla sua eliminazione: vale come
eliminato.

### Instradamento (`events/route.rs`, puro)

Sui thread del canale del workspace, con tempo e id passati dal chiamante:

- **proposta comparsa** → nuovo thread intitolato col primo task (`"<titolo> +N"` se più d'uno), proposta aggiunta a
  `proposal_ids`, messaggio con l'elenco dei task (i primi 20, poi "… e altri N"). Fa eccezione la proposta di un
  **pianificatore o di un revisore** (`source_task` = un task `plan` o `review`) il cui task è già in un thread
  (il piano aperto da un segretario con `open_plan_task`, la review entrata col task accettato): va **sempre** in
  quel thread, anche se ha già altre proposte pendenti; il thread la aggiunge a `proposal_ids`, viene riaperto se
  era chiuso e sveglia il titolare. Così anche i task accettati entrano nel thread d'origine e una review fra loro
  vi riporta i suoi follow-up, a catena. Ripiega sul thread nuovo solo se il task non ha thread o se i task proposti
  superano `TASKS_PER_THREAD_MAX` (ultima difesa). Le proposte dell'Intake (nessun `source_task`) aprono un thread
  proprio;
- **proposta accettata** → stesso thread, task collegati, «Proposta accettata: N task in coda.». Se la proposta non
  ha un thread (pendente già prima dell'accensione) lo riceve con le stesse regole della proposta comparsa: il thread
  d'origine del suo `source_task` (portato dall'evento `ProposalAccepted`), altrimenti uno nuovo. **Scartata** →
  «Proposta scartata.» e thread chiuso, a meno che abbia task diversi dai `plan` o un'altra proposta pendente:
  allora resta aperto e lo chiude la conclusione dei suoi task o l'ultima proposta scartata (nessun thread se la
  proposta non ne aveva uno). Decisa, la proposta si toglie da `proposal_ids`;
- **task creato a mano** → nuovo thread col suo titolo e quel task;
- **eventi di un task** → il thread a cui è collegato; se non ce n'è uno (task già esistente all'accensione) lo
  si crea al primo evento. Un task eliminato senza thread non ne crea uno;
- un evento di un task (non l'eliminazione) **riapre** il suo thread se era chiuso;
- un task `plan` concluso senza aver fatto una proposta (né sulla board né già riportata nel thread) scrive
  «Pianificazione conclusa senza proposta: <titolo>» (o «Risposta: <titolo>» se il report inizia con «Answer:») con
  il riassunto intero, che contiene le domande o la risposta del
  pianificatore, diviso in parti se supera un messaggio (vedi "Testi lunghi divisi in parti"): la prima parte porta
  `event` e la notifica, le altre solo il testo, e il titolare si sveglia dall'ultima, vedendole tutte;
- alla fine del passo, un thread toccato e aperto i cui task collegati sono tutti `done` o spariti dalla board si
  **chiude** con «Tutti i task sono conclusi: thread chiuso.» (messaggio di sistema senza `event`). Serve almeno un
  task che non sia un `plan` (un piano da solo aspetta la proposta o le risposte) e nessuna proposta del thread
  ancora pendente.

Nei thread con un **titolare** l'instradamento restituisce anche i risvegli (`EventWake`: thread, segretario,
messaggio da cui parte il turno), vedi "Risvegli da evento".

Testi (`events/texts.rs`), una riga ciascuno: «Task creato: <titolo>», «Task avviato: <titolo>», «Task finito:
<titolo>», «Revisione finita: <titolo> (esito: superata | problemi trovati)», «Task fallito: <titolo>» (con
` — ` e la prima riga non vuota della nota, o del riassunto se manca, tagliata a 120 caratteri con «…»), «Il task
richiede attenzione: <titolo>», «Domanda risposta: <titolo>», «Task eliminato: <titolo>». Fa eccezione
«Pianificazione conclusa senza proposta: <titolo>» (o «Risposta: <titolo>»), che porta sotto le domande o la risposta del pianificatore (nel thread non si
vedono altrove), e la domanda: «Domanda del worker: <titolo>» (o «Domanda del pianificatore: …» per un task `plan`),
poi il testo, «Opzioni: a · b» se ci sono e «Usa «Rispondi» su questo messaggio: la tua risposta arriva al worker
così com'è.». Il riassunto del worker
non è nel testo: il campo `event` porta i dati completi (`workspace`, `task_id`/`proposal_id`, `status` e, per
`task_done`/`task_failed`, `summary`, `verdict` e `note` del tentativo concluso) e li legge il segretario. La riga
nell'interfaccia (`HubEventRow`) mostra solo il testo, più il link «apri il task nel Tasks Manager» se il task è
ancora nella board.

Un passo con eventi è **una sola** modifica atomica del canale (`HubState::update_when`, sotto il lock dell'HUB
e dopo il controllo che il workspace esista ancora), seguita da un `hub-update` per ogni thread toccato. Un passo
che non tocca nessun thread non scrive. Un evento che non si può riportare (per esempio un id non valido) finisce
nel log e non ferma gli altri; un thread creato per un collegamento che fallisce viene tolto.

## Notifiche

Con l'HUB acceso i messaggi dei thread producono una **notifica di sistema**. Il backend decide cosa notificare e
quando ed emette l'evento `hub-notify`; il **frontend** decide se mostrarla e la mostra. Codice in
`hub/notify.rs` e `hub/notify/`: `notice` (i DTO), `kind` (tipi, titoli, suoni, significatività), `text` (il
corpo, puro), `pending` (il raggruppamento, puro) `sinks` (i destinatari oltre al frontend e la decisione di chi riceve, puro) e `dispatch` (la colla con Tauri,
`HubNotifier`, stato gestito).

### Quando notifica

Mai per i messaggi dell'utente (anche quelli con `origin`, scritti da un canale di contatto) né per quelli di
sistema scritti dal motore (avvisi, «Risposta inviata»). Un messaggio che l'utente scrive **nell'app** (senza
`origin`) non è una notifica, ma i `NoticeSink` lo ricevono comunque, subito e senza raggruppamento
(`NoticeSink::app_message_written`): i canali di contatto lo rispecchiano nel topic del thread (docs/CONNECTORS.MD §10,
«Eco dei messaggi scritti nell'app»).

| Origine | Tipo | Titolo | Suono |
|---|---|---|---|
| Risposta di un segretario in un thread (`append_text`) | Reply | «@nome ti ha risposto» | ding |
| Messaggio di un mittente esterno (`deliver_external`) | External | «Nuovo da <nome>» | ding |
| Domanda di un worker | Question | «Un worker ha una domanda» | attenzione |
| Task fallito | TaskFailed | «Task fallito» | attenzione |
| Task in attenzione | TaskAttention | «Un task richiede attenzione» | attenzione |
| Task `plan` finito senza proposta, con le domande del pianificatore | PlanWithoutProposal | «Pianificazione conclusa: servono tue risposte» | attenzione |
| Task `plan` finito senza proposta, con la risposta (riassunto che apre con «Answer:») | PlanWithoutProposal | «Risposta pronta alla tua domanda» | attenzione |
| Proposta creata | ProposalCreated | «Nuova proposta di task» | ding |
| Thread concluso (chiuso dal router) | ThreadConcluded | «Thread concluso» | ding |
| Task di revisione finito | ReviewDone | «Revisione finita» | ding |
| Altro task finito | TaskDone | «Task finito» | ding |

Gli eventi della board li raccoglie `route` (`Routed.notices`, una `EventNotice { thread_id, message_id, kind, text }` per
messaggio che notifica, con il corpo del messaggio come testo; il mapping puro è in `events/notices.rs`) e li offre
`HubFeed` dopo i `hub-update`. Creato, avviato, eliminato, domanda risposta e proposta accettata o scartata non
notificano.

### Raggruppamento

Per thread, in una **finestra di 3 secondi** (`GROUP_WINDOW`) che parte dal primo candidato: se ne arrivano altri
dello stesso thread resta **uno solo**, il più significativo, e a parità l'ultimo arrivato. La significatività, dalla
maggiore: risposta, messaggio esterno, domanda, task fallito, task in attenzione, pianificazione senza proposta, proposta, thread
concluso, revisione, task finito. Così un evento seguito dalla risposta del titolare dà una notifica sola (la
risposta). A fine finestra `HubNotifier` ricontrolla le impostazioni, poi consegna (vedi "Destinatari"). Nessun lock
resta preso durante l'attesa, l'emissione o la consegna ai sink; spegnere l'HUB butta i candidati in attesa (`hub_set_enabled`).

### Contenuto

`HubNotice { channel, thread_id, message_id, title, body, sound }` (`sound` è `ding` o `attention`; `message_id` è il
messaggio del thread che ha causato la notifica, quello che ha vinto nel raggruppamento). `body` ha al massimo due
righe: «<canale> · <titolo del thread>» (il canale è il **nome** del workspace, «Generale» per il Generale,
«Workspace» se non esiste più: mai un id) e, se il messaggio non è vuoto, la sua prima riga non vuota, ripulita e
tagliata a 120 caratteri con «…» (mai in mezzo a un carattere).

### Destinatari

Oltre al frontend le notifiche raggruppate vanno ai **sink**: `pub trait NoticeSink: Send + Sync { fn deliver(&self, notice: &HubNotice); }`
(`hub/notify/sinks.rs`), che chi li possiede registra con `hub::set_notice_sink(app, Arc<dyn NoticeSink>)`, per esempio
un connettore che gira la notifica a un canale di contatto. `deliver` gira su un thread del pool bloccante, mai con
un lock dell'HUB preso, e può bloccare; non deve andare in panico.

La decisione è pura (`Targets::resolve(enabled, notifications, has_sinks)`): il raggruppamento parte se l'HUB è
acceso **e** (le notifiche di sistema sono accese **o** c'è almeno un sink). A fine finestra i sink ricevono ogni
notifica raggruppata **anche con l'interruttore delle notifiche spento** (con l'HUB spento nessuno riceve nulla);
l'evento `hub-notify` verso il frontend esce solo con HUB **e** interruttore accesi. Quindi l'interruttore
riguarda solo il toast del frontend.

### Visibilità, click, suoni (frontend)

Il frontend mostra la notifica se la finestra non è in primo piano, oppure se lo è ma il thread non è quello aperto
e visibile; con il thread visibile non si mostra nulla e non suona nulla. Un solo suono per notifica mostrata.
Su Windows la toast è cliccabile (`show_hub_notification`, WinRT): il click porta la finestra in primo piano ed
emette `hub-notification-activated` con `{ channel, thread_id }`, per aprire il thread; altrove si usa il plugin
delle notifiche, senza click. `show_hub_notification` valida il canale (`Channel::parse`) e l'id del thread
(`valid_ref_id`) prima di mostrare qualcosa; il primo rigo di `body` va nel primo testo della toast, il secondo nel
secondo.

### Impostazione

**Notifiche di sistema** (`notifications` in `HubSettings`, comando `hub_set_notifications`): **acceso di
default**, anche per i file scritti prima del campo (`Default` scritto a mano). Sta nell'accordion «Sessioni dei
segretari» della pagina Impostazioni. Conta solo con l'HUB acceso. Non spegne i sink registrati (vedi "Destinatari").

### Doppioni con `task-notify`

Con l'HUB attivo **e** `notifications` acceso, task fallito, in attenzione e domanda del worker notificano
dall'HUB (che apre il thread) e il frontend **salta** `task-notify` (guarda `enabled && notifications` nelle
impostazioni). Spento l'interruttore, o l'HUB, `task-notify` torna come prima. Il backend di `task-notify` non
cambia.

### Terminali silenziati

Con la stessa condizione (HUB acceso **e** `notifications` acceso) il frontend non fa partire nemmeno suono e
notifica di sistema degli eventi dei coding agent (`agent-event`, `hubCoversAgentEvent`): né per i terminali
dell'utente né per i worker, che altrimenti suonerebbero due volte (suono dell'evento + ding dell'HUB). Le
notifiche sono solo quelle dell'HUB. Lo stato del tab (✓/❗) e i contatori restano; il self-test delle Impostazioni
suona sempre. Lo stato si legge all'arrivo dell'evento: l'interruttore ha effetto subito.

## Motore dei segretari

Il motore fa parlare i segretari: quando l'utente scrive «@mario a che punto siamo?» in un thread sveglia **solo**
Mario, gli apre una sessione con un contesto piccolo, chiama il modello configurato con i soli tool consentiti a
Mario, esegue i tool in Rust e scrive la risposta nel thread con autore `{ "type": "bot", "name": "mario" }`. Gli
altri segretari restano fermi e non costano nulla. È un sottosistema del backend, staccato dai coding agents: non è
una skill di Claude Code o OpenCode e non passa da MCP. Codice in `hub/engine/`.

| File | Responsabilità |
|---|---|
| `chat.rs` | i tipi della conversazione (messaggi, chiamate a tool, richiesta, risposta, `ModelError`) e il trait `ChatModel` |
| `openai.rs` | il client reale (`OpenAiModel`, `async-openai`) e la classificazione degli errori |
| `tools.rs` | schema JSON, descrizione e validazione di ogni tool; quale board può leggere un segretario |
| `audit.rs` | avviso di audit delle modifiche e limite di modifiche per turno, dalla classe del tool |
| `output.rs` | board e thread in JSON compatto per il modello |
| `context.rs` | system prompt, messaggio di apertura di un turno, richiesta e pulizia del riassunto |
| `mentions.rs` | le menzioni `@nome` |
| `planning.rs` | `open_plan_task` (testo del task `plan`, Bot, collegamento, titolare) e `decide_proposal` (a quale proposta si lega e la decisione), generici sulla board |
| `bot_choice.rs` | i Bot-agente visti da un segretario: l'elenco nel system prompt e la verifica del Bot scelto (puro) |
| `relay.rs` | `relay_answer` (le domande in attesa, la scelta di quale e la consegna), generico sulla board |
| `board_ops.rs` | `task_action` e `board_control` (verifica sulla vista corrente, scrittura, esito), generici sulla board (`BoardWriter`) |
| `sessions.rs` | sessioni, titolare, code dei thread, scadenze (puro, tempo passato dal chiamante) |
| `turn.rs` | il ciclo di tool calling, la compattazione della conversazione, il riassunto |
| `texts.rs` | i messaggi di sistema in italiano |
| `runtime.rs` | lo stato Tauri `SecretaryEngine`, gli esecutori delle code e lo schedulatore delle scadenze |
| `jobs.rs` | i lavori di una coda (turno, riassunto) sull'app |
| `host.rs` | gli effetti reali: thread, board (viste e `BoardActions`), portachiavi; in `host/board_writes.rs` quelli di `task_action` e `board_control`, in `host/thread_state.rs` quello di `set_thread_state` (che passa da `hub/thread_state.rs`, lo stesso codice del comando) |
| `status.rs` | `HubSecretaryStatus`, l'evento `hub-secretary-status` e il comando `hub_secretary_status` |

La logica (menzioni, sessioni, ciclo, contesto, tool) è pura o generica sui trait `ChatModel`, `TurnHost` e
`ProposalBoard`: i test usano un modello finto (`engine/fake.rs`) che risponde con sequenze scritte a mano, un host
finto e una board finta (`engine/fixtures.rs`), senza rete.

### Allegati nel contesto dei segretari

Un segretario vede degli allegati solo **nome, tipo e dimensione** (`hub/attachments/listing.rs`, puro): mai il
contenuto né il percorso, e in questa versione nessuna immagine va al modello. Nel contesto del risveglio e nei
messaggi nuovi di un turno successivo (`output::message`, lo stesso di `thread_read`) il messaggio ha
`attachments: [{ name, type, size }]`, con la dimensione leggibile («1.5 KB»). Se il messaggio che apre il turno ha
allegati, dopo il testo c'è un blocco `Attachments:` con una riga `nome (tipo, dimensione)` per allegato e la nota che
il segretario non può aprirli: può dire all'utente «ho visto che hai allegato X» e, se la richiesta ne ha bisogno,
passarla alla pianificazione (`open_plan_task`), dove il pianificatore li legge (vedi "Pianificazione dal thread").
Senza allegati il contesto è identico a prima.

### Menzioni e risveglio

`hub_send` salva il messaggio dell'utente e poi chiama `SecretaryEngine::on_message`, che decide chi svegliare
(`Sessions::route`, puro). Fa eccezione la risposta («Rispondi») a un evento `task_question`: è la risposta
diretta al worker e non sveglia nessun segretario, nemmeno con una menzione (vedi "Domande dei worker").

- solo i messaggi dell'utente svegliano qualcuno **tramite le menzioni**: quelli dei bot, di sistema e di autori
  `external` mai (niente catene bot-bot, e un'email con «@mario» non sveglia nessuno: `Sessions::route` esce se
  l'autore non è `user`); gli eventi della board svegliano solo il titolare del thread, con le regole di "Risvegli
  da evento", e gli elementi esterni solo il segretario scelto dalla regola di consegna ("Risvegli da elementi
  esterni"). I messaggi dell'utente scritti da un canale di contatto (`origin` valorizzato) seguono questa stessa
  regola;
- una menzione è `@` a inizio testo o dopo un carattere che non può stare in un nome (`posta@mario.it` non lo è),
  seguita dal nome (lettere, cifre, `-`; il `-` finale non conta; minuscolo); un token che prosegue con altri
  caratteri di parola (`@màrio`, `@mario_1`) non è una menzione. Una menzione di un nome che non è un segretario si
  ignora;
- le menzioni di segretari **configurati** (con `system_prompt`) e **ammessi** nel canale svegliano quei
  segretari, nell'ordine del testo, ciascuno con la sua sessione (una sessione aperta sullo stesso thread si
  riprende). Un segretario non configurato o non ammesso produce un messaggio di sistema che lo spiega, in
  risposta al messaggio («@anna non è configurato: …», «@luca non può essere menzionato in questo canale.»);
- un messaggio **senza menzioni di segretari** sveglia il primo segretario ancora configurato e ammesso nel canale
  tra: l'**autore del messaggio a cui risponde** («Rispondi» su un messaggio di un segretario; non su un evento o un
  messaggio di sistema), il titolare della **sessione attiva** più recente, il **titolare del thread** (`owner`) e
  l'**ultimo segretario** che vi ha scritto (`Sessions::route` con `sessions::Fallback`, ricavato dal thread prima
  del lock del motore). Anche un segretario addormentato (sessione scaduta, app riavviata, thread chiuso da «Tutti i
  task sono conclusi», che resta chiuso) si risveglia così, con una sessione nuova. Se nessuno può rispondere, un
  messaggio di sistema in risposta lo dice e suggerisce di menzionare un segretario con `@nome`. Un messaggio che
  menziona solo segretari non svegliabili non ripiega sugli altri candidati.

### Risvegli da evento

Un secondo motivo di risveglio, oltre al messaggio dell'utente. Nei thread con un **titolare** (`owner`, il segretario
che vi ha aperto la pianificazione) questi eventi della board lo svegliano (`events/route.rs`):

| Evento | Messaggio da cui parte il turno |
|---|---|
| proposta pronta | «Nuova proposta: …» |
| pianificazione conclusa senza proposta | «Pianificazione conclusa senza proposta: …» (il riassunto porta le domande) |
| domanda di un worker o del pianificatore (`ask_user`) | «Domanda del worker: …» / «Domanda del pianificatore: …» |
| task fallito | «Task fallito: …» |
| task in attenzione | «Il task richiede attenzione: …» |
| tutti i task del thread conclusi | «Tutti i task sono conclusi: thread chiuso.» |

**Assunzione** (per contenere i costi: ogni risveglio è una chiamata al modello): avvio e completamento di un singolo
task, accettazione e scarto di una proposta (li ha decisi l'utente), risposta a una domanda (l'ha data l'utente) e
eliminazione di un task restano solo messaggi a template. Al massimo un risveglio per thread a ogni passo della board, dall'ultimo evento che lo chiede (il contesto
porta anche i precedenti). Un thread senza titolare riceve solo i messaggi.

Dopo aver salvato i thread (e rilasciato il loro lock) il thread degli eventi chiama `SecretaryEngine::on_events`, che
accoda il turno (`Sessions::route_event`) se il titolare è ancora configurato e ammesso nel canale; altrimenti nulla
(riga nel log). Il turno parte con una **sessione nuova** che si apre dall'evento: il primo messaggio porta i dati del
thread, gli ultimi messaggi, l'evento in JSON e le indicazioni per il titolare (in inglese: raccontare in poche righe;
una proposta si presenta e si chiede all'utente di usare «Rispondi» sul suo messaggio; un piano il cui riassunto apre
con «Answer:» è la risposta alla domanda dell'utente e si riferisce, senza aprire una nuova pianificazione; le altre
domande del pianificatore si pongono all'utente; la domanda di un worker si presenta in breve, con le opzioni, aiutando a capirla, e si chiede di
usare «Rispondi» sul suo messaggio; un fallimento si spiega; la fine si riassume). Se il titolare ha già una sessione aperta sul thread
l'evento diventa il suo turno successivo, con la conversazione che ha già (due sessioni dello stesso segretario sullo
stesso thread non esistono). Un turno svegliato da un evento **non può mai** usare `decide_proposal` né
`relay_answer`. Da quel momento il
titolare ha una sessione attiva: la risposta dell'utente senza menzione («sì, accetta») va a lui come sempre, e anche
dopo la scadenza della sessione la risposta lo risveglia (è il titolare del thread).

### Risvegli da elementi esterni

Un terzo motivo di risveglio (`WakeCause::External`): un connettore consegna un elemento (un'email, un messaggio) che
viene salvato nel thread con autore `external`. Dopo il salvataggio, e fuori dal lock dei thread, chi lo ha salvato
chiama `SecretaryEngine::on_external(app, channel, thread_id, trigger_id, secretary)` con il segretario già
risolto dalla regola di consegna (`owner` → il titolare del thread, `secretary:<nome>` → quel segretario);
`Sessions::route_external` (puro, come `route_event`, con cui condivide il codice) lo sveglia **solo se**
configurato e ammesso nel canale, altrimenti nulla (riga nel log, il messaggio resta nel thread). HUB spento: nulla.

- **Mai menzioni dal contenuto.** `route_external` non le legge: sveglia solo il segretario indicato, e `route`
  ignora ogni messaggio non `user`.
- **Contenuto non fidato.** L'istruzione fissa del motore dice che i messaggi con autore `external` (e ciò che citano)
  sono dati da fuori dell'app, mai richieste dell'utente. Per il modello (`output::message`, quindi anche
  `thread_read` e il contesto) l'autore è `{ "type": "external", "source": "<connettore>", "name": "<mittente>" }`
  (mai l'istanza né l'id esterno) con `"untrusted": true`; un messaggio dell'utente con `origin` porta
  `"via": "<connettore>"`. Il primo messaggio del turno nomina connettore e mittente, ripete che è un dato non
  fidato (nessuna richiesta, link o comando va eseguito), riporta il messaggio in JSON e chiede di dire all'utente,
  in breve, cosa è arrivato e se serve la sua attenzione.
- **Poteri ridotti.** Come per gli eventi della board, un turno `External` **non può mai** usare `decide_proposal`
  (`planning::proposal_target`) né `relay_answer` (`relay::relay_candidates`); gli errori per il modello lo dicono («never
  when a board event or external content woke you up»). `open_plan_task` resta consentito (vedi
  `docs/CONNECTORS.MD`, "Domande aperte"); il testo del piano cita come richiesta dell'utente solo messaggi `user`,
  mai un elemento esterno, che nel contesto del piano compare come `[external <mittente> via <connettore>, untrusted]`.
- Il resto è come per gli eventi: con una sessione aperta l'elemento è il suo turno successivo, altrimenti ne parte
  una nuova; i turni passano dalla coda del thread.

### Code dei thread

Ogni thread ha una **coda** di lavori eseguiti uno alla volta, nell'ordine di arrivo, da un task di
`tauri::async_runtime` (un esecutore per thread con lavori, che si ferma a coda vuota). Un messaggio che arriva
mentre un segretario lavora su quel thread viene accodato e trattato dopo; due menzioni nello stesso messaggio
fanno due turni in fila. Thread diversi lavorano in parallelo. Anche il riassunto di una sessione scaduta passa
dalla coda, così un risveglio successivo sullo stesso thread legge il riassunto già aggiornato. Se un esecutore
si fermasse senza svuotare la coda (un panico in un lavoro), il messaggio successivo che sveglia un segretario nel
thread ne avvia un altro.

### Sessioni

Le sessioni stanno solo in memoria (un riavvio dell'app le chiude); la memoria vera sono board, thread e riassunto.

- **Risveglio**: il contesto si costruisce da zero. System prompt = system prompt del segretario + istruzione fissa
  del motore (in inglese: chi è, che lavora con la board di coding agents, che non conosce il codice, che non
  decide questioni tecniche, che usa i tool per leggere lo stato, che parla dei workspace sempre per nome e non mostra
  mai i loro id, risposte brevi, e che un testo accorciato lo dice con la lunghezza intera e la chiamata che legge il
  resto, da fare prima di rispondere su quel testo: mai dire all'utente che un report è troncato né proporre di rifare
  il lavoro per riaverlo, e per riportare un testo lungo lo si riassume indicando il task, che l'utente legge intero
  nel Tasks Manager) + **dove si trova** (il Generale, o il workspace del canale col suo nome) e i
  **workspace che può consultare**, con nome e id (`workspace_names.rs`) + i **Bot** (vedi "Bot nel contesto") +
  «Always reply to the user in <Language>». Il primo messaggio porta i dati del thread (titolo, canale, stato, task e proposte da decidere collegati,
  **riassunto**), gli **ultimi 10 messaggi** prima di quello che lo ha svegliato (in JSON, con i dati degli eventi)
  e il messaggio che lo ha svegliato. Un evento con `summary`/`note` accorciati (vedi `summary_chars`/`note_chars`)
  porta per il modello, al posto di quelle lunghezze, il **cursore** `summary_truncated` / `note_truncated`
  (`output/text_cursor.rs`): `{ "total_chars": 20000, "read_with": { "tool": "task_details", "arguments": {
  "task_id": "t-1", "workspace": "ws-1", "field": "report.summary", "offset": 0 } } }` (per la nota `field: "note"`).
  La lettura parte da 0 perché il pezzo dell'evento è ripulito dai caratteri di controllo e la sua lunghezza non è un
  offset esatto nel testo della board; seguendo `next_offset` si legge il testo intero.
- **Sessione attiva**: la conversazione interna cresce solo dentro la sessione. Un turno successivo riceve i
  messaggi comparsi nel thread dal turno precedente (tranne i suoi, al massimo 10) e il nuovo messaggio. Gli esiti
  dei tool dei turni passati oltre 600 caratteri diventano un esito accorciato esplicito, sempre JSON valido ed
  entro 600 caratteri (`output/tool_result.rs::shortened`): `{ "shortened": true, "total_chars": N, "head": "…",
  "reread": { "tool": "task_details", "arguments": { … } } }`, con `head` l'inizio dell'esito e `reread` la chiamata
  originale (stesso `call_id` nel messaggio assistant precedente) solo per i tool di lettura: una modifica non si
  ripete mai, e argomenti troppo lunghi per il limite si omettono. Un esito corto resta identico. Nel riassunto di
  fine sessione ogni voce della conversazione tiene 800 caratteri, e una più lunga finisce con
  `[… truncated: N characters in total]`. La conversazione tiene al massimo 40 messaggi
  (togliendo i più vecchi fino a un messaggio dell'utente, così nessun esito resta senza la sua chiamata).
- **Chiusura** dopo `idle_minutes` senza attività (l'attività riparte a ogni messaggio instradato e alla fine di
  ogni turno; una sessione al lavoro o con un turno in coda non scade): il segretario chiede al modello, senza
  tool, il **riassunto aggiornato del thread** dal riassunto precedente e dalla conversazione della sessione (al
  massimo 10 righe, in inglese perché lo legge il modello); la risposta si ripulisce (niente caratteri di
  controllo né righe vuote, al massimo 10 righe e 1500 caratteri) e va in `HubThread.summary`. Se la chiamata
  fallisce, o il provider non è configurato, resta il riassunto precedente (errore nel log). Poi la conversazione
  si butta. Una sessione senza turni completati si chiude senza riassunto.
- **Timer**: un solo thread (`hub-secretary-sessions`) per tutte le sessioni, non uno per sessione: dorme fino
  alla prossima scadenza (tra 200 ms e 30 s, così un cambio di `idle_minutes` vale entro 30 secondi), chiude le
  sessioni scadute accodandone il riassunto e si ferma quando non ci sono più sessioni (riparte con la prossima).
  Il controllo è anche **pigro**: una sessione scaduta che lo schedulatore non ha ancora raccolto non fa da
  titolare, e una nuova menzione la chiude (riassunto in coda) prima di aprirne una nuova.

**Impostazioni cambiate durante una sessione.** Le sessioni aperte non si rompono: ogni turno rilegge le
impostazioni, quindi provider, modello, API key, lingua, system prompt, tool e `max_steps` valgono **dal turno
successivo** (il turno in corso finisce con quelli con cui è partito). Un segretario eliminato, rinominato,
svuotato del system prompt o escluso dal canale chiude la sua sessione al turno successivo, senza riassunto, con
«@mario non è più disponibile in questo canale: la sua sessione è chiusa.».

**Spegnere l'HUB** (`hub_set_enabled(false)`) chiude tutte le sessioni senza riassunto e annulla i lavori in corso
(`abort` degli esecutori). L'epoca del motore cresce: un lavoro già oltre l'ultimo punto di attesa non scrive più
nei thread. **Rimuovere un workspace** butta le sessioni e i lavori del suo canale.

### Ciclo di tool calling (`turn.rs`)

Un turno: conversazione → chiamata al modello con i soli tool del segretario → se il modello chiede dei tool, per
ognuno il codice controlla che sia nella lista del segretario, valida gli argomenti contro lo schema, lo esegue e
restituisce l'esito (JSON, al massimo `TOOL_RESULT_MAX` = 6000 caratteri: gli esiti sono fatti per starci, con
`task_details` e `thread_read` che dividono da sé i testi in pezzi; uno che lo supera è un difetto, con un
`log::warn!`, e il modello riceve al suo posto un errore in JSON valido con `total_chars` e l'inizio in `head`,
`output::oversized`, mai un JSON spezzato); altrimenti il testo è la risposta del segretario, che si
scrive nel thread ripulita e, se supera un messaggio, divisa in parti consecutive (vedi "Testi lunghi divisi in
parti"; una sola notifica). Al massimo `max_steps` chiamate al modello per turno.

| Caso | Esito per il modello | Nel thread |
|---|---|---|
| Tool non nella lista (o inesistente) | errore «Tool `x` is not available to you…», il turno continua | «@mario ha chiesto il tool «x», che non gli è consentito: richiesta rifiutata.» |
| Argomenti non validi (JSON non valido, non un oggetto, campi sconosciuti o mancanti, valori fuori dai limiti) | errore «Invalid arguments for `x`: …», il turno continua | «@mario ha chiamato «x» con argomenti non validi: richiesta rifiutata.» |
| Errore di esecuzione (workspace inesistente, board non ammessa) | errore con il motivo, il turno continua | — |
| Ultimo passo con altre chiamate a tool | le chiamate non si eseguono e non entrano nella conversazione | «@mario si è fermato dopo N passi senza una risposta finale. …» |
| Una modifica oltre il limite del turno (vedi "Classi dei tool, audit e limite") | errore «`x` was not run: you already made 3 changes in this turn…», il tool non si esegue, il turno continua | «@mario ha chiesto «x» dopo 3 modifiche in questo turno, il massimo: richiesta rifiutata.» |
| Endpoint irraggiungibile, timeout, HTTP d'errore, risposta non valida o vuota, provider non configurato, API key illeggibile | il turno finisce | «@mario non può rispondere: <motivo>.» |

Gli errori tornano al modello come `{"error": "…"}`. Nessun caso va in panico né lascia la coda bloccata: ogni
lavoro finisce e la coda passa al successivo. La conversazione resta valida anche dopo un errore (ogni chiamata a
tool ha il suo esito).

### Classi dei tool, audit e limite (`audit.rs`)

Ogni tool ha una classe (`SecretaryTool::class`, `ToolClass`), secondo l'effetto di una chiamata riuscita:

| Classe | Significato | Tool |
|---|---|---|
| `Read` | sola lettura | `board_status`, `board_overview`, `task_details`, `thread_read` |
| `Write` | modifica reversibile del thread o della board, senza conferma | `post_message`, `open_plan_task`, `decide_proposal`, `relay_answer`, `task_action`, `board_control`, `set_thread_state` |
| `Destructive` | modifica irreversibile o che cambia cosa farà l'agente; non si esegue mai da sé: chiede conferma all'utente | `request_task_action`, `request_task_update` |

- **Audit.** Una chiamata **riuscita** di un tool `Write` o `Destructive` lascia un messaggio di sistema (in
  italiano) nel thread, tranne quando il suo effetto si vede già: `post_message` è il messaggio stesso,
  `open_plan_task` e `decide_proposal` producono gli eventi del task creato e della proposta decisa. Gli avvisi,
  scritti dal turno e non dall'host: `relay_answer` («@mario ha inviato al worker del task t-7 questa risposta: …»,
  con il task a cui è andata), `task_action` («@ugo ha ritentato «Login».», «@ugo ha portato «Login» in testa alla
  coda.», «@ugo ha spostato «Login» prima di «Logout».») e `board_control` («@ugo ha messo in pausa la board di Sito
  Web.», «@ugo ha ripreso la board di Sito Web.», «@ugo ha impostato a 2 i task in parallelo della board di Sito
  Web.», o le due cose insieme), che nomina solo ciò che è cambiato davvero e non lascia avvisi se la board era già
  così, e `set_thread_state` («@mario ha chiuso il thread.», «@mario ha riaperto il thread.»), solo se lo stato è cambiato. Il testo si dichiara in `audit::notice`, con un `match` esaustivo sulle chiamate: un tool nuovo
  non compila finché non dice se lascia un avviso e quale. Una chiamata fallita o rifiutata non lascia avvisi di
  audit (solo, se rifiutata, quello del rifiuto).
- **Limite per turno.** Al massimo `MAX_MUTATIONS_PER_TURN = 3` chiamate riuscite per turno a tool `Write` diversi
  da `post_message` o `Destructive` (`audit::MutationBudget`, che il ciclo tiene per tutti i passi del turno).
  Oltre, la chiamata non si esegue ed è un rifiuto come un tool non consentito (`ToolRejection::TurnLimit`):
  errore per il modello e avviso nel thread (tabella sopra). Le chiamate fallite non consumano il limite; letture
  e `post_message` restano libere. `max_steps` non cambia.

La classe non è esposta al frontend.

### Tool

Schema JSON e descrizione (in inglese: li legge il modello) sono scritti in `tools.rs` (quelli di `board_status`, `board_overview`, `task_details`, `thread_read`, `relay_answer`, `task_action`, `board_control` e `set_thread_state` nei moduli di `tools/`) accanto alla struttura degli
argomenti in cui si validano (`#[serde(deny_unknown_fields)]`); un test verifica che le proprietà dello schema e i
campi della struttura coincidano. Gli argomenti vuoti valgono `{}`. Solo `open_plan_task`, `decide_proposal`
(vedi "Pianificazione dal thread"), `relay_answer` (vedi "Domande dei worker"), `task_action` e `board_control`
(vedi "Azioni sulla coda") agiscono sulla board; `request_task_action` e `request_task_update` (vedi "Azioni che chiedono conferma") la cambiano solo dopo il clic dell'utente.

| Tool | Argomenti | Effetto |
|---|---|---|
| `board_status` | `workspace?` (il **nome** del workspace, senza distinguere maiuscole e spazi ai bordi; vale anche l'id; al massimo 200 caratteri); filtri in AND: `status?` (array senza ripetizioni di `queued`, `running`, `done`, `failed`), `attention?` (`true` = solo i task che aspettano l'utente: il worker aspetta lui, una domanda senza risposta o l'agente non pronto), `kind?` (`work`, `review`, `plan`), `query?` (testo senza distinguere maiuscole in id, titolo, riassunto del report, nota e specifica del task; al massimo 200 caratteri; vuoto = nessuna ricerca); pagina: `updated_since?` / `updated_before?` (istante ISO 8601 su `updated_at`: una data `2026-11-05`, mezzanotte UTC, o data e ora con scarto `2026-11-05T09:30:00Z`; l'inizio è incluso, la fine esclusa; una data non valida o `updated_since` non anteriore a `updated_before` è `InvalidArguments`); pagina: `offset?` (da 0, default 0) e `limit?` (da 1 a 40, default 20) | una pagina della board del workspace del canale, o di quello indicato se il segretario ha quel canale, con `workspace` (id) e `workspace_name`: conteggi per stato dell'intera board (mai filtrati), proposte pendenti, `paused`, `total` (i task che soddisfano i filtri), `offset`, `limit`, `next_offset` (solo se ci sono altri risultati) e i task **dal piu' recente** (ora di creazione; a parita', l'ultimo aggiunto alla board) con `id`, `title`, `status`, `attention`, `waiting_on`, riassunto del report (300 caratteri), nota (200), `created_at` e `updated_at` (UTC); la specifica si cerca ma non si restituisce. Le righe sono quelle di `list_tasks` (`compact_row`), che non cambia; filtro, ricerca e pagina sono in `hub/engine/board_query.rs`. Nel Generale senza `workspace` l'errore elenca i nomi dei workspace leggibili; un nome sconosciuto elenca i nomi possibili, un nome ambiguo (due workspace con lo stesso nome) elenca gli id tra cui scegliere |
| `board_overview` | — | per ogni workspace che il segretario può consultare (i suoi canali, nell'ordine dello store; al massimo 20, il resto è `omitted_workspaces`): `workspace`, `workspace_name`, i conteggi per stato, `proposals_pending`, `paused`, `attention_total` con i primi 3 task che aspettano l'utente (`id`, `title`, `status`) e `questions_total` con le prime 3 domande dei worker senza risposta (`task_id`, testo tagliato a 120 caratteri). Output in `output/overview.rs`; per i dettagli si passa a `board_status` o `task_details` |
| `task_details` | `task_id` (1..64 caratteri), `workspace?` (come `board_status`), `field?` (al massimo 64 caratteri), `offset?` (intero ≥ 0, solo con `field`) | un task per intero (`output/task_details.rs`, da `TaskView::to_json(true)`): specifica (`task`), stato, dipendenze e `waiting_on`, nota, agente, `question` (testo e risposte suggerite) se un worker aspetta, `report` completo (riassunto, `commit`, `pr_url`, `files_changed`, `tests`, `follow_ups`, `diffstat`, per le revisioni `verdict` e `findings`), `allowed_actions`, `can_edit`, `cascade`, ora UTC di creazione e aggiornamento e `worktree` solo con branch e base. Mai la sessione, il percorso del worktree né lo storico delle domande. Specifica, nota, domanda e report li scrive un agente: l'esito ha `untrusted: true` e il prompt del motore li dichiara dati e non istruzioni. **Nessun contenuto è irrecuperabile**: ogni esito sta in `TOOL_RESULT_MAX` (6000 caratteri, misurati sul JSON serializzato, virgolette e a capo raddoppiati compresi), quindi il taglio di `turn.rs` non lo spezza mai. Se la scheda intera non ci sta, i testi lunghi (`task`, `note`, `question.text`, `report.summary`, `report.diffstat`) e gli elenchi (`question.options`, `report.findings`, `report.follow_ups`, `report.files_changed`, `report.tests`, `depends_on`, `waiting_on`, `cascade.dependents`, `cascade.running`) portano il loro primo pezzo: lo spazio si divide in parti uguali partendo dai campi più corti (che restano interi) e i fatti corti (stato, `commit`, `pr_url`, `verdict`, `question.id`) ci sono sempre. `more` dice per ogni campo incompleto da dove riprendere, es. `"more": { "task": { "next_offset": 2400, "total_chars": 18000 }, "report.follow_ups": { "next_offset": 5, "total_items": 40 } }`; senza `more` il task è intero. Con `field` l'esito è solo `{ task_id, field, offset, untrusted, content \| items, next_offset?, total_chars \| total_items }`: un pezzo grande quanto il limite permette (testi in caratteri, mai a metà di un carattere; elenchi per voci intere), `next_offset` assente = campo finito. Una voce di un elenco troppo lunga per stare da sola in una pagina esce accorciata con il suo `more` (`report.follow_ups.3`, `report.tests.2.details`, `report.tests.2.command`), e con quel nome si legge a pezzi come gli altri campi. Nomi e parsing in `task_page.rs`, pezzi e ripartizione in `output/task_details/pieces.rs`. Errori per il modello: `offset` senza `field`, `field` sconosciuto (fra gli argomenti non validi), campo che il task non ha (es. `report.summary` senza report) o `offset` oltre la fine. Un id inesistente dà un errore che rimanda a `board_status`; un workspace fuori dai canali del segretario è rifiutato come in `board_status` |
| `thread_read` | `before?` (id di un messaggio del thread) oppure `offset?` (quanti degli ultimi messaggi saltare, da 0), mai insieme; senza argomenti gli ultimi. Oppure `message_id?` (id di un messaggio, mai con `before`) con `offset?` in caratteri: legge il testo intero di quel messaggio a pezzi | titolo, canale (con `workspace_name` nel canale di un workspace), stato, task e proposte da decidere collegati, riassunto e una pagina di al massimo 20 messaggi, meno se lunghi perché la pagina sta sempre in `TOOL_RESULT_MAX` misurata sul JSON (si tengono i più recenti della finestra; `output/message_page.rs`) (autore, ora UTC, testo tagliato a 1000 caratteri con `body_truncated: { total_chars }` se lo è, dati dell'evento senza i campi vuoti; un messaggio che da solo non sta nella pagina ha il testo accorciato, sempre con `body_truncated`, e se non basta dell'evento restano solo gli id (`kind`, `workspace`, `task_id`, `proposal_id`, `status`, `question.id`, `request.id`) e i cursori `summary_truncated` / `note_truncated`, con `event_truncated: true`: mai una pagina vuota, e per ogni messaggio con allegati `attachments`: solo nome, tipo e dimensione, mai percorso né contenuto), più, se la pagina non parte dall'inizio, `older_messages` e `next_before` (l'id del primo messaggio escluso, da passare a `before` per leggere il resto senza buchi né ripetizioni) e, se non arriva alla fine, `newer_messages`. Con `message_id` l'esito è invece `{ message_id, offset, content, next_offset?, total_chars, untrusted? }`: un pezzo del testo entro `TOOL_RESULT_MAX`, con `next_offset` finché continua (stesse convenzioni di `task_details`; `output/message_body.rs`). I dati dei task negli eventi restano i riassunti tagliati alla creazione, con i cursori `summary_truncated` / `note_truncated` (vedi "Sessioni") che dicono la lunghezza intera e la chiamata a `task_details` che li legge interi. Un `before` o `message_id` sconosciuto, un `offset` oltre la fine o `before` insieme a `message_id` è un errore per il modello; la finestra è calcolata in `thread_page.rs` |
| `post_message` | `body` (1..53744 caratteri, `LONG_BODY_MAX`) | un messaggio intermedio del segretario nel thread, diviso in parti consecutive se supera 8000 caratteri (vedi "Testi lunghi divisi in parti"); oltre il limite è rifiutato fra gli argomenti non validi («`body` must be 1 to 53744 characters of plain text»); la risposta finale è comunque il testo del modello |
| `open_plan_task` | `goal` (1..4000 caratteri, ripulito dai caratteri di controllo), `bot` (obbligatorio, 1..64 caratteri: l'id di un Bot-agente abilitato che pianifica, cioè con `plan` fra i `kinds`) | crea un task `plan` in coda nel workspace del canale, eseguito dal Bot scelto, collegato al thread, e rende il segretario titolare; esito `{ task_id, title, status }`. Nel Generale è un errore per il modello: deve chiedere all'utente il progetto. Un Bot sconosciuto, disabilitato o senza `plan` è un errore per il modello, senza ripieghi e senza task creato |
| `decide_proposal` | `accept` (bool), **nessun id** | accetta o scarta la proposta dell'evento a cui risponde il messaggio dell'utente; esito `{ proposal, decision, tasks_queued }` |
| `relay_answer` | `body` (1..4000 caratteri, il limite delle risposte della board), `question_id?` e `task_id?` (al massimo 64 caratteri; vuoto vale assente) | consegna `body` al worker come risposta a una domanda in attesa del thread, solo in un turno svegliato da un messaggio dell'utente; senza id vale l'unica domanda in attesa, con più domande si passano `question_id` e `task_id` letti da `thread_read` (`event.question.id`, `event.task_id`) o da `task_details` (`question.id`); esito `{ task_id, question_id, delivered }` e nel thread «@mario ha inviato al worker del task t-7 questa risposta: …» |
| `task_action` | `task_id` (1..64 caratteri), `action` (`retry`, `to_top`, `move_before`), `before_task_id?` (1..64 caratteri, obbligatorio con `move_before` e vietato con le altre; vuoto vale assente), `workspace?` (come `board_status`) | ritenta un task fallito, porta un task in coda in testa alla coda o prima di un altro task in coda, della board del canale del thread; esito `{ workspace, task_id, title, status }` (più `before_task_id`, `before_title` per `move_before`) e un avviso nel thread. Vedi "Azioni sulla coda" |
| `request_task_action` | `task_id` (1..64 caratteri), `action` (`stop`, `delete`, `delete_cascade`, `mark_done`), `reason` (1..300 caratteri) | chiede all'utente di fermare, eliminare (anche con i task che ne dipendono) o segnare fatto un task: **non esegue nulla**, scrive nel thread una scheda con «Conferma» e «Annulla»; esito `{ request_id, task_id, title, action, status: "awaiting_confirmation", expires_in_minutes, note }`. Vedi "Azioni che chiedono conferma" |
| `request_task_update` | `task_id` (1..64 caratteri), almeno uno fra `title` (1..80), `task` (la specifica completa, fino a 100000), `depends_on` (l'elenco completo di id, fino a 50), `model` (stringa, o `null` per il modello predefinito dell'agente) e `bot` (l'id di un Bot abilitato, 1..64 caratteri; `null` è rifiutato: il Bot si cambia, non si toglie), `reason` (1..300 caratteri) | chiede all'utente di cambiare quei campi di un task `queued` o `failed`: **non cambia nulla**, scrive nel thread una scheda col diff (prima e dopo) e «Conferma» e «Annulla»; esito `{ request_id, task_id, title, action: "update", status: "awaiting_confirmation", expires_in_minutes, note }`. Un campo assente resta com'è. Un nuovo `bot` porta nel task agente, modello, effort e skill del Bot (un `model` passato insieme scavalca solo il modello). La validazione è quella del pannello Task e si fa **già alla richiesta** (un ciclo, un modello che non è dell'agente del task, un Bot sconosciuto o disabilitato o un task non modificabile sono rifiutati subito, senza scheda). Vedi "Azioni che chiedono conferma" |
| `board_control` | `paused?` (bool), `max_parallel?` (intero da 1 a 16, i limiti di `board_set`), almeno uno dei due; `workspace?` (come `board_status`) | mette in pausa o riprende la board del canale del thread e/o ne imposta `max_parallel`; esito `{ workspace, paused, max_parallel, changed }` con in `changed` solo le parti cambiate davvero (vuoto, con `note`, se la board era già così: nessuna scrittura) |
| `set_thread_state` | `state` (`closed` o `open`), **nessun id**: vale solo il thread del turno | chiude o riapre il thread in cui lavora il segretario, solo in un turno svegliato da un messaggio dell'utente (mai da un evento della board o da un elemento esterno); esito `{ state, changed }`. Riaprire non sveglia nessuno. Classe `Write`; l'avviso di audit compare solo se lo stato è cambiato. Opt-in: è nei preset Coordinatore e Operatore, non entra nei segretari esistenti |

**Workspace per nome**: gli id restano il riferimento interno (file, eventi, comandi), ma il modello parla dei
workspace col nome. `WorkspaceNames` (`workspace_names.rs`, puro) è l'istantanea dei workspace letta dal
`WorkspaceStore` a ogni turno: un workspace rinominato si vede subito col nome nuovo, perché nessun nome è copiato
nei file dei thread. Risolve il riferimento di `board_status` (prima l'id esatto, poi il nome) e aggiunge
`workspace_name` accanto all'id nei dati degli eventi (`event.workspace_name`) e nell'intestazione del thread.
Gli id dei task restano visibili all'utente; quelli dei workspace no.

**Bot nel contesto**: un segretario con `open_plan_task` o `request_task_update` trova nel system prompt, a ogni
turno, i Bot-agente **abilitati** (`bots::agent_bots`, letto dallo store dei Bot: un Bot modificato si
vede dal turno dopo) che gli servono (`engine/bot_choice.rs::describe`): con `open_plan_task` **solo quelli che
pianificano** (`kinds` con `plan`), una riga `- <id>: <nome> - <compito>`; con `request_task_update` quelli che
eseguono task, con i loro `kinds` (`- <id>: <nome> - <compito> [work, review]`), e sceglie solo un Bot adatto al
tipo del task. Se nessun Bot abilitato pianifica, il prompt dice che non si possono aprire pianificazioni e di far
abilitare un Bot con `plan`: nessun ripiego su `general`. Gli altri
segretari non lo ricevono. Nessun tool nuovo: è la soluzione più piccola, come i nomi dei workspace.

Il segretario non vede mai la API key né file del disco: i tool leggono solo board (dalla sorgente delle viste che
`lib.rs` registra con `automation::board_views`) e thread, e agiscono sulla board solo con le azioni che `lib.rs`
registra (`hub::set_board_actions`). Nessun lock dei thread o della board è tenuto durante
una chiamata al modello: i tool prendono i lock solo per leggere o scrivere, fuori dal main thread.

### Pianificazione dal thread

Il flusso della tappa 3: «@giorgio aggiungi X al progetto» → Giorgio chiama `open_plan_task` → un coding agent
pianifica → la proposta torna nello stesso thread come evento e sveglia Giorgio, che la presenta → l'utente usa
«Rispondi» sull'evento e scrive in libero («sì, accetta», «no, lascia stare») → Giorgio chiama `decide_proposal` →
gli eventi dei task svegliano Giorgio, che riassume. Codice in `engine/planning.rs` (puro), `engine/host.rs` (effetti)
e `hub/actions.rs`.

**Azioni sulla board.** `hub` non dipende da `automation`: `lib.rs` registra `hub::BoardActions` (puntatori a
funzione, come `BoardSource`) con `automation::queue_hub_plan_task` (crea il task `plan`) e
`automation::decide_hub_proposal` (lo stesso `proposal_decide` dei pulsanti Accetta/Scarta del pannello Task, con la
stessa validazione) e `automation::answer_question` (la risposta a una domanda di un worker, vedi "Domande dei
worker"). Senza registrazione i tool rispondono al modello «The board cannot be changed right now.» e la risposta
diretta «… le azioni sulla board non sono disponibili.».

**`open_plan_task { goal, bot }`.**

- Solo nel canale di un workspace; nel Generale l'errore chiede al modello di domandare all'utente il progetto.
- Il Bot è obbligatorio: il segretario lo sceglie dall'elenco dei Bot del suo system prompt (vedi "Bot nel
  contesto"). Prima di creare qualcosa `planning::open_plan` lo verifica fra i Bot-agente salvati
  (`engine/bot_choice.rs::check`): sconosciuto → «Unknown bot "…": use the id of one of the bots listed in your
  instructions.», disabilitato → «The bot "…" is disabled: …», senza `plan` fra i `kinds` → «The bot "…" cannot run
  plan tasks: choose one of the bots listed in your instructions whose kinds include plan, …». Nessun ripiego su un
  altro Bot.
- Titolo: la prima riga del goal, al massimo 72 caratteri (la board ne ammette 80), «Plan» se non resta nulla.
- Testo (inglese, lo legge il pianificatore, che non vede altro): la richiesta dell'utente **citata testualmente**
  (il messaggio del turno; in un risveglio da evento, l'ultimo messaggio dell'utente), il goal, il riassunto del thread
  e gli ultimi 10 messaggi prima del turno (autore e testo, 1000 caratteri ciascuno; uno più lungo finisce con
  `[… truncated: N characters in total; full text in the thread message <id>]`). La richiesta citata è sempre
  intera (un messaggio sta in 8000 caratteri): è il testo che ha generato la pianificazione, e tagliarla toglierebbe
  al pianificatore proprio ciò che deve pianificare.
- Allegati: se la richiesta citata o uno di quei messaggi ne ha, una sezione `## Attachments` (tra la richiesta e il
  goal) li elenca con **percorso assoluto**, nome, tipo e dimensione (`engine/planning/attachments.rs`, puro; il
  percorso lo ricava `attachments::layout::stored_file` dalla radice `hub_attachments/`, passata a `plan_order`
  con l'ambiente del workspace: `/mnt/<d>/...` in un workspace WSL, vedi "Allegati") e
  chiede al pianificatore di leggerli e di riportare i percorsi utili nelle specifiche dei task che propone, perché
  i worker vedono solo la specifica. Ogni allegato compare una volta; nelle righe dei messaggi recenti resta
  ` [attached: nome, ...]`. Mai il contenuto dei file. Senza allegati il testo è identico a prima; la sezione sta
  nei limiti di testo del task (al più 5 allegati per messaggio e 11 messaggi).
- Chi pianifica: esattamente il Bot scelto. `PlanOrder.bot` arriva a `automation::queue_hub_plan_task` e il task
  `plan` ne copia agente, modello, effort e skill alla creazione, come ogni task con Bot (snapshot: cambiare il Bot
  dopo non tocca il task; vedi [BOARD.MD](BOARD.MD), "Task di pianificazione"). Il Bot `general` lascia il modello
  predefinito dell'agente; l'app non sceglie più un modello per il pianificatore. Repository: la radice. Se la radice non è un repository git il pianificatore lavora nella cartella del workspace,
  senza worktree e in sola lettura, e propone un task per cartella (i repository git rilevati e le cartelle senza git,
  che girano in modalità libera; vedi [BOARD.MD](BOARD.MD), "Pianificazione in una cartella senza git" e
  "Cartelle senza git"); senza nessun repository non c'è più un errore.
- Il task si collega al thread (`link_tasks`) e il segretario ne diventa il **titolare** (`set_owner`). Creazione e
  collegamento avvengono **sotto il lock dei thread**: il thread degli eventi della board, che aspetta lo stesso lock,
  trova il task già collegato e ne scrive gli eventi («Task creato: …») in questo thread invece di aprirne uno. Il lock
  dura la creazione (qualche comando git), mai una chiamata al modello; l'automazione non prende mai il lock dei
  thread. Un thread con già 200 task non ne crea altri.
- Le domande del pianificatore (pianificazione conclusa senza proposta) le pone il titolare svegliato dall'evento;
  quando l'utente risponde, il segretario può aprire un **nuovo** `open_plan_task` con il goal originale, le domande
  e le risposte (la descrizione del tool lo chiede; anche il testo del piano porta i messaggi recenti con le
  domande). Durante la pianificazione il pianificatore può anche chiedere con `ask_user` (vedi "Domande dei worker").
- **Domande sul codice.** «@mario dove si imposta il tema?» non chiede modifiche: il Coordinatore apre comunque un
  `open_plan_task`, con il Bot che risponde alle domande (di serie `Quick Answer`, skill `code-answer`). Il task `plan`
  gira in sola lettura e chiude senza proposta con un riassunto che inizia con «Answer:» (esito "Answer" delle
  istruzioni del pianificatore, vedi [BOARD.MD](BOARD.MD), "Task di pianificazione"). L'evento nel thread è allora
  «Risposta: <titolo>» invece di «Pianificazione conclusa senza proposta: <titolo>», con la risposta intera
  (`events/texts.rs::plan_without_proposal`; il marcatore si riconosce in testa al report, spazi iniziali e maiuscole
  ignorati, `events/texts.rs::plan_answers`), e sveglia il titolare, che la riferisce all'utente a parole semplici
  senza aprire una nuova pianificazione (le indicazioni del titolare distinguono la risposta dalle domande). La
  notifica di sistema resta della categoria PlanWithoutProposal (`notify_plans` su Telegram) ma si intitola «Risposta
  pronta alla tua domanda».

**`decide_proposal { accept }`.** La decisione resta dell'utente e la garanzia è nel codice, non nel modello:

- non prende un id: il motore, all'inizio del turno, lega il turno alla proposta dell'evento a cui il messaggio
  dell'utente risponde (`planning::proposal_target`). Deve essere un turno svegliato da un **messaggio dell'utente**
  (`WakeCause::Message`, mai un evento o un elemento esterno)
  con `reply_to` su un messaggio `proposal_created` dello stesso thread e dello stesso workspace, la cui proposta è
  ancora in attesa nel thread (l'ultimo evento della proposta è `proposal_created`);
- all'esecuzione la proposta deve essere ancora pendente sulla board; poi la decide con la validazione della board;
- altrimenti (nessun `reply_to` sulla proposta, risveglio da evento o da elemento esterno, proposta già decisa) è rifiutato con un errore
  per il modello, che deve dire all'utente di usare «Rispondi» sul messaggio della proposta;
- il primo messaggio del turno lo ricorda al modello quando c'è una proposta da decidere, e la descrizione del tool
  gli chiede di domandare invece di decidere se l'intenzione non è chiara;
- il messaggio nel thread («Proposta accettata: N task in coda.» / «Proposta scartata.») lo scrivono gli eventi della
  board, come per i pulsanti; accettata, i task si collegano al thread (la proposta è sua). I pulsanti del pannello
  Task restano.

### Azioni sulla coda

`task_action` e `board_control` (`board_ops.rs`, puro e generico sulla board; effetti in `host/board_writes.rs`)
sono le prime azioni dei segretari sulla coda: solo reversibili (classe `Write`), quindi senza conferma, con
l'avviso di audit e il limite per turno di ogni modifica. Nessun segretario esistente le riceve: si scelgono, o si
crea il preset Operatore.

- **Solo su richiesta dell'utente** (`board_ops::require_user_request`): come `relay_answer`, solo in un turno
  svegliato da un messaggio dell'utente; un risveglio da evento della board o da elemento esterno è rifiutato, così
  un testo non fidato non può mai cambiare la coda.
- **Perimetro** (`tools::change_workspace`, puro): solo la board del workspace del canale del thread. Lì `workspace`
  si può omettere o deve essere quel workspace: un altro è rifiutato (`OutsideThread`, «You can change only the
  board of this thread's channel…»). Nel Generale serve `workspace`, che deve essere tra i canali del segretario
  (come `board_workspace`).
- **Verifica.** Prima di scrivere si legge la vista corrente della board (`HubFeed::current_views`): il task deve
  esserci (un id di un'altra board o inesistente dà «No task with id … on the board of workspace …»), e l'azione
  deve essere in `TaskView::allowed_actions` (`retry`, `to_top`) o, per `move_before`, entrambi i task devono avere
  `can_reorder` e essere diversi. Il rifiuto dice lo stato del task e la regola. `board_control` scrive solo ciò che
  cambia rispetto alla vista.
- **Scrittura.** Con le azioni registrate da `lib.rs` (`BoardActions::act_on_task`, `move_task`, `set_board`, cioè
  `automation::hub_task_action`, `hub_task_move`, `hub_board_set`): lo stesso codice e la stessa validazione dei
  comandi `task_action`, `task_move` e `board_set` del pannello Task, che quindi si aggiorna come per un clic. Fuori
  dal main thread e solo se l'epoca dell'HUB è ancora quella del lavoro. Un errore della board torna al modello
  («The board did not accept the change: …») e non lascia avvisi.

### Azioni che chiedono conferma

`request_task_action` e `request_task_update` (`engine/confirm.rs`, `confirm/resolve.rs`, puro e generico sui dati; effetti in
`host/requests.rs` e `action_requests.rs`) sono l'unico modo in cui un segretario arriva a fermare, eliminare, segnare
fatto o modificare un task. Il segretario non le esegue mai: mette nel thread una scheda e le esegue
solo il clic dell'utente su «Conferma».

- **Chi li ha.** Solo il preset Operatore, e solo se scelto (opt-in: nessun segretario esistente lo riceve, e i preset
  `seeded` non lo contengono). Classe `Destructive`: conta nel limite per turno come ogni modifica e non lascia un
  avviso di audit, perché la scheda è già visibile.
- **Quando si può chiamare** (`confirm::open_request`): solo in un turno svegliato da un messaggio dell'utente
  (`board_ops::require_user_request`, come `task_action`); solo in un thread di un workspace; solo per un task
  **collegato al thread** (`task_ids`) e presente sulla board; l'azione deve essere in `allowed_actions` del task
  (e una `delete_cascade` non può toccare task in corso); **una sola richiesta aperta per task**. Gli errori sono per il
  modello, in inglese.
- **La scheda.** Un messaggio di sistema con `event.kind = action_request` e `event.request` (`HubActionRequest`):
  `id` (lo stesso del messaggio), `action`, `reason`, `secretary`, `task_title`, `task_status` (lo stato del task alla
  richiesta), `dependents` (titoli dei task che una cascata toglierebbe), `expires_at`, `state`, `resolved_at`, `note`.
  Lo `state` (`pending`, `confirmed`, `cancelled`, `expired`, `stale`, `failed`) si aggiorna **sul posto**
  (`ChannelThreads::update_request`).
- **Scadenza.** 30 minuti (`REQUEST_TTL_MS`). Il frontend mostra come scaduta, senza pulsanti, una richiesta in attesa
  con `now >= expires_at` (`displayState`); il backend la segna `expired` al primo clic di conferma dopo la scadenza.
  «Annulla» vale anche dopo la scadenza, se non è ancora stata decisa.
- **Invalidazione** (`confirm::invalidate_for_task`, chiamata da `events/route.rs` a ogni evento del task): se il task
  cambia stato o sparisce, le sue richieste in attesa passano a `stale` subito, con una nota, senza aspettare il clic.
- **Conferma** (`hub_resolve_action_request`, `confirm::decide`). Sotto il lock dei thread, così due clic non
  eseguono due volte: si rilegge il task sulla board; se la richiesta non è più in attesa è un errore; con
  `confirm = false` è `cancelled`; se è scaduta `expired`; se il task manca, ha cambiato stato o non consente più
  l'azione `stale`; altrimenti si esegue l'azione con la stessa funzione del comando `task_action` del pannello Task. Se la
  board la rifiuta lo stato è `failed` e la nota riporta l'errore. Scaduta, non più valida o fallita non sono errori
  del comando: la richiesta torna aggiornata.
- **Esito e risveglio.** Una decisione scrive una riga di sistema `event.kind = action_request_resolved` (con la
  richiesta nello stato finale e il testo in italiano di `confirm/texts.rs`), in risposta alla scheda. Quella riga
  sveglia **il segretario che ha fatto la richiesta**, anche se non è il titolare del thread, che così dice
  all'utente come è andata (il preset gli chiede di non dire «fatto» prima). Poi `hub-update`.
- **Modificare un task** (`request_task_update`, `confirm/update.rs`). Stessa scheda, stessi stati, scadenza,
  invalidazione e risveglio, con `action = update` e `HubActionRequest.update` (`HubTaskUpdate`): `changes` è il diff
  per la scheda, un elemento per campo che cambia davvero (`title`, `task`, `depends_on`, `model`, `bot`; `before` e
  `after`, `null` = nessun valore, testi tagliati a 600 caratteri; per il Bot gli id di prima e di dopo, etichetta
  «Bot», «nessuno» per un task senza Bot), mentre `patch` (i valori nuovi **completi**) e `base` (gli
  stessi campi com'erano alla richiesta) restano nel dato salvato e non arrivano ai bindings (`#[ts(skip)]`). Altri
  campi del task (repository, agente, tipo, E2E) non si possono chiedere. Con il Bot cambiano anche agente, modello,
  effort e skill del task (la copia la rifà `Board::update_task` alla conferma); la scheda mostra solo il Bot.
  - *Alla richiesta*: il task deve essere collegato al thread e `can_edit` (`queued` o `failed`: un task in corso o
    concluso è rifiutato), il patch deve cambiare qualcosa, un nuovo Bot deve esistere ed essere abilitato
    (`bot_choice::check`, lo stesso errore di `open_plan_task`), e la modifica si prova sulla board con
    `BoardActions::check_task_update` (`automation::hub_task_update_check`: `Board::update_task` su una copia, con la
    stessa validazione del pannello Task, cioè grafo senza cicli, dipendenze esistenti, modello fra quelli dell'agente
    del task). Un errore torna al modello e non scrive nessuna scheda.
  - *Alla conferma*: `decide` rifiuta (`stale`) se il task ha cambiato stato, non è più modificabile, o se uno dei campi
    della richiesta non ha più il valore di `base` (l'utente l'ha cambiato nel frattempo). Altrimenti
    `BoardActions::update_task` (`automation::hub_task_update`, la stessa funzione del comando `task_update`) applica
    il `patch` **completo**, rifacendo tutta la validazione; se la board lo rifiuta lo stato è `failed` con l'errore.
  - *Scheda*: `HubActionRequestCard` mostra per ogni campo il valore di prima (barrato) e quello di dopo; il pulsante
    «Conferma» non è rosso, perché non è una distruzione.
- **Frontend.** `HubActionRequestCard` (nella conversazione di entrambe le viste) mostra chi chiede, azione (le
  etichette del Tasks Manager), task, motivo, `dependents` per una cascata, stato e `note`; finché la richiesta aspetta
  e non è scaduta ci sono «Conferma» e «Annulla». La logica pura è in `src/lib/hubActionRequests.ts`, la rilettura del
  thread dopo il clic in `resolveActionRequest` (`app/actions/hub.ts`).

### Domande dei worker

Il flusso della tappa 4: un worker (o il pianificatore) con un dubbio chiama `ask_user { question, options? }`
(vedi [BOARD.MD](BOARD.MD)) e chiude il turno → la domanda compare come evento `task_question` nel thread del suo
task, che aspetta l'utente → l'utente usa «Rispondi» sull'evento e scrive la risposta → l'app la scrive nel
terminale del worker, che riprende → l'evento `task_question_answered` chiude l'attesa. Codice in `hub/questions.rs`
(puro: quali domande aspettano in un thread), `hub/direct_answer.rs`, `engine/relay.rs` e `hub/actions.rs`.

**Risposta diretta.** Decisione dell'utente: rispondere a una domanda **non passa mai dal modello**. È
deterministico, gratuito e funziona anche nei thread senza segretario titolare.

- `hub_send` salva il messaggio; se ha `reply_to` su un evento `task_question` (`questions::replied_question`) il
  messaggio è la risposta e non sveglia nessun segretario. Il thread si legge (copia, lock rilasciato) e il testo
  del messaggio, **così com'è**, va all'azione `answer_question` registrata da `lib.rs`
  (`automation::answer_question`: segna la domanda sulla board e la digita nel terminale del worker solo se è fermo
  in attesa; nessun lock dell'HUB è in mano durante la consegna).
- Se il messaggio ha allegati, al testo si aggiunge (dopo una riga vuota) «Attached files (read them at these absolute
  paths):» e una riga per allegato con percorso assoluto, nome, tipo e dimensione (`direct_answer::answer_reply`,
  che riceve la radice degli allegati e l'ambiente del workspace del canale, cioè del worker: `/mnt/<d>/...` in
  WSL); un messaggio con soli allegati consegna solo quell'elenco. Senza allegati
  il testo resta quello scritto. Il limite di 4000 caratteri della board vale sul testo completo.
- La domanda deve essere ancora in attesa **nel thread**: per il suo task l'ultimo evento è proprio quella domanda,
  del workspace del canale. Una domanda già risposta, o di un task concluso, fallito o ripartito, dà «Risposta non
  inviata: questa domanda ha già una risposta o il suo task non è più in attesa.» senza chiamare la board.
- Esito, come messaggio di sistema in risposta al messaggio dell'utente: «Risposta inviata al worker.» oppure
  «Risposta non inviata: <motivo della board> Se serve, apri il terminale del worker dal Tasks Manager e rispondi
  lì.» (worker che sta ancora lavorando, permesso in attesa, tab chiuso, risposta vuota o oltre 4000 caratteri...).
  Il link «apri il terminale del worker» è già sulla riga dell'evento finché il tab è aperto.

**Nell'interfaccia.** L'evento `task_question` è una scheda (`HubQuestionCard`), non una riga: bordo e icona
«attenzione», «Domanda dal worker» o «Domanda dal pianificatore», id del task, ora e badge «Aspetta te» finché
aspetta. Le opzioni sono pulsanti che rispondono subito (`answerQuestion`: `hub_send` con `reply_to` e rilettura
del thread, perché la consegna è sincrona e l'esito è già scritto); «Rispondi» apre il compositore sulla domanda per
una risposta libera; «apri il terminale del worker» compare se il tab è ancora aperto. Lo stato lo ricava
`lib/hubQuestions.ts` dal thread, come il backend: aspetta finché la sua è l'ultima voce del task; «Risposta
inviata» (con il testo) dopo un esito «Risposta inviata al worker.», o senza testo se la risposta l'ha vista solo la
board (terminale, segretario); «Non più in attesa» se il task è andato avanti. L'esito «Risposta non inviata…» è un
avviso rosso «Risposta non consegnata» col link al terminale; la domanda resta in attesa. Il dettaglio del task nel
Tasks Manager mostra la domanda con il pulsante «Rispondi nell'Agents HUB» (solo a HUB acceso): il backend non
espone il thread di un task, quindi `openTaskThread` legge al massimo 10 thread del canale (prima quelli in attesa)
e apre quello con la domanda. Un task con una domanda pendente è «In attesa di risposta» (stato `question` di
`displayStatus`, blu, distinto da «Richiede attenzione»).

**Il titolare.** La domanda sveglia il titolare del thread, se c'è (vedi "Risvegli da evento"): la presenta in
breve e può aiutare a capirla, ma un risveglio da evento non può rispondere.

**`relay_answer { body, question_id?, task_id? }`.** Per quando l'utente chiede al segretario di rispondere al posto suo («@mario digli di
usare la seconda opzione»). Le garanzie sono nel codice:

- all'inizio del turno il motore ricava le domande in attesa del thread (`relay::relay_candidates`), e solo in un turno
  svegliato da un **messaggio dell'utente**; il tool ne sceglie una (`relay::choose`). Senza id vale l'**unica**
  domanda in attesa. Con più domande il modello passa `question_id` e `task_id`, che legge da `thread_read` (
  `event.question.id` e `event.task_id`) o da `task_details` (`question.id`): l'id di una domanda (`q-<tentativo>-<n>`) è
  unico solo nel suo task, quindi serve anche `task_id` quando due task hanno lo stesso id (con il solo `question_id`
  ambiguo: errore che chiede `task_id`);
- un id che non è fra le domande in attesa del thread è rifiutato (errore per il modello), e un risveglio da evento
  o da elemento esterno rifiuta **anche con gli id**: la scelta avviene dopo la regola del risveglio. Nessuna domanda
  in attesa: errore per il modello. Più di una domanda e nessun id: errore che chiede al modello di domandare
  all'utente quale, o di passare gli id se l'utente l'ha detto, e di dirgli che può usare «Rispondi» sull'evento di
  quella domanda (che diventa una risposta diretta). Il segretario non risponde mai di sua iniziativa;
- consegnata, il thread mostra cosa è stato inviato (l'avviso di audit, vedi "Classi dei tool, audit e limite"): «@mario ha inviato al worker questa risposta: …»; un errore
  della board torna al modello;
- come ogni tool nuovo, non si aggiunge da solo ai segretari esistenti: lo sceglie l'utente nelle impostazioni.

### Client del modello

`OpenAiModel` (`openai.rs`) usa `async-openai` 0.43 (MIT; feature `chat-completion` e `rustls-no-provider`) verso
`provider.base_url` + `provider.model`, con la API key letta dal portachiavi a ogni turno (`Authorization: Bearer`).
reqwest è la 0.13 già presente per l'updater, con timeout di 120 s per chiamata e 10 s per la connessione; il
provider crittografico di rustls è `ring`, come per l'updater (installato se manca), così non si compila
`aws-lc`. `OpenAIConfig` legge `OPENAI_*` dall'ambiente: ogni campo si sovrascrive, quindi nessuna variabile
dell'ambiente (per esempio una chiave OpenAI dell'utente) arriva all'endpoint scelto. Niente streaming.

Gli errori si classificano (`ModelError`) e diventano un messaggio di sistema in italiano: non raggiungibile,
tempo scaduto, HTTP 401/403 (API key), 404 (indirizzo o modello), 429, altri codici, risposta non valida o vuota.
Nel log va solo la classe dell'errore (e per gli errori di rete il messaggio di reqwest), mai il corpo della
risposta: alcuni provider ripetono parte della chiave in un 401. La feature `byot` (parametri extra di llama.cpp
come grammatica o `json_schema`) si aggiungerà quando il motore vincolerà l'output.

### Stato verso il frontend

`HubSecretaryStatus`: `channel`, `thread_id`, `secretary`, `expires_at` (millisecondi Unix: quando la sessione si
chiude se non succede nulla, con l'`idle_minutes` corrente) e `working` (sta lavorando sul thread o ha un turno in
coda: "sta scrivendo"). L'evento `hub-secretary-status` porta l'elenco completo, ordinato per canale, thread e nome,
a ogni cambiamento (risveglio, inizio e fine di un lavoro, scadenza, spegnimento); `hub_secretary_status` lo legge
all'avvio della vista. I messaggi dei segretari passano dal normale `hub-update`.
