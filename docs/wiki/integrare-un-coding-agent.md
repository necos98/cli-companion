# Integrare un nuovo coding agent

> **cli-companion non sa come funziona un coding agent. Offre dei contratti;
> l'integrazione implementa quelli che le servono, ed è il contratto
> implementato a far funzionare l'agente.**

Questa guida spiega come aggiungere un agente (Claude Code è la prima integrazione
di riferimento, OpenCode la seconda, descritta in fondo con ciò che è stato provato).
Vale una regola sola:
**il core non nomina mai un agente specifico**. Tutto ciò che è proprio di un
agente (comandi, flag, formato degli hook, cartelle di configurazione, sintassi
dei permessi) vive in `src-tauri/src/agents/<id>/`. Un test architetturale
(`src-tauri/src/agents/architecture.rs` e `tests/architecture.test.mjs`) fallisce se fuori da quella cartella compare un
riferimento a un agente.

Indice: [Il modello](#il-modello) · [I contratti](#i-contratti) ·
[Procedura](#procedura-passo-passo) · [Checklist](#checklist-finale) ·
[Integrazione di riferimento n. 2: OpenCode](#integrazione-di-riferimento-n-2-opencode)

## Il modello

Un'integrazione è una struct che implementa il trait `AgentIntegration`
(`agents/contracts/integration.rs`).

**Nucleo obbligatorio** (senza non esiste l'agente):

| Metodo | Scopo |
| --- | --- |
| `id()` | Id stabile, `[a-z0-9-]` senza trattini ai bordi. Compare in `--notify`, nelle cartelle del pacchetto, nell'API e nel campo `agent` dei task. Non si cambia più. |
| `display_name()` | Nome mostrato nell'interfaccia. |
| `detect(&Target)` | Rileva l'agente in un target (Windows o una distro WSL) e ritorna `Detection { found, version, path }`. Può lanciare processi: il core lo chiama fuori dal main thread, con timeout (`detection::DETECT_TIMEOUT`). |
| `interactive_command()` | Riga digitata nella shell del tab per avviare l'agente. Deve valere in PowerShell, cmd e bash (niente `$`, backtick, `%`, `!`). È anche la base dell'avvio integrato `builtin:<id>`. |

**Capabilities** (tutto il resto): `capabilities()` ritorna una struct
`Capabilities` con un campo `Option<&dyn Trait>` per ogni contratto
(`agents/contracts/capabilities.rs`). `Capabilities` **non ha `Default`**: ogni
integrazione elenca tutti i campi, `Some(self)` o `None`. Quando si aggiunge un
contratto al core, ogni integrazione smette di compilare finché non decide, ed è
voluto: non esistono implementazioni di default "non supportato" che nascondano
una lacuna. Il test di conformità controlla che `declared()` coincida con i campi
presenti.

Regola per chi integra: **dichiara solo ciò che sai fare davvero**. Un contratto
`None` è una scelta legittima e il core la gestisce (vedi la tabella di ogni
contratto); un contratto dichiarato ma fatto male è un difetto.

| Contratto | Campo | Trait | File del contratto |
| --- | --- | --- | --- |
| Pacchetto | `package` | `IntegrationPackage` | `contracts/package.rs` |
| Eventi degli hook | `hook_events` | `HookEvents` | `contracts/hook_events.rs` |
| Ripresa della sessione | `session_resume` | `SessionResume` | `contracts/session_resume.rs` |
| Automazione dei worker | `automation` | `WorkerAutomation` | `contracts/automation.rs` |
| Permessi dei worker | `worker_permissions` | `WorkerPermissions` | `contracts/worker_permissions.rs` |
| Skill del plugin | `plugin_skills` | `PluginSkills` | `contracts/plugin_skills.rs` |
| Skill utente | `user_skills` | `UserSkills` | `contracts/user_skills.rs` |
| Limiti di utilizzo | `usage_limits` | `UsageLimits` | `contracts/usage_limits.rs` |
| Consumi della sessione | `session_usage` | `SessionUsageReader` | `contracts/session_usage.rs` |
| Profilo di input | `input_profile` | `InputProfileSource` | `contracts/input_profile.rs` |
| Modelli | `models` | `ModelCatalog` | `contracts/models.rs` |
| Effort | `effort` | `EffortLevels` | `contracts/effort.rs` |
| Avvio interattivo dei Bot | `interactive_launch` | `InteractiveLauncher` | `contracts/interactive_launch.rs` |

Il frontend non conosce i contratti per nome di agente: riceve i dati
dichiarativi (`agent_profiles`: contratti dichiarati, regole di input, dati di
ripresa, avviso sui limiti, testi dei permessi) e agisce su quelli.

### Registrata, abilitata, scelta, pronta

Aggiungere l'integrazione al registro (`INTEGRATIONS`) dice solo che **esiste**. Quello che l'utente
vede e usa dipende da quattro passi, tutti nel core: nessuno è codice dell'integrazione.

1. **Abilitata.** L'utente abilita le integrazioni da Impostazioni → Generale → Integrazioni
   (`integrations.json`, vedi `agents/enablement.rs`). Una integrazione appena registrata **non** si
   abilita da sola per chi ha già il file; al primo avvio (file assente) sono abilitate quelle con un
   pacchetto già installato, e se non ce n'è nessuna la prima del registro. Il predefinito è sempre fra
   le abilitate. Una disabilitata non compare in avvii, form dei task, scelte dei worker né in
   `get_workspace_settings.agents`.
2. **Scelta.** `registry::resolve_agent(explicit, workspace_agent, uso)` sceglie l'agente di ogni
   uso: prima la scelta esplicita (l'agente di un task o di un Bot), poi `Workspace.agent`, poi
   il predefinito globale. L'uso decide il contratto che l'integrazione deve dichiarare
   (`AgentUse::required`): oggi esiste solo `Worker` → `automation`. Una scelta sconosciuta,
   disabilitata o senza il contratto è un errore (`AgentUnavailable`: `unknown agent: <id>`,
   `<nome> is disabled: enable it in the integrations settings`, `<nome> does not support automated
   sessions`), **mai un ripiego** su un altro agente. Nel core non scrivere mai un
   `if agent == ...` per scegliere: passa dal risolutore.
3. **Pronta.** Per sapere se si può avviare qualcosa in un ambiente il core usa `Readiness`
   (`agents/readiness.rs`), nell'ordine: disabilitata, agente non trovato (`detect`), pacchetto non
   installato (`package.is_installed`), pacchetto di un'altra versione (`package.version` diverso da quello
   installato), pronta. Senza il contratto `package` conta solo il rilevamento. Il sondaggio lancia
   processi: è cache di 30 s e si azzera a ogni installazione o rimozione. Conseguenze per chi
   integra: `detect` deve essere affidabile e a tempo limitato, e `version` deve essere stabile e deve
   cambiare **solo** quando cambia ciò che si genera; una `version` instabile lascia l'integrazione
   per sempre “Da aggiornare” e i suoi task in attesa.
4. **Contratti dichiarati.** Cosa compare dipende da `capabilities()`:

| Cosa vede l'utente | Contratto che serve | Senza il contratto |
| --- | --- | --- |
| Scheda nelle Integrazioni, avvio `builtin:<id>` (“+ Terminale”) | solo il nucleo | — |
| Installa / Aggiorna / Rimuovi, stato “Pronto” | `package` | nessuna azione; pronta appena rilevata |
| Notifiche di fine lavoro, ripresa dei tab | `hook_events`, `session_resume` (con `package` per gli hook) | niente notifiche / ripresa |
| Skill del plugin nei Bot (un Bot con una skill del plugin) | `plugin_skills` (richiede `package`) | il Bot non può usare quell'agente: `does not ship the plugin skills` |
| Agente dei worker e dei task, campo **Agente** dei form | `automation` | non selezionabile; come agente del workspace o del task: `does not support automated sessions` |
| Elenco `agents` di `get_workspace_settings` (modelli, `rule_format`, prontezza) per l'Intake e Project Setup | nessuno: ci sono tutte le abilitate | — |
| Regole allow/deny dei worker per l'agente | `worker_permissions` | le regole sono rifiutate; `rule_format` è `null` |
| Suggerimenti di modello (form dei task, impostazioni dei worker, tool MCP), validazione del nome | `models` | nessun suggerimento; il nome vale solo come forma sicura |
| Voci “Bot” del menu di avvio (`bot:<id>`) e relativo avvio | `interactive_launch` | la voce c'è ma è bloccata: `<nome> non supporta l'avvio dei Bot.` |
| Limiti di utilizzo, token per sessione, tasti propri | `usage_limits`, `session_usage`, `input_profile` | assenti |

L'ordine delle voci nel registro è l'ordine in cui si mostrano le integrazioni e quello da cui si parte la
prima volta; non decide più l'agente predefinito, che sceglie l'utente.

### I due canali tra agente e app

Due meccanismi dell'app, indipendenti dall'agente, sono ciò che l'integrazione
deve collegare all'agente:

- **Canale degli hook** (`src-tauri/src/ipc/`). L'agente esegue
  `<exe> --notify <evento> <agent_id>` ogni volta che succede qualcosa. Eventi
  ammessi: `session-start`, `finished`, `session-end`, `needs-input`,
  `permission-request` (`HookEventKind`). L'exe legge le variabili
  `CLI_COMPANION_SESSION` (tab di appartenenza; senza, l'evento è ignorato con
  exit 0), `CLI_COMPANION_TOKEN` (il segreto di quella sessione, non un token globale dell'app) e `CLI_COMPANION_PORT`, che l'app imposta nel
  terminale (su WSL anche in `WSLENV`). Lo stdin dell'hook, se presente, passa da
  `HookEvents::normalize`. L'unico output ammesso su stdout è la decisione di
  blocco del fine turno. Dettagli in `docs/DESIGN.MD`, sezione `ipc/`.
- **Server MCP** (`src-tauri/src/mcp/`). L'agente avvia `<exe> --mcp` come server
  MCP stdio chiamato `cli-companion`. Eredita le stesse variabili d'ambiente del
  terminale: fuori da cli-companion espone zero tool. I tool (`get_task`,
  `complete_task`, `propose_tasks`, ...) dipendono dal ruolo della sessione
  (worker, revisore, sessione normale): vedi `docs/BOARD.MD`.

Un agente che non sa eseguire un comando a un evento, o non sa caricare un server
MCP, non può dichiarare `package` in modo utile: la maggior parte dei contratti
sotto dipende dal pacchetto.

## I contratti

Per ognuno: scopo, firma, cosa fa il core quando c'è e quando manca, quali test
di conformità (`agents/conformance.rs`) lo riguardano. I test girano su ogni
integrazione registrata e su una `FakeIntegration` (`agents/testing.rs`), e la
suite stessa è provata contro integrazioni volutamente rotte (`Flaw`).

### 1. Pacchetto: `IntegrationPackage`

**Scopo.** I file che l'app genera e installa *dentro* l'agente perché
l'agente parli con l'app: gli hook che chiamano `--notify`, la dichiarazione del
server MCP `cli-companion` (`<exe> --mcp`) e le skill dei preset.

**Firma** (`contracts/package.rs`).

```rust
fn version(&self, ctx: &InstallCtx, target: &Target) -> Option<String>;
fn files(&self, ctx: &InstallCtx, target: &Target) -> Result<Vec<PackageFile>, String>;
fn is_installed(&self, target: &Target, det: &Detection) -> bool;
fn install(&self, ctx: &InstallCtx, target: &Target, det: &Detection) -> Result<String, String>;
fn uninstall(&self, ctx: &InstallCtx, target: &Target, det: &Detection) -> Result<String, String>;
```

- `InstallCtx { exe, data_dir }` porta l'exe corrente (il comando degli hook) e la
  cartella dati; `exe_for(target)` lo traduce per WSL (`/mnt/c/...`),
  `plugin_root(agent_id, target)` dà la cartella del pacchetto generato:
  `<dati app>/agent-plugins/<id>/<target>/`.
- `version` e `files` sono **pure**: nessun processo, nessun disco. `version`
  deve cambiare quando cambiano gli hook, la dichiarazione MCP o le skill (per
  Claude Code è `<release>-h<hash>`): il core la confronta con quella installata
  per segnalare i pacchetti da aggiornare. `None` = non calcolabile, mai da
  aggiornare. L'hash si calcola con `fnv32` (`contracts/package.rs`, esportata da
  `agents`), la stessa funzione per tutte le integrazioni: non scriverne un'altra.
- `files` ritorna path relativi, con slash dritti, senza `..`, senza duplicati e
  senza BOM.
- `install`/`uninstall` ritornano il log dei passi. Il core li serializza per
  coppia (agente, target) con `with_target_lock`.

**Core con il contratto.** Interfaccia di installazione/rimozione/aggiornamento
(comandi `install_agent_plugin`, `remove_agent_plugin`, `agent_plugins_update`),
stato "installato" per ogni ambiente, avviso "da aggiornare".

**Core senza il contratto.** L'agente compare nell'elenco, ma `installed` è sempre
`false` e l'installazione risponde "non ha un pacchetto da installare". Senza
pacchetto non arrivano hook né MCP: niente notifiche di fine lavoro, niente
worker, niente preset.

**Conformità** (`package`): `version` non vuota, senza spazi e stabile su due
target; `files` riuscito per Windows e WSL; path relativi, con slash dritti, senza
`..`, senza duplicati, senza BOM.

### 2. Eventi degli hook: `HookEvents`

**Scopo.** Normalizzare lo stdin grezzo che l'agente passa all'hook in un
`HookEvent` neutro, e dire come si fa proseguire un turno che l'app ha bloccato.

**Firma** (`contracts/hook_events.rs`).

```rust
fn normalize(&self, kind: HookEventKind, raw: &str) -> HookEvent;
fn stop_block_output(&self, reason: &str) -> String;
```

- `HookEvent` ha solo campi opzionali: `agent_session_id`, `wait`,
  `message`, `already_continuing`, `background_work`, `start_source`, `end_reason`,
  `transcript_path`, `tool_name`, `tool_summary`. Si riempie ciò che l'agente
  fornisce. I nomi dei campi dell'agente (`session_id`, ...) restano nel file
  dell'integrazione.
- `wait` dice cosa aspetta l'agente in un `needs-input`, con un tipo neutro
  `WaitKind`: `Permission` (un dialogo di permesso), `Idle` (turno finito, la
  sessione aspetta da tempo un messaggio: non è ferma su un dialogo), `Question`
  (una domanda all'utente), `Other` (attesa non classificata). È l'integrazione a
  tradurre i propri tipi di notifica: Claude Code mappa `permission_prompt`,
  `idle_prompt` ed `elicitation_dialog`, e un tipo sconosciuto in `Other`. Il core
  non vede mai il valore grezzo: un worker in attesa `Idle` resta `running`, ogni
  altra attesa lo porta a `needs_input`, e un `needs-input` di attesa
  `Permission` che segue di poco un `permission-request` dello stesso tab è un
  duplicato e si scarta. Sul filo `notification_type` porta il nome del tipo
  neutro (`permission`, `idle`, `question`, `other`), scelto dal protocollo.
- `normalize` è tollerante: input vuoto o malformato dà un evento senza dettagli,
  mai un errore.
- `stop_block_output` è la riga scritta su stdout dall'hook di fine turno quando
  l'app risponde `{"block": "<motivo>"}` (un worker che chiude il turno senza
  aver chiamato `complete_task`): deve far proseguire l'agente. È l'unico
  output che `--notify` produce. Il formato lo decide l'integrazione: per
  Claude Code lo legge l'agente stesso, per OpenCode un plugin nostro (vedi
  l'ultima sezione).

**Core con il contratto.** `--notify` legge lo stdin (tetto 1 MB) e lo passa a
`normalize`; l'app usa i dettagli per l'id della sessione dell'agente (ripresa),
il transcript (consumi), il testo dei permessi richiesti e per distinguere un
fine turno vero da uno bloccato o con lavoro in background.

**Core senza il contratto.** L'evento parte lo stesso, senza dettagli, e non c'è
mai output di blocco: le notifiche funzionano, ma niente id di sessione (la
ripresa non ha cosa riprendere), niente consumi e un worker che chiude il turno
senza consegnare non viene fatto proseguire.

**Conformità** (`hook_events`): per ogni `HookEventKind` gli input `""`,
`"not json"`, `"[]"`, `"{}"` danno `HookEvent::default()`; `stop_block_output("x")`
non è vuoto e contiene il motivo.

### 3. Ripresa della sessione: `SessionResume`

**Scopo.** Dire al frontend come riprendere la conversazione di un tab dopo il
riavvio dell'app. È solo dati: il frontend non sa nulla del modo in cui un agente
riprende una sessione.

**Firma** (`contracts/session_resume.rs`).

```rust
fn resume_spec(&self) -> ResumeSpec;
// ResumeSpec { command_names, strip_flags: Vec<ResumeFlag>, resume_template }
```

- `command_names`: nomi del programma (minuscolo, senza spazi); il primo è il
  canonico, usato quando il tab non sta eseguendo l'agente. Servono al frontend
  per riconoscere di quale agente è un tab dal comando di avvio.
- `strip_flags`: flag di ripresa/continuazione da togliere dal comando originale
  (`names` con il trattino, `takes_value`).
- `resume_template`: modello con i segnaposto `{command}` e `{id}`, ciascuno
  **una volta**.

L'id che sostituisce `{id}` è quello riportato dall'hook `session-start`
(`HookEvent::agent_session_id`): senza `hook_events` non c'è.

**Core con il contratto.** Il frontend salva l'id di sessione dell'agente e alla
riapertura rilancia il comando di ripresa.

**Core senza il contratto.** Il tab si riapre con il comando originale (o una
shell vuota): nessuna ripresa.

**Conformità** (`session_resume`): `command_names` non vuoto e minuscolo; il
programma del comando interattivo è tra i `command_names`; nomi dei flag che
iniziano con `-`; ogni segnaposto esattamente una volta; il comando risultante è
sicuro per la shell.

### 4. Automazione dei worker: `WorkerAutomation`

**Scopo.** Costruire il comando che avvia una sessione automatica (un worker del
Tasks Manager) senza utente.

**Firma** (`contracts/automation.rs`).

```rust
fn command(&self, env_kind: TargetKind, req: &AutomationRequest) -> Option<AutomationLaunch>;
// AutomationRequest { bootstrap_prompt, permission_mode, model, permissions_file, effort }
// AutomationLaunch  { command: String, env: Vec<(String, String)> }
```

- `bootstrap_prompt`: ASCII sicuro, garantito dal chiamante; chiede all'agente di
  chiamare `get_task`.
- `permission_mode`: `Plan` | `Default` | `AcceptEdits` | `Bypass`
  (`PermissionMode::ALL`, dal più restrittivo). L'integrazione mappa ciascun
  livello su ciò che l'agente offre.
- `model`: già validato (forma generica `validate::valid_model_shape` e `ModelCatalog::is_valid` dell'agente), ma l'integrazione **rivalida**.
- `effort`: `Option<Effort>`, vedi il contratto 12. L'integrazione lo riceve solo se lo dichiara e lo ammette; con `None` il comando resta identico.
- `permissions_file`: file di regole preparato dal contratto `WorkerPermissions`
  *della stessa integrazione*, già nel formato di path del target.
- Il risultato è un `AutomationLaunch` con due parti. `command` è digitato nella
  shell del tab: valido in PowerShell, cmd e bash. `None` = richiesta non
  rappresentabile in modo sicuro. Tutto ciò che non si può mettere letterale tra
  doppi apici si rifiuta (`is_shell_safe_path` per i path).
- `env` sono le variabili d'ambiente che il **core** imposta sul processo del tab
  *prima* che la shell parta (le prepara il backend per quel tab, `pty/staged_env.rs`,
  e `pty_spawn` le consuma una volta sola; il frontend non può impostarne). Per un
  agente che si configura con l'ambiente (modello, regole di permesso, file di
  configurazione) è il canale giusto: **niente sintassi di shell**, quindi vale
  uguale in PowerShell, cmd e bash, e i valori sono letterali (JSON compreso, senza
  escape). Su WSL attraversano il confine con `WSLENV`, con il flag `/u` (da
  Windows a WSL, valore invariato): il valore deve quindi essere **già nel formato
  di path del target** (un path del file di regole su WSL è `/mnt/c/...`, come per
  `permissions_file`). Le variabili arrivano a tutti i processi del tab, non solo
  all'agente.
- Validatori (`contracts/automation.rs`): il **nome** è `[A-Z_][A-Z0-9_]{0,63}` e
  non può essere riservato (`CLI_COMPANION_*`, `WSLENV`, `PATH`, `TERM`,
  `COLORTERM`); il **valore** è al massimo 32 KiB e senza caratteri di controllo
  (NUL, a capo, tab); nessun nome ripetuto. Il registro (`automation_command`) rifiuta
  un'integrazione che ne produce di non valide con l'errore inglese
  `<nome> returned an invalid environment variable for automated sessions: <NOME>`,
  senza ripiegare su altro. `first_invalid_env_name` applica le tre regole insieme.
- Il comando deve far partire l'agente **con il server MCP `cli-companion`
  utilizzabile senza conferme** (per Claude Code una regola di permesso sul
  server): un worker che non può chiamare `get_task` non fa nulla.

**Core con il contratto.** `automation_command(agent_id, env_kind, req)` è usato
dallo scheduler per avviare i worker con l'agente **del task**, e l'agente compare
come scelta per il workspace (Impostazioni → Worker) e per un task (campo **Agente**
dei form, tool MCP `propose_tasks`/`update_task`). Un task il cui agente non è
`Ready` nell'ambiente del workspace resta in coda finché lo diventa (vedi
“Registrata, abilitata, scelta, pronta”).

**Core senza il contratto.** `"<nome> does not support automated sessions"`: non si può
scegliere come agente di un task né del workspace, e i task non partono con quell'agente.
`registry::default_agent()` (il predefinito: la prima del registro finché l'utente non
sceglie) **deve** dichiarare `automation`: lo verifica la conformità. Se l'utente sceglie
come predefinito un agente senza `automation`, creare un task senza un agente esplicito (e
senza un agente del workspace) fallisce con quell'errore invece di ripiegare su un altro agente.

**Conformità** (`automation`): per ogni target e ogni `PermissionMode` esiste un
avvio il cui `command` contiene il prompt ed è sicuro per la shell, e le cui `env`
hanno nomi e valori validi e nessun nome duplicato (il messaggio dice target e
modalità); prompt con `" $ \` % ! \\` o a capo, o vuoto, rifiutati; modello valido
(il primo consigliato dal catalogo) accettato, modelli non validi (vuoto, spazi,
`;`, `"`, 65 caratteri) rifiutati; `permissions_file` con caratteri pericolosi
rifiutato, uno sicuro usato nel comando **oppure in una variabile d'ambiente**.

### 5. Permessi dei worker: `WorkerPermissions`

**Scopo.** Le regole allow/deny del workspace, nel formato che l'agente sa
leggere. Il core non conosce la sintassi delle regole: le valida, le scrive e le
descrive solo tramite questo contratto.

**Firma** (`contracts/worker_permissions.rs`).

```rust
fn rule_format(&self) -> RuleFormat;        // max_rules, max_rule_chars, example (inglese)
fn guide(&self) -> PermissionsGuide;        // testi dell'interfaccia (italiano)
fn validate_rules(&self, rules: &[String]) -> Result<Vec<String>, ValidationError>;
fn prepare(&self, data_dir: &Path, workspace_id: &str, env_kind: TargetKind,
           allow: &[String], deny: &[String]) -> Result<Option<String>, String>;
```

- `validate_rules`: trim, righe vuote e doppioni scartati, `Err` tipizzato
  (`ValidationError`) per l'interfaccia e per il modello.
- `prepare`: scrive (solo se cambia) un file nella cartella dati e ritorna il path
  per `AutomationRequest::permissions_file`; `None` se non ci sono regole; `Err`
  se il risultato non è usabile in modo sicuro in una riga di comando.
- `guide`: sintassi, avvertenza del bypass, segnaposto dei campi. Sono testi
  per l'utente, quindi in italiano; `RuleFormat::example` lo legge il modello
  (schema MCP): inglese.

**Core con il contratto.** `validate_worker_rules`, `worker_rule_format`,
`worker_permissions_file` (modulo `worker_rules.rs`) lavorano con l'integrazione
predefinita dei worker e con quella indicata dal task; la pagina Worker delle
impostazioni mostra la guida.

**Core senza il contratto.** Una lista vuota resta vuota; una lista con regole è
rifiutata con `ErrorCode::PermissionRulesUnsupported` (non verrebbero mai
applicate); `prepare` dà `None`; la guida è `null` e l'interfaccia non mostra i
campi delle regole.

**Conformità** (`worker_permissions`): forma delle regole completa; guida non
vuota; righe vuote scartate; l'esempio è una regola valida e invariata da
`validate_rules`; senza regole `prepare` dà `Ok(None)`.

### 6. Skill del plugin: `PluginSkills`

**Scopo.** Sessioni interattive guidate da una skill (Task Intake, Skill Wizard,
Project Setup), che partono solo come Bot (`bot:<id>`, `bots/launches.rs`). Il
catalogo è del core (`agents/presets.rs`, `PRESETS`, testi in
`agents/presets/<id>.md`, **in inglese**: li legge il modello).

**Firma** (`contracts/plugin_skills.rs`).

```rust
fn skill_path(&self, skill: &Preset) -> String;  // dove il pacchetto spedisce la skill
```

Il comando di avvio non è più qui: lo costruisce `InteractiveLauncher` (punto 13).

**Chi dichiara `plugin-skills` (campo `plugin_skills`) deve spedire nel proprio
pacchetto la skill di *ogni* voce di `PRESETS`**, al path `skill_path`, con
contenuto identico a `preset.skill_text()`. L'integrazione adatta solo il
*contenitore* (cartella, nome file, frontmatter che l'agente richiede), mai il
testo della skill, e deve poter invocare la skill dal primo prompt. Richiede
quindi `package`.

**Core con il contratto.** Un Bot con una skill del plugin può usare l'agente.
Gli avvii integrati (`launches.rs`) sono solo `builtin:<agent_id>`, nessuna voce
per le skill: nessun avvio per skill né sottomenu “Avvia con…”. Il comando
di ripristino del tab è quello interattivo (riprende la conversazione invece di
rieseguire la skill).

**Core senza il contratto.** Un Bot con una skill del plugin e questo agente
dà l'avviso `does not ship the plugin skills` (`bots/resolve.rs`) e la skill non
viene applicata.

**Conformità** (`plugin_skills`): richiede `package`; ogni skill di `PRESETS` è
spedita a `skill_path` con contenuto identico al catalogo; nessun controllo sul
comando (non esiste più).

### 7. Skill utente: `UserSkills`

**Scopo.** Dire dove l'agente legge le skill personali. L'app archivia le skill
dell'utente (`<dati app>/user-skills`, gestite dal preset Skill Wizard e dai tool
MCP `save_skill`/`list_skills`/`delete_skill`) e le sincronizza in queste cartelle.

**Firma** (`contracts/user_skills.rs`).

```rust
fn windows_dir(&self) -> Result<PathBuf, String>;       // cartella su Windows
fn wsl_home_subpath(&self) -> &'static str;             // relativa alla home Linux, slash dritti
```

**Core con il contratto.** `user_skills/targets.rs` aggiunge un target per
l'agente su Windows e in ogni distro in uso.

**Core senza il contratto.** Le skill non vengono copiate per quell'agente.

**Conformità** (`user_skills`): `wsl_home_subpath` non vuoto, relativo, senza
backslash né `..`.

### 8. Limiti di utilizzo: `UsageLimits`

**Scopo.** Le finestre di limite dell'abbonamento (5 ore, settimanale).

**Firma** (`contracts/usage_limits.rs`).

```rust
fn limits(&self, target: &Target) -> UsageLimitsReply;  // bloccante (rete)
fn notice(&self) -> &'static str;                       // avviso di affidabilità (italiano)
```

`UsageLimitsReply { state, five_hour, seven_day, message }` con `state` in
`ok | expired | unavailable | disabled | unsupported`.

**Core con il contratto.** Il comando `agent_usage_limits` interroga l'agente;
l'avviso `notice` compare accanto all'interruttore dei consumi.

**Core senza il contratto.** Stato `unsupported`, nessun avviso: l'interfaccia
non mostra i limiti di quell'agente. È la scelta giusta per un agente senza
abbonamento a finestre o con limiti non leggibili in modo affidabile.

**Conformità** (`usage_limits`): `notice` non vuoto.

### 9. Consumi della sessione: `SessionUsageReader`

**Scopo.** Token di una sessione letti dal transcript.

**Firma** (`contracts/session_usage.rs`).

```rust
fn session_usage(&self, target: &Target, transcript_path: &str) -> Result<SessionUsage, String>;
```

`transcript_path` arriva dall'evento hook (`HookEvent::transcript_path`) come
path visto dall'agente nel target. L'input è non fidato: va validato (vuoto e
`..` rifiutati). Bloccante (disco): il core lo chiama fuori dal main thread. Una
cache è interna all'integrazione.

**Core con il contratto.** Il comando dei consumi mostra token, contesto e modello.

**Core senza il contratto.** `Ok(None)`: nessun dato, nessun errore.

**Conformità** (`session_usage`): i path `""` e `"../x"` sono rifiutati.

### 10. Profilo di input: `InputProfileSource`

**Scopo.** Tasti propri dell'agente nel terminale, in forma dichiarativa. Oggi
l'unico evento è `ImagePaste` (incolla con appunti senza testo): al posto del
Ctrl+V il terminale invia la sequenza della regola.

**Firma** (`contracts/input_profile.rs`).

```rust
fn input_profile(&self) -> InputProfile;   // InputProfile { rules: Vec<InputRule> }
// InputRule { event: InputEvent, env_kind: Option<TargetKind>, send: String }
```

**Core con il contratto.** Il frontend lo legge all'avvio e lo risolve con
`resolveInputOverride`.

**Core senza il contratto.** Profilo vuoto: il terminale mantiene il comportamento
standard.

**Conformità** (`input_profile`): `send` non vuoto; nessuna regola duplicata per
(evento, ambiente).

### 11. Modelli: `ModelCatalog`

**Scopo.** I modelli che l'agente consiglia e la regola per riconoscerne il nome.
Il core non nomina nessun modello: sceglie per *fascia* (`ModelTier::Standard`
per il lavoro ben definito, `ModelTier::Strong` per progettazione, modifiche
rischiose e bug difficili).

**Firma** (`contracts/models.rs`).

```rust
fn suggested(&self) -> &'static [SuggestedModel];  // { id, label, tier }, in ordine di preferenza
fn default_model(&self) -> Option<&'static str>;   // None = decide l'agente
fn is_valid(&self, model: &str) -> bool;           // la regola dell'integrazione per un nome
```

`is_valid` deve rifiutare tutto ciò che non è sicuro come singolo argomento di una
riga di comando (spazi, `$`, virgolette, backtick, `;`, `|`, `&`, a capo, vuoto).
Claude Code accetta alias e id completi, quindi usa la forma sicura
(`validate::valid_model`) invece di un elenco chiuso.

**Due livelli di regola.** La validazione di un nome di modello ha due barriere, e
non vanno confuse:

1. **Forma generica** (`validate::valid_model_shape`, core): ASCII con cifre,
   `.`, `_`, `-`, `/`, `:`, 1-128 byte, senza `-` o `/` iniziale, `/` finale o
   `//`. Nessun metacarattere di shell. La applicano il parsing delle specifiche
   dei task (`board/spec.rs`), il `pattern` dello schema degli strumenti MCP
   (`MODEL_SHAPE_PATTERN`) e il frontend (`src/lib/model.ts`). Non sa nulla di
   nessun agente: scarta solo ciò che non potrebbe mai essere un nome.
2. **Regola dell'agente** (`ModelCatalog::is_valid`): decide cosa è un modello di
   *quell'agente* (`provider/modello` per OpenCode, alias o id per Claude Code).
   La applicano la creazione e la modifica di un task, le impostazioni dei worker
   e `automation_command`.

La regola dell'agente sta **dentro** la forma generica: la conformità richiede che
ogni modello consigliato e il `default_model` la rispettino. Un agente il cui nome
di modello non passa la forma generica non può dichiarare `models` (si estende la
forma nel core, non la si aggira).

**Cosa deve dichiarare l'integrazione.**

- `suggested`: i modelli che vuole proporre, ciascuno con `id` (il nome come lo vuole il suo
  comando), `label` (il nome per l'utente) e `tier`. Dichiara **almeno un modello per fascia**
  (`Standard` e `Strong`): `tier_models` dà un consiglio per fascia solo se ci sono entrambe, e
  senza le descrizioni degli strumenti MCP restano generiche. L'ordine conta dentro la fascia: il
  primo è quello consigliato.
- `default_model`: il modello da usare quando nessuno lo sceglie. `None` significa che l'app non
  passa nessun modello e decide l'agente (è il caso di Claude Code).
- `is_valid`: la regola per riconoscere un nome. Non è un elenco chiuso (invecchierebbe a ogni
  nuovo modello): è una regola sulla forma, sicura come argomento di una riga di comando.

**Dove finisce, nel core.** Il frontend, i tool MCP e le skill dei preset non nominano mai un
modello: leggono il catalogo.

| Dove | Cosa usa |
| --- | --- |
| Impostazioni → Integrazioni | il contratto `Modelli` fra le funzioni dichiarate |
| Form del task e Impostazioni → Workspace → Worker | `suggested` come suggerimenti del campo modello, `default_model` come segnaposto |
| `get_workspace_settings.agents` | `models` (`id`, `label`, `tier`) e `default_model`, da cui Task Intake e Project Setup scelgono il modello per fascia |
| Tool MCP `propose_tasks` / `update_task` | `tier_models`: i nomi nelle descrizioni del campo `model` |
| Creazione e modifica di task, impostazioni dei worker | `model_is_valid` → `is_valid` |
| Avvio di un worker | `automation_command` rifiuta un modello non valido; il modello è quello del task, poi quello del workspace per l'agente (`worker_agents`), poi `default_model` |

I modelli sono sempre **quelli dell'agente del task**: un modello che l'agente non accetta viene
rifiutato (`invalid model for <nome>: <modello>`) e, cambiando agente nel form, il modello scritto
si svuota se il nuovo agente non lo consiglia.

**Core senza il contratto.** Nessun consiglio: i campi modello non hanno suggerimenti, le
descrizioni del campo `model` non fanno nomi e la validità è la sola forma sicura
(`validate::valid_model_shape`, il primo livello).

**Conformità** (`models`): id consigliati senza duplicati, con etichetta e validi
per `is_valid`; `default_model`, se presente, valido; consigliati e predefinito
dentro la forma generica `valid_model_shape`; `is_valid` rifiuta nomi
vuoti o con spazi, `$`, virgolette, backtick, `;`, `|`, `&`, a capo; se l'agente
dichiara anche `automation`, il comando di avvio accetta ogni modello consigliato
(Windows e WSL). Il test architetturale vieta i nomi dei modelli dell'agente fuori
dalla sua cartella, skill dei preset comprese.

### 12. Effort: `EffortLevels`

**Scopo.** Il livello di ragionamento del modello in una sessione automatica. Il vocabolario
è del core (`Effort`: `low` | `medium` | `high` | `xhigh` | `max`, in serde minuscolo ed
esportato nei bindings); ogni agente dichiara quali livelli sa applicare. I Bot e i worker
chiedono un `Effort`, mai un flag.

**Firma** (`contracts/effort.rs`).

```rust
fn levels(&self) -> &'static [Effort];                    // dal più basso, mai vuoto
fn command_args(&self, effort: Effort) -> Option<String>; // argomenti da aggiungere alla riga di comando
```

- `command_args` è chiamato solo con un livello di `levels`. Il testo del livello viene da
  `Effort::as_str`, mai da una stringa libera: nella riga di comando non finiscono valori
  fuori dall'enum.
- `WorkerAutomation::command` aggiunge `command_args(req.effort)` **solo se** `effort` è
  impostato: con `None` il comando è identico, byte per byte, a quello di prima.
- **Agente senza il contratto** (o livello che non dichiara): l'avvio non fallisce.
  `automation_command` toglie l'effort dalla richiesta, scrive un `log::warn!` e restituisce
  l'avviso in `AutomationStart::warnings` (accanto al comando, in `launch`), così il
  chiamante lo mostra. L'integrazione non vede mai un effort che non ha dichiarato.
- Al frontend: `IntegrationInfo.effort_levels` elenca i livelli di ogni agente (vuoto senza il
  contratto), così l'interfaccia nasconde il selettore; il contratto compare anche fra le
  `capabilities` come `effort`.

**Conformità** (`effort`): almeno un livello, senza doppioni; il contratto `automation` è
richiesto; gli argomenti di ogni livello sono sicuri per la shell e compaiono nel comando di
automazione, su Windows e WSL.

**Claude Code.** Dichiara tutti e cinque i livelli: `--effort <livello>`, in coda al comando
(dopo `--model`). **OpenCode** non lo dichiara.

### 13. Avvio interattivo dei Bot: `InteractiveLauncher`

**Scopo.** Aprire un terminale interattivo *da un Bot* (`bots/agent.rs`): agente, modello,
effort e skill li sceglie il Bot e il comando lo costruisce il backend, mai una stringa
statica (come era per i preset, oggi rimossi: la skill del plugin passa da qui).

**Firma** (`contracts/interactive_launch.rs`).

```rust
fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch>;
fn applies_instructions(&self) -> bool;
// InteractiveRequest { model: Option<&str>, effort: Option<Effort>, skill: Option<LaunchSkill>,
//                      instructions_file: Option<&str> }
// LaunchSkill        { Plugin(&str), User(&str) }
// InteractiveLaunch  { command: String, restore_command: String, env: Vec<(String, String)>,
//                      restore_env: Vec<(String, String)> }
fn is_safe_launch_token(value: &str) -> bool;   // funzione libera del contratto
```

- `command` è digitato nella shell del tab: valido in PowerShell, cmd e bash, e parte da
  `interactive_command()`. Contiene modello, effort e skill.
- `restore_command` tiene modello ed effort ma **mai la skill**: il tab si riapre
  riprendendo la conversazione, non rieseguendo la skill (come il ripristino dei preset).
- `restore_env` sono le variabili che il tab ripristinato dopo un riavvio rimette sul processo
  quando `restore_command` non basta a conservare le scelte del Bot (OpenCode: il modello).
  Mai la skill; stesse regole di `env`; vuota se il comando di ripristino basta (Claude Code).
- `env` segue le regole di `AutomationLaunch::env` (nomi e valori validi, nessun doppione,
  percorsi già nel formato del target); il core le prepara per quel tab (`pty/staged_env.rs`).
- **Carattere sicuro.** Ogni valore digitato nella riga (modello, nome della skill) deve
  superare `is_safe_launch_token`, lo stesso insieme di `--model` (`valid_model`: ASCII
  alfanumerico, `.`, `_`, `-`, 1-64 caratteri); altrimenti `None`. Un agente con una regola
  propria per i modelli (`ModelCatalog::is_valid`) la applica in aggiunta.
- `None` = richiesta non rappresentabile in modo sicuro: il core lo mostra come “caratteri
  non ammessi” e **non ripiega** su un avvio senza modello o skill.
- **Effort.** Il core consegna `effort` solo se `EffortLevels` dell'agente lo ammette; con un
  agente senza il contratto lo toglie e lo segnala in inglese fra gli avvisi
  (`bots::resolve::resolve_bot`). Chi non lo dichiara può quindi ignorarlo.
- **Istruzioni di sistema.** `instructions_file` è un file scritto dall'app nella sua cartella
  dati (oggi la variante interattiva dello standard di ingegneria, per i Bot con `work`), già
  nel formato di path del target: l'agente lo aggiunge alle proprie istruzioni di sistema con
  il suo meccanismo documentato, sia nell'avvio sia nel ripristino (`restore_command` o
  `restore_env`), mai come primo prompt. Un path che non supera `is_shell_safe_path` dà `None`.
  `applies_instructions` dichiara se l'agente lo sa fare: se è `false` il core non lo consegna,
  il Bot parte senza e l'avviso in inglese `<nome> does not support system instructions: the
  engineering standard is not applied` finisce fra gli avvisi del lancio.

**Core con il contratto.** `bots/launches.rs` (puro) produce le voci del menu di avvio
`bot:<id del Bot>` (una per Bot abilitato, con agente effettivo e un eventuale motivo di
blocco in italiano) e, per il Bot scelto, il lancio pronto (`BotTabLaunch`). I comandi Tauri
sono `bot_launches` (economico, nessun processo) e `bot_interactive_launch`, che il
frontend chiama con la chiave del tab che sta per aprire: valida il tab, risolve il Bot con
l'agente del workspace (o il predefinito), verifica che una skill utente esista ancora e,
se `env` non è vuota, la prepara con `pty::stage_env` perché `pty_spawn` la applichi. La
prontezza dell'agente nell'ambiente la valuta il frontend.

**Ripristino.** Al riavvio il frontend chiama `bot_restore_env` per ogni tab con `launchId`
`bot:<id>` (`app/botRestore.ts`) prima di aprirlo: il backend rimette in `pty::stage_env` la
`restore_env` del Bot (`bots::launches::build_bot_restore`), senza ricaricare la skill. Se il Bot
non esiste più o il suo agente è cambiato, il tab riparte comunque con il comando salvato e
l'utente riceve un avviso (mai un ripiego silenzioso).

**Core senza il contratto.** La voce del Bot è bloccata con `<nome> non supporta l'avvio dei
Bot.` e il lancio fallisce con lo stesso messaggio.

**Conformità** (`interactive_launch`): per ogni combinazione di modello (nessuno e i
consigliati), effort (nessuno e i livelli dichiarati) e skill (nessuna, ogni skill del plugin se
`plugin_skills`, una skill utente se `user_skills`) esiste un avvio; `command` e `restore_command`
sono sicuri per la shell e partono da `interactive_command()`; senza skill coincidono; il
ripristino non contiene mai la skill (nemmeno in `restore_env`); `env` e `restore_env` sono valide; gli argomenti dell'effort
dichiarato compaiono in entrambe le righe. Nomi di skill con `$`, `"`, spazi o vuoti e un
modello come `a b;x` sono rifiutati. Se `applies_instructions`, un file di istruzioni sicuro
compare nell'avvio e nel ripristino (nella riga o nell'ambiente) e uno con `$` è rifiutato.

**Claude Code.** `claude --dangerously-skip-permissions [--model <m>] [--effort <livello>]
[--append-system-prompt-file "<file>"] ["/cli-companion-notify:<id>" | "/<nome>"]`: nessuna
`env`. Il flag è nell'help della CLI (`claude --help`, voce `--bare`), aggiunge il file al prompt
di sistema predefinito e resta in `restore_command`; la CLI si ferma se il file non esiste.
**OpenCode.** Il TUI non ha `--model`: il modello (validato da `ModelCatalog::is_valid`)
passa da `OPENCODE_CONFIG_CONTENT` con il solo `{"model":"…"}` (niente permessi né agente
di partenza: un tab interattivo conserva la configurazione dell'utente). La skill, di plugin
o dell'utente, è il primo prompt dei preset (`--prompt "Use the skill tool to load the
skill <id>, then follow its instructions."`); `restore_command` è `opencode --standalone` e `restore_env` ripete `OPENCODE_CONFIG_CONTENT`.
Il file di istruzioni va nella stessa variabile come `{"instructions":["<file>"]}` (documentazione
v2 "Instructions": un path assoluto è usato com'è scritto e le istruzioni configurate seguono gli
`AGENTS.md`). L'effort è ignorato: OpenCode non dichiara `EffortLevels`.

## Procedura passo passo

1. **Leggi l'integrazione di riferimento.** `agents/claude_code/` ha un file per
   contratto (`package.rs`, `hook_events.rs`, `resume.rs`, `automation.rs`,
   `permissions.rs`, `plugin_skills.rs`, `skills.rs`, `usage_limits.rs`,
   `session_usage.rs`, `input_profile.rs`, `models.rs`, `effort.rs`, `interactive_launch.rs`)
   più il rilevamento in `mod.rs`.
   Copia la *struttura*, non i dettagli.
2. **Studia l'agente.** Per ogni contratto che vuoi dichiarare, scrivi su carta come
   l'agente lo realizza e *dove lo hai letto* (documentazione ufficiale, non
   memoria). Ciò che non è documentato si verifica a mano prima di scriverlo nel
   codice (vedi il punto 8).
3. **Crea la cartella `src-tauri/src/agents/<nome>/`** con `mod.rs`: una struct
   unit (`pub struct MioAgente;`), le costanti (`AGENT_ID`, comando interattivo,
   nomi propri dell'agente) e `impl AgentIntegration`. `capabilities()` elenca
   **tutti** i campi: `Some(self)` per i contratti implementati, `None` per gli
   altri. Un file per contratto, con i test del file in fondo.
4. **Rilevamento.** `detect` usa `target.run_timeout(.., DETECT_TIMEOUT)` e
   `parse_version`; su WSL risolvi il path assoluto con
   `resolve_wsl_command_cached` (scarta i programmi Windows visibili dal `PATH`
   di WSL). Il path assoluto serve a tutti i comandi successivi.
5. **Pacchetto.** Genera i file (hook → `--notify <evento> <id>`, server MCP
   `cli-companion` → `<exe> --mcp`, skill dei preset) in modo puro; calcola la
   versione da tutto ciò che genera; implementa installazione, stato e rimozione
   con i comandi o i file di configurazione dell'agente. Idempotente: reinstallare
   non deve lasciare residui (vedi `remove_obsolete_skills` in Claude Code).
   Scrivere in configurazioni dell'utente richiede un marcatore per riconoscere
   ciò che hai scritto tu, e la rimozione deve toccare solo quello.
6. **Eventi, ripresa, automazione, permessi, preset, ...** nell'ordine che ti
   serve. Tutto ciò che finisce in una riga di comando è non fidato: valida e
   rifiuta (`None`) invece di fare escape.
7. **Registra l'integrazione**: `mod <nome>;` in `agents/mod.rs` e una riga in
   `INTEGRATIONS` di `agents/registry.rs`. Il registro è l'unico punto in cui il
   core nomina gli agenti. L'ordine è quello in cui si mostrano le integrazioni e
   la prima è il ripiego quando non c'è nessuna scelta (nessun `integrations.json`
   e nessun pacchetto installato): aggiungila *dopo* quella esistente. Non basta
   registrarla perché l'utente la usi: la abilita lui da Impostazioni → Generale →
   Integrazioni (e può renderla predefinita). Poi dichiara l'agente ai test architetturali: una riga in
   `INTEGRATIONS` di `agents/architecture.rs` (cartella, id, comandi, valori
   propri degli hook come i tipi di notifica) e una in
   quello di `tests/architecture.test.mjs`. Senza la riga Rust il test fallisce
   ("le cartelle di agents/ e INTEGRATIONS devono coincidere").
8. **Esegui i test di conformità.** `cargo test conformance` verifica da solo ogni
   contratto dichiarato. Non c'è niente da registrare nella suite. Poi aggiungi i
   test del file di ogni contratto (casi principali, bordi, fallimenti).
9. **Verifica manuale** su un'installazione reale (dev: `npm run tauri dev`):
   - Impostazioni → Generale → Integrazioni: la scheda compare, con le funzioni
     dichiarate; rileva versione e percorso su Windows e in una distro WSL;
     abilitarla e renderla predefinita funziona (e disabilitarla chiede conferma);
     "Installa", "Aggiorna" e "Rimuovi" funzionano e portano lo stato a “Pronto”
     solo con il pacchetto aggiornato; "Prova notifica" riesce.
   - Avvio integrato `builtin:<id>`: l'agente parte nel tab; a fine turno
     arrivano suono e ✓; una richiesta di permesso dà ❗.
   - Riavvio dell'app: il tab riprende la conversazione (se `session_resume`).
   - Un worker: un task in coda parte con il comando di automazione, l'agente
     chiama `get_task`, consegna con `complete_task` (se `automation`). Prova sia
     scegliendo l'agente nel workspace (Impostazioni → Workspace → Worker) sia
     nel solo task (campo **Agente**), e con l'agente non pronto (pacchetto non
     installato): il task resta in coda con “In attesa” e parte appena lo è.
   - Modelli (se `models`): i suggerimenti compaiono nel form del task e nelle
     impostazioni dei worker; un modello consigliato parte davvero; un nome non
     valido è rifiutato; `get_workspace_settings` (da un Intake) elenca l'agente
     con i suoi modelli e la sua prontezza.
   - Permessi: regole allow/deny rispettate in un worker (se
     `worker_permissions`), impostate per l'agente nelle impostazioni del workspace.
   - Skill del plugin: ognuno dei tre Bot predefiniti (Intake, Project Setup,
     Skill Wizard), con l'agente fissato sul tuo, si avvia e carica la skill.
   - Ogni voce "da verificare" della tua analisi del punto 2.
10. **Documenta.** Aggiorna `README.md` solo se serve all'utente (installazione
    dell'agente) e `docs/DESIGN.MD`, sezione `agents/`, con una sottosezione per la
    tua integrazione. Questa guida cambia solo se cambia un contratto.
11. **Il test architetturale deve restare verde** (`cargo test architecture`,
    `npm test`): se fallisce hai scritto il nome dell'agente fuori dalla sua
    cartella. Il messaggio indica file e riga. Il test legge il solo codice di
    produzione: esclusi commenti, `#[cfg(test)]`/`#[test]` e i moduli dichiarati
    `#[cfg(test)] mod x;` (Rust); commenti e file `*.test.*`, `__tests__`,
    `fixtures` (frontend). Cerca il modulo dell'agente (solo `mod.rs` e
    `registry.rs` di `agents/` possono dichiararlo), le stringhe che contengono il
    suo id e le stringhe uguali ai suoi comandi. Sposta il dettaglio dentro un
    contratto (o, se il contratto non basta, estendilo: vedi sotto).

### Se un contratto non basta

Non aggiungere un `if agent == "..."` nel core. Estendi il contratto (o
aggiungine uno): un file in `contracts/`, un campo in `Capabilities` (e in
`declared()` e `Capability`), il test in `conformance.rs`, l'implementazione nelle
integrazioni esistenti (anche `None`) e la `FakeIntegration`. Poi aggiorna questa
guida.

Due casi reali, entrambi emersi con OpenCode e risolti così (non con un `if`):

- **La forma del nome del modello è demandata all'integrazione.** I modelli di
  OpenCode sono `provider/modello` (a volte `provider/a/b` o con `:tag`), e la
  vecchia barriera generica (`[A-Za-z0-9._-]`, niente `/`) li rifiutava prima
  ancora che l'agente li vedesse. Il core ora ha solo una forma larga e sicura
  (`valid_model_shape`) e lascia la regola vera a `ModelCatalog::is_valid`
  (vedi la sezione 11).
- **Un avvio può avere bisogno dell'ambiente.** OpenCode non ha flag per modello e
  agente nel TUI: si configura con variabili d'ambiente, e digitarle nella riga
  di comando richiederebbe una sintassi diversa per PowerShell, cmd e bash.
  `WorkerAutomation::command` ora ritorna un `AutomationLaunch { command, env }`
  e il core imposta le variabili sul tab (sezione 4).

## Checklist finale

- [ ] `agents/<nome>/` ha un file per contratto, tutti i campi di `Capabilities`
      elencati, nessun `Default`.
- [ ] `id` stabile, `[a-z0-9-]`; registrato in `registry.rs` (una riga).
- [ ] Il nome dell'agente non compare fuori dalla sua cartella
      (test architetturale verde, Rust e frontend).
- [ ] `detect` funziona su Windows e WSL, con timeout, e non scambia un programma
      Windows per uno Linux.
- [ ] Pacchetto: `files` e `version` puri e stabili; install/uninstall idempotenti
      e senza residui; hook su `session-start`, `finished`, `needs-input`
      (e `permission-request`/`session-end` se l'agente li ha); server MCP
      `cli-companion` con `--mcp`.
- [ ] Se dichiari `plugin_skills`: la skill di *tutte* le voci di `PRESETS` è nel
      pacchetto, testo identico al catalogo, invocabile come primo prompt.
- [ ] Se dichiari `automation`: ogni `PermissionMode` mappato; il server MCP è
      utilizzabile senza conferme; prompt, modello e path non fidati rifiutati;
      eventuali `env` con nomi e valori validi, path già nel formato del target.
- [ ] Se dichiari `interactive_launch`: modello e nome della skill passano da `is_safe_launch_token` (o dalla regola dell'agente) e, se non sicuri, `None`; `restore_command` mai con la skill; eventuali `env` con nomi e valori validi; `applies_instructions` vero solo se `instructions_file` arriva all'agente sia all'avvio sia al ripristino (altrimenti `false`: il core avvisa).
- [ ] Se dichiari `models`: `is_valid` rifiuta ciò che non è sicuro come argomento;
      almeno un modello consigliato per fascia (`Standard` e `Strong`), con
      etichetta; `default_model` valido (o `None`); i nomi dei modelli restano nella
      cartella dell'agente.
- [ ] `version` del pacchetto stabile (cambia solo se cambia ciò che generi) e
      `detect` a tempo limitato: da loro dipende la prontezza per ambiente, quindi
      se i task dell'agente partono o restano in attesa.
- [ ] Mostrata con l'integrazione abilitata (non basta registrarla): scheda nelle
      Integrazioni, avvio `builtin:<id>`, e per ogni contratto dichiarato la voce
      corrispondente (Bot con skill del plugin con `plugin_skills`; campo **Agente** dei task e
      scelta dell'agente del workspace con `automation`).
- [ ] Se dichiari `worker_permissions`: l'esempio di `RuleFormat` è valido;
      `prepare` scrive solo se cambia.
- [ ] Testi per l'utente in italiano, testi letti dal modello (skill, preset,
      esempi di regole, errori RPC) in inglese.
- [ ] Nessuna stringa dell'utente interpolata in una shell: prompt, modello e
      path validati, input degli hook parsato con tolleranza.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`, `npm run lint`, `npm run typecheck`, `npm test` (vedi
      [CONTRIBUTING.md](../../CONTRIBUTING.md) per i comandi e la nota su `WSLENV`).
- [ ] Verifica manuale della procedura (punto 9) fatta su un'installazione reale.
- [ ] `docs/DESIGN.MD` aggiornato.

---


## Integrazione di riferimento n. 2: OpenCode

OpenCode (`src-tauri/src/agents/opencode/`) è la seconda integrazione reale. È nata
come esercizio di progetto sulla documentazione ufficiale e poi è stata scritta e
provata: questa sezione documenta **ciò che esiste**, e per ogni punto che
l'esercizio aveva lasciato **[da verificare]** dice l'esito provato. Serve da
esempio di un agente molto diverso da Claude Code: nessun marketplace di plugin,
configurazione in file e in variabili d'ambiente, nessun transcript su file.

Verifiche fatte su **OpenCode v2.0.30, Windows** (vedi "Cosa è stato verificato e
cosa no" in fondo). Legenda: **[provato]** = visto funzionare su quella
installazione; **[limite]** = limite noto e accettato; **[non verificato]** = coperto
solo da test unitari.

### Versioni: solo la 2.x

La pagina di installazione distingue OpenCode V1 e V2 (stesso eseguibile). La v2
ha cambiato il formato della configurazione (regole `permissions` come array di
`{action, resource, effect}`, `mcp.servers`, `plugins` come array) e l'API dei
plugin (`setup(ctx)` con `ctx.mcp`, `ctx.skill`, `ctx.event`, `ctx.session`). Il
formato v1 è ancora accettato e migrato dalla v2, ma l'API dei plugin v1 è diversa.
Per questo l'integrazione supporta **solo la 2.x**: `detect` riporta qualunque
versione (la scheda la mostra), ma `install` rifiuta una versione diversa dalla 2
(`OpenCode 1.14.3 non e' supportato: serve la versione 2.x`) e, senza pacchetto
installato per una versione supportata, l'integrazione non è mai `Pronto`.

### Nucleo

- `id()` → `"opencode"`, `display_name()` → `"OpenCode"`.
- **Eseguibile** [provato]. Su Windows `opencode` è lo shim npm `opencode.cmd`, che
  lancia `node_modules\@opencode\cli\bin\opencode.exe`; `opencode --version` stampa
  `opencode v2.0.30` (`version.rs` salta il nome e la `v`). Il rilevamento usa
  `run_timeout(.., DETECT_TIMEOUT)`. **WSL** [non verificato]: sul PC di sviluppo
  non c'è un'installazione nativa nella distro (lo shim Windows visibile nel `PATH` di
  WSL è scartato da `resolve_wsl_command_cached`, come deve), quindi i percorsi WSL
  sono coperti solo da test unitari.
- `interactive_command()` → `opencode --standalone`. Vedi sotto perché il flag è
  indispensabile.

### Servizio condiviso e `--standalone`

Questa è la scoperta che ha deciso quasi tutto il resto [provato]. Per default il
TUI di OpenCode v2 si collega a un **servizio in background condiviso per utente**,
con l'ambiente di chi lo ha avviato per primo. I plugin e i server MCP locali girano
*lì*, quindi **non vedono** le variabili `CLI_COMPANION_*` del tab (il solo tool
shell le vede). Con `--standalone` il TUI avvia un server privato
(`opencode serve --stdio`) come proprio figlio, che eredita l'ambiente del
terminale: lo vedono il plugin, i figli MCP e la sostituzione `{env:NOME}`.

Conseguenze:

- Ogni comando generato (interattivo, worker, preset, ripresa) usa `--standalone`.
- Il plugin è **inerte** a meno che `CLI_COMPANION_SESSION` sia impostata e `argv`
  non contenga `--service`: nello stesso servizio condiviso non fa nulla, e fuori da
  un tab di cli-companion nemmeno.
- Un `opencode` digitato a mano in un tab funziona ma **senza notifiche né MCP**
  [limite documentato]. La ripresa aggiunge sempre `--standalone`.

### 1. Pacchetto

**Strada scelta: una cartella nostra, scoperta da sola** [provato]. Il pacchetto è
`<config>/plugins/cli-companion-notify/`, dove `<config>` è la cartella che riporta
`opencode debug paths` (riga `config`; `%USERPROFILE%\.config\opencode` su
Windows, `XDG_CONFIG_HOME` rispettata). OpenCode carica da solo i plugin-cartella
con un `index.ts`. File generati (puri, `version`/`files`):

```text
index.ts                       (prima riga: intestazione fissa con la versione)
skills/<id del preset>/SKILL.md
```

Il plugin, a runtime, registra **il server MCP `cli-companion`**
(`ctx.mcp.transform`, comando `<exe> --mcp`) e **le skill dei preset**
(`ctx.skill.transform`): quindi non si modifica nessun file di configurazione
dell'utente, non c'è nessun merge in `opencode.json` e non serve nessuna variabile
d'ambiente per il pacchetto. Provato: OpenCode carica il plugin *generato* con il
vero `cli-companion.exe --mcp`, chiama `get_task` contro l'app in esecuzione e
ottiene il titolo del task.

Dettagli di `install`/`uninstall`/`is_installed` (`package.rs`):

- **Riconoscimento**: la prima riga di `index.ts` è un'intestazione fissa
  (`// cli-companion-notify <versione>: generated by cli-companion, ...`). Una
  cartella con lo stesso nome senza quell'intestazione è **estranea** e l'installazione
  la rifiuta; la rimozione cancella solo la nostra cartella.
- **Copia e destinazione**: i file si scrivono anche in `plugin_root`
  (`<dati app>/agent-plugins/opencode/<target>/`), insieme a `destination.txt` con la
  cartella in cui il plugin è stato messo; `uninstall` usa quella registrata (e se
  manca, la rilegge da `debug paths`). La cartella di destinazione si rifà da zero a
  ogni installazione, niente avanzi.
- **`version`**: `<release>-h<hash>` con `fnv32` su indice e skill, quindi cambia
  quando cambia il plugin o il testo di un preset.

Alternative provate e scartate:

- `OPENCODE_CONFIG_DIR`, `OPENCODE_CONFIG` e `OPENCODE_CONFIG_CONTENT` esistono e sono
  rispettate (precedenza osservata: globale < `OPENCODE_CONFIG` / `CONFIG_DIR` <
  progetto < `CONFIG_CONTENT`). `CONFIG_DIR` non è stata scelta per il pacchetto:
  servirebbe in ogni terminale e sostituirebbe un eventuale valore dell'utente. Le
  altre due le usa l'automazione (sotto).
- Non esiste un flag `--config`.
- Un plugin **file singolo** nell'array `plugins` viene saltato in silenzio:
  valgono solo le cartelle. È il motivo della cartella con `index.ts`.

### 2. Eventi degli hook

Il plugin sottoscrive `ctx.event.subscribe` (il filtro `types` **non** è applicato
nella 2.0.30: filtra il plugin stesso; l'id di sessione è in `data.sessionID`) e per
ogni evento esegue `<exe> --notify <evento> opencode` con uno stdin JSON **nostro**
(chiavi `session_id`, `source`, `continuing`, `wait`, `message`, `tool`, `summary`,
`reason`): `normalize` è un lettore semplice e testato, senza dipendere da un
formato altrui. Esiti provati [provato salvo dove indicato]:

| Evento cli-companion | Evento OpenCode | Note |
| --- | --- | --- |
| `session-start` | `session.created` | Parte al primo prompt, non all'apertura del TUI. `source` `startup` la prima volta, `clear` le successive. Se invece compare un evento di una sessione mai vista (`--session`, `--continue`: nessun `session.created`) si manda `session-start` con `source` `resume`. |
| `finished` | `session.execution.succeeded` | `session.idle` e `session.status` esistono nei tipi ma non scattano mai. |
| `needs-input` | `permission.asked` (`wait` permission, `tool` = azione, `summary` = risorse), `form.created` con `metadata.kind` question (`wait` question), `session.execution.failed` (`wait` other, il messaggio d'errore, ad es. la quota finita) | `permission.asked` manda anche `permission-request`. |
| `session-end` | `session.deleted` | **Solo questo** [limite]: chiudere il TUI non emette nulla (il server privato è terminato senza eseguire i gestori d'uscita: provati `exit`, `beforeExit`, segnali, chiusura di stdin). Un worker il cui OpenCode è stato chiuso resta `running` finché il tab non viene chiuso. |

Le sessioni dei subagenti (con `parentID`) sono ignorate.

**Blocco del fine turno** [provato]. `stop_block_output` scrive su stdout
`{"continue":"<motivo>"}`; il plugin lo legge dall'output di `--notify finished` e
chiama `ctx.session.prompt({ sessionID, text })`: parte un secondo turno e il
`finished` successivo porta `continuing: true`, che il core traduce in
`already_continuing`. `transcript_path` resta `None` (le sessioni stanno in SQLite).

### 3. Ripresa della sessione

`opencode --session <id>` riprende (gli id iniziano con `ses`; un id inesistente ne
crea una nuova), `--continue` l'ultima [provato].

```rust
ResumeSpec {
    command_names: ["opencode", "opencode.exe", "opencode.cmd"],
    strip_flags: [ --standalone, --session/-s (con valore), --continue/-c, --fork ],
    resume_template: "{command} --standalone --session {id}",
}
```

Si toglie anche `--standalone` per non duplicarlo: il modello lo rimette sempre.

### 4. Automazione dei worker

**`opencode run` non è adatto** [provato]: esce alla prima fine di turno (quindi
non può essere bloccato e fatto proseguire) e si blocca se stdin è una pipe aperta.
I worker usano il **TUI**: `opencode --standalone --prompt "<bootstrap>"` (`--prompt`
invia da solo), più `--auto` per il `Bypass`. Il TUI **non ha** `--model` né
`--agent`, quindi il resto passa dall'`env` di `AutomationLaunch` (la prima
applicazione reale di quel contratto):

| Variabile | Contenuto |
| --- | --- |
| `OPENCODE_CONFIG_CONTENT` | JSON compatto: `model` (se dato), `default_agent` (`plan` o `build`) e le regole di permesso della modalità |
| `OPENCODE_CONFIG` | solo se il workspace ha regole: il file preparato da `WorkerPermissions::prepare` (path nel formato del target) |

Mappatura dei livelli (`mode_rules.rs`):

| `PermissionMode` | Agente | Regole di primo livello |
| --- | --- | --- |
| `Plan` | `plan` | tutto `ask`; `read`/`glob`/`grep`/`skill`/`question`/`execute` e `cli-companion_*` `allow`; i file `*.env` e `*.env.*` `ask` (gli `*.env.example` `allow`) |
| `Default` | `build` | le stesse regole di `Plan` |
| `AcceptEdits` | `build` | quelle di `Default` più `edit` `allow` |
| `Bypass` | `build` | solo `execute` e `cli-companion_*` `allow`, con `--auto` |

Le regole del workspace stanno nel file di `OPENCODE_CONFIG` sotto
`agents.build.permissions` e `agents.plan.permissions`: vengono *dopo* tutte le regole
di primo livello e l'ultima che corrisponde vince, quindi **le regole del workspace
battono quelle di modalità**. Con `--auto` i `deny` espliciti valgono ancora
[provato: `git log` negato, `git status` consentito, `echo` chiesto].

**Server MCP senza conferme** [provato]. Il modello non chiama il tool direttamente:
con il "Code Mode" predefinito di OpenCode usa il tool `execute` e scrive
`tools["cli-companion"].get_task()`. L'azione di permesso è `cli-companion_<tool>`; la
regola `cli-companion_*` `allow` (più `execute` `allow`) lo rende utilizzabile senza
conferme. Il prompt di bootstrap, "Call the get_task tool", funziona **invariato**
(provato con più modelli gratuiti); ogni chiamata MCP porta
`_meta["ai.opencode/sessionID"]`.

**Modello**: `provider/modello`. Il `command` rifiuta (`None`) un modello che
`ModelCatalog::is_valid` non accetta; il modello valido va nel JSON, mai nella riga.

### 5. Permessi dei worker

Sintassi utente: `azione(risorsa)` oppure solo `azione` (= qualunque risorsa),
esempio `shell(git status *)` (`RuleFormat::example`); `*` e `?` come wildcard,
l'ultima regola che corrisponde vince. `validate_rules` fa trim, scarta righe vuote e
doppioni, controlla forma, lunghezza e caratteri di controllo. `prepare` scrive
`<dati app>/worker-permissions/opencode/<ws>.json` (solo se cambia) con le regole
`allow` e poi `deny` sotto `agents.build/plan.permissions`, e ritorna il path per
`OPENCODE_CONFIG`; un path non sicuro per la shell è un errore. La `guide` spiega
che nel bypass i `deny` restano validi.

### 6. Preset

Le skill dei preset le registra il **plugin** (non servono comandi personalizzati).
Una skill **non è un comando slash**: si carica con il tool `skill`, quindi il primo
prompt è `opencode --standalone --prompt "Use the skill tool to load the skill <id>,
then follow its instructions."` [provato nel TUI]. `skill_path` è
`skills/<id>/SKILL.md` e il contenuto è `preset.skill_text()` identico (lo verifica la
conformità). Un file di comando personalizzato avrebbe funzionato (`--prompt "/cmd"`
si espande nel TUI, non in `run`) ma non serve.

### 7. Skill utente

Dichiarata: `windows_dir()` → `<config>/skills` (`XDG_CONFIG_HOME/opencode/skills`
o `~/.config/opencode/skills`), `wsl_home_subpath()` → `.config/opencode/skills`.
OpenCode legge anche `~/.claude/skills`; a parità di id vince
`~/.config/opencode/skills` [provato con una home finta]: abilitando entrambe le
integrazioni non ci sono doppioni.

### 8. Limiti di utilizzo, 9. Consumi, 10. Profilo di input

Non dichiarati (`None`):

- `usage_limits`: OpenCode usa chiavi e abbonamenti dei provider, nessuna finestra.
- `session_usage`: le sessioni stanno nel database SQLite `opencode.db`, non c'è un
  transcript su file e il contratto prende un path (vedi "Punti in cui il core
  dovrebbe crescere").
- `input_profile`: l'incolla di immagini non è stata verificata, vale il comportamento
  standard del terminale.

### 11. Modelli

Forma `provider/modello` (anche `provider/a/b`, `:tag`, es. `ollama/llama3:8b`).
Consigliati (`models.rs`): `opencode/gpt-5.4-mini` (Standard), `opencode/gpt-5.5`
(Strong) e `opencode/big-pickle` (Standard), tutti di OpenCode Zen; `default_model`
`None` (vale la configurazione dell'utente). `is_valid` = `valid_model_shape` più un
provider valido (minuscole, cifre, `.`, `_`, `-`) e segmenti non vuoti dopo la prima
`/`: un nome senza provider è rifiutato. È il secondo livello descritto nella
sezione 11 dei contratti.

### 12-13. Effort e avvio interattivo dei Bot

L'effort non è dichiarato (nessun flag nel TUI). L'avvio interattivo dei Bot (`interactive_launch.rs`)
riusa le parti già provate: il modello va in `OPENCODE_CONFIG_CONTENT` con il solo campo `model`
(stessa variabile dei worker, senza permessi né agente di partenza), la skill è il primo prompt
(`plugin_skills::skill_command`, condiviso) e il ripristino è `opencode --standalone`. Il file di
istruzioni di sistema è la chiave `instructions` dello stesso JSON (`mode_rules::interactive_content`).

### Riepilogo: cosa dichiara OpenCode

| Contratto | Esito | Perché |
| --- | --- | --- |
| `package` | Sì | cartella plugin nostra; MCP e skill registrati a runtime; solo 2.x |
| `hook_events` | Sì | stdin deciso dal nostro plugin; blocco del fine turno provato |
| `session_resume` | Sì | `--session`/`--continue` provati, sempre con `--standalone` |
| `automation` | Sì | TUI con `--prompt` e `env` (`OPENCODE_CONFIG_CONTENT`/`OPENCODE_CONFIG`); `run` non adatto |
| `worker_permissions` | Sì | `azione(risorsa)`, file in `OPENCODE_CONFIG` che batte le regole di modalità |
| `plugin_skills` | Sì | skill del plugin caricate dal primo prompt |
| `user_skills` | Sì | `<config>/skills`; nessun doppione con `~/.claude/skills` |
| `usage_limits` | No | nessuna finestra di utilizzo |
| `session_usage` | No | niente transcript su file |
| `input_profile` | No | incolla immagini non verificato |
| `models` | Sì | `provider/modello`, regola propria dentro la forma generica |
| `effort` | No | nessun flag di effort nel TUI né nell'avvio (solo `run --model provider/modello#variante`, non verificato in `OPENCODE_CONFIG_CONTENT`) |
| `interactive_launch` | Sì | modello in `OPENCODE_CONFIG_CONTENT` (il TUI non ha `--model`), skill dal primo prompt, effort ignorato (non dichiarato) |

Cosa vede l'utente: abilitata da Impostazioni → Generale → Integrazioni e con il
pacchetto installato (**Installa**) per una 2.x rilevata, una scheda “OpenCode”, l'avvio
`builtin:opencode`, OpenCode come agente fissabile nei Bot (anche con skill del plugin),
OpenCode come **Agente** dei worker e dei task con i modelli suggeriti e le regole
nelle impostazioni dei worker. Senza pacchetto o con una versione diversa dalla 2 i
task che lo usano restano in coda con “In attesa”.

### Cosa è stato verificato e cosa no

Fatto a mano su questo PC (OpenCode v2.0.30, Windows):

- rilevamento della versione e di `opencode debug paths`;
- ciclo reale installa / `is_installed` / rimuovi in una home finta (test ignorato
  `opencode_real_install_cycle_in_a_fake_home`, lanciabile con `cargo test -- --ignored`);
- OpenCode che carica il plugin **generato** con il vero `cli-companion.exe --mcp`,
  chiama `get_task` contro l'app in esecuzione e riceve il titolo del task;
- il prototipo del plugin nel TUI: `session-start`, `finished`, blocco e
  continuazione, `permission-request`/`needs-input`, caricamento di una skill da
  `--prompt`;
- le regole di precedenza dei permessi, `--auto` con `deny`, la home finta delle skill.

**Non fatto attraverso l'interfaccia dell'app** (la build di sviluppo non può girare
accanto all'app installata: stessa cartella dati). Checklist manuale rimasta:

- [ ] pagina Integrazioni: OpenCode “Trovato” e poi “Pronto” dopo **Installa**;
- [ ] tab `builtin:opencode`: parte, a fine turno suono e ✓, richiesta di permesso ❗;
- [ ] un Bot con una skill del plugin (con OpenCode come agente fissato e come agente del workspace);
- [ ] un worker completo fino a `complete_task`, con modello e regole del workspace;
- [ ] ripresa del tab dopo il riavvio dell'app;
- [ ] WSL con un OpenCode nativo nella distro.

### Fonti

Documentazione v2 di OpenCode (consultata il 2026-11-04):
[Home](https://opencode.ai/v2/docs/) ·
[Config](https://opencode.ai/v2/docs/config/) ·
[Plugins](https://opencode.ai/v2/docs/plugins/) ·
[Scrivere plugin](https://opencode.ai/v2/docs/build/plugins/) ·
[Server](https://opencode.ai/v2/docs/build/server/) ·
[MCP servers](https://opencode.ai/v2/docs/mcp-servers/) ·
[Permissions](https://opencode.ai/v2/docs/permissions/) ·
[Skills](https://opencode.ai/v2/docs/skills/) ·
[Commands](https://opencode.ai/v2/docs/commands/) ·
[CLI](https://opencode.ai/v2/docs/cli/).
Dove la documentazione e il comportamento differivano ha vinto la prova sulla 2.0.30
(per esempio il filtro `types` degli eventi, `session.idle`).

### Punti in cui il core è cresciuto, e quelli che restano

Emersi dall'integrazione (il core non si aggira con un `if`, si estende un contratto).
**Fatti**, con Claude Code che continua a passare:

1. **Modelli `provider/modello`**: la validazione del *nome* è demandata
   all'integrazione (`ModelCatalog::is_valid`, usata da `automation_command`,
   `resolve_spec_agent` e dalle impostazioni dei worker); resta una prima barriera
   generica sulla forma, `valid_model_shape` (ammette `/` e `:`), nel parsing delle
   specifiche (`board/spec.rs`), nel `pattern` degli strumenti MCP
   (`MODEL_SHAPE_PATTERN`) e nel frontend.
2. **Variabili d'ambiente per un avvio**: `AutomationLaunch { command, env }`, con
   validatori nel contratto, preparazione per tab nel backend (`pty/staged_env.rs`,
   `pty/env.rs`) e inoltro su WSL con `WSLENV` e `/u`.

**Restano**:

3. **Consumi senza transcript su file**: `SessionUsageReader` presuppone un file
   (`transcript_path`); OpenCode ha le sessioni in SQLite.
4. **Fine sessione senza un evento di uscita**: il core conosce `session-end` solo se
   l'agente lo manda; un agente che muore in silenzio lascia il worker `running` fino
   alla chiusura del tab.

Ognuno è un cambio del core coperto da test di conformità e dalla
`FakeIntegration`, e va fatto con le integrazioni esistenti che continuano a passare.
