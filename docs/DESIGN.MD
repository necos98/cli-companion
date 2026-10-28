# cli-companion — Design

App desktop (Windows) per gestire cartelle di lavoro ("workspace") locali e WSL2
e aprirci dentro terminali integrati a tab.

## Stack

- **Tauri v2** + **Rust** (backend), **React 18 + TypeScript + Vite** (frontend)
- UI: **shadcn/ui** (componenti in `src/components/ui/`, stile new-york) +
  **Tailwind CSS v4** (tema dark in `src/index.css`, alias `@/` -> `src/`).
  Nota storica: la prima versione usava gluestack-ui v1 + react-native-web,
  rimossi perché incompatibili con il dev server Vite 8 (sorgenti .jsx/.tsx
  grezzi non pre-bundlati -> schermata bianca). Vedi `tests/no-gluestack.test.mjs`.
- Terminali: **xterm.js** (`@xterm/xterm` + `@xterm/addon-fit`) nel frontend,
  **portable-pty** (ConPTY su Windows) nel backend
- Persistenza: **JSON** (`workspaces.json`, `session.json`, `boards/<workspace_id>.json`, `schedule.json` nella app-data dir di Tauri)
- Board dei task: **React Flow** (`@xyflow/react`) + **dagre** (`@dagrejs/dagre`) per il layout a grafo

## Architettura

```text
+------------------+  invoke (comandi)   +------------------+
|  React (WebView) | ------------------> |  Rust (Tauri)    |
|  - Sidebar       | <-- canali ---------|  - workspaces.rs |
|  - Tab terminali |  uno per sessione pty|  - wsl.rs        |
|  - xterm.js      |                      |  - pty.rs        |
+------------------+                      +---------+--------+
                                                    |
                                    +---------------+---------------+
                                    |                               |
                              ConPTY: powershell/cmd          wsl.exe -d <distro>
                              cwd = cartella                  --cd <linux-path>
```

Frontend e backend comunicano solo via comandi Tauri (`invoke`) per le azioni
ed eventi Tauri (`emit`/`listen`) per lo stream di output dei terminali.

## Backend (`src-tauri/src/`)

### `workspaces.rs` — store JSON + CRUD

- `Workspace { id, name, kind, path, distro?, shell?, created_at, launches, default_launch?, worker_permission, worker_model?, delivery, use_schedule }`
  - `kind`: `"windows"` o `"wsl"`
  - `path`: percorso Windows (`C:\...`) oppure percorso Linux assoluto (`/...`)
  - `distro`: solo per WSL; `shell`: override opzionale
  - `launches`: `Launch { id, name, command }` (`#[serde(default)]`,
    id `ln-<millis>-<n>`), salvati dentro `workspaces.json`; rimossi insieme al
    workspace. Il vecchio `is_default` si legge solo per la migrazione.
  - `default_launch`: avvio predefinito del workspace. Assente = eredita il
    predefinito globale (`launch_settings.json`, solo integrati); `"shell"` =
    shell vuota; altrimenti l'id di un avvio del workspace o di un integrato
    (`builtin:<agent_id>`, `builtin:preset:<id>` per ogni preset, da `agents::builtin_launches`).
    Al caricamento `migrate_default` converte il vecchio `is_default` e `builtin:orchestrator` in `builtin:preset:task-intake` (stessa migrazione per `default_launch` di `launch_settings.json`).
  - `worker_permission`: tetto dei permessi dei worker automatici, `"plan" | "default" |
    "accept-edits" | "bypass"` (vocabolario di `agents::PermissionMode`);
    `#[serde(default)]` -> `"accept-edits"`, cosi' i vecchi `workspaces.json` non diventano
    piu' permissivi.
  - `worker_allow` / `worker_deny`: `Vec<String>` (serde default), pre-approved / forbidden
    Claude Code permission rules for workers (`Bash(git:*)`, `Edit`, ...), set with
    `set_worker_permission_rules(id, allow, deny)`; validated by `worker_permissions::validate_rules`
    (trimmed, max 100 rules of 300 chars, shape `Name` or `Name(...)`, no control chars,
    no parentheses on `mcp__` names). At launch of a Claude Code worker (`Host::worker_settings_path`) `{"permissions":{"allow":[..],"deny":[..]}}` is written
    (only if changed) to `<app_data>/worker-settings/<workspace_id>.json` and the command gets
    `--settings "<path>"` (before `--allowedTools`, which is variadic; `/mnt/<d>/...` for WSL;
    paths with `" $ ` % !` are refused because the line is typed into PowerShell, cmd or
    bash). Claude Code merges these with its other settings; deny always wins, even in
    bypass. Caveats shown in the UI: a prefix rule such as `Bash(docker exec:*)` allows any
    command starting that way; deny rules are not a security boundary (other invocation
    forms can bypass them).
  - `worker_model`: `Option<String>`, modello di default dei worker automatici, usato quando
    il task non indica `model`; assente = quello dell'agente (`#[serde(default)]`).
  - `delivery`: `Delivery { mode: pr|local_merge|push|commit (default commit), parallel (default
    false, solo pr/local_merge), merge_method: squash|merge|rebase (default squash) }`; comando
    `set_delivery(id, delivery)` (normalizza `parallel`) e riallineamento dello scheduler.
  - `use_schedule`: bool, i task partono solo nelle fasce orarie; `set_use_schedule(id, value)`.
  - Migrazioni al caricamento (`parse_workspaces`, sul JSON): `auto_orchestrate.enabled` ->
    `use_schedule`; `worktree_setup` -> `repos["."].setup` se vuoto; i due campi spariscono.
- Stato condiviso `WorkspaceStore` (in memoria + salvataggio su ogni modifica).
- File: `<app_data>/workspaces.json`. Se corrotto, backup in `.json.bak` e riparto vuoto.
- Comandi: `list_workspaces`, `add_workspace`, `rename_workspace`, `remove_workspace`,
  `set_worker_permission(id, mode)` (valida il valore, ritorna il `Workspace`),
  `set_worker_model(id, model)` (trim, vuoto = nessuno, validato con `valid_model`),
  `set_delivery(id, delivery)`, `set_use_schedule(id, value)`,
  `add_launch`, `update_launch`, `remove_launch` (ritornano il `Workspace`
  aggiornato; nome 1-80 caratteri, comando 1-1000, trim come `src/lib/launch.ts`),
  `open_workspace_folder` (`explorer.exe`; per WSL il percorso UNC
  `\\wsl.localhost\<distro>\...`; l'exit code di explorer è ignorato).
- `add_workspace` valida tutto prima di salvare:
  - Windows: il path deve esistere ed essere una cartella;
  - WSL: distro non vuota, path assoluto (accetta anche `C:\...` o `\\wsl$\...`
    convertendoli), cartella esistente nella distro (via `wsl test -d`);
  - niente duplicati a parità di `kind + path + distro`;
  - nome vuoto -> usa l'ultimo segmento del percorso.

### `wsl.rs` — interop WSL2

- `installed_distros()`: esegue `wsl.exe -l -q` e parsifica l'output.
  Nota: `wsl.exe` scrive in **UTF-16LE con BOM**, quindi la decodifica gestisce
  UTF-16LE/BE e UTF-8, con filtro difensivo su righe spurie.
- `wsl_dir_exists(distro, path)`: `wsl.exe -d <distro> -- test -d <path>`.
- `windows_to_wsl_path()`: `C:\a\b` -> `/mnt/c/a/b`,
  `\\wsl$\Distro\home\u` -> `/home/u`.
- Le chiamate a `wsl.exe` usano `CREATE_NO_WINDOW` (niente flash di console).
- Comandi esposti: `list_wsl_distros`, `check_wsl_path` (per la verifica live nel form).

### `pty.rs` — sessioni terminale

- Ogni tab del frontend corrisponde a una sessione PTY numerata (`u32`),
  tenuta in `PtyState { sessions: Arc<Mutex<HashMap<...>>>, next_id }`
  (limite: 32 sessioni).
- `pty_spawn(workspace_id, tab_id, cols, rows)` (`tab_id` = chiave UUID del tab,
  validata: ASCII alfanumerico/`-`/`_`, max 64):
  - Windows: `powershell.exe -NoLogo` (o shell custom) con cwd = cartella;
  - WSL: `wsl.exe -d <distro> --cd <path> [-e <shell>]`;
  - entrambi i rami impostano `TERM=xterm-256color` + `COLORTERM=truecolor`
    (senza questi hint molti CLI disabilitano i colori perché l'app GUI
    non ne eredita alcuno); niente `FORCE_COLOR`, per rispettare
    `NO_COLOR`/`--no-color`;
  - entrambi i rami impostano `CLI_COMPANION_SESSION=<tab_id>`: gli hook degli
    agenti ereditano la variabile e cosi' l'evento si lega al tab. Su WSL si
    aggiunge `CLI_COMPANION_SESSION/u` a `WSLENV` (valore del processo padre
    preservato, senza duplicati: `merge_wslenv`) perche' la variabile attraversi
    il confine verso la distro e, via interop, torni all'exe Windows dell'hook;
  - impostano anche `CLI_COMPANION_TOKEN` (token dell'app, `automation::token()`),
    propagato su WSL con `/u` nello stesso `WSLENV` (`merge_wslenv` accetta piu'
    variabili): serve a `--notify` e `--mcp` per autenticarsi sul socket;
  - `cwd` opzionale (usato dalle sessioni automatiche) al posto della cartella del
    workspace, validato come questa (Windows `is_dir`, WSL `wsl_dir_exists`);
  - ricontrolla che la cartella esista al momento dello spawn.
- `PtySession` tiene il `tab_id`; `pty_write` avvisa `automation::user_input` quando
  l'input contiene Invio (il worker che aspettava l'utente torna `running`).
- Un thread di lettura per sessione inoltra i chunk al frontend sul `Channel`
  passato a `pty_spawn` (`{kind:"output",data}`; alla chiusura `{kind:"exit"}`):
  ogni tab riceve solo il proprio output, anche quello prima del ritorno dello spawn.
- Decodifica UTF-8 **incrementale**: i chunk possono spezzare un carattere
  multibyte, quindi i byte incompleti restano in coda fino al chunk dopo
  (niente `�` su accenti/box-drawing).
- Comandi: `pty_spawn`, `pty_write`, `pty_resize`, `pty_close`.
  `pty_close` uccide il processo figlio; il thread di lettura invia comunque
  `exit`, così frontend e backend non divergono.

### `agent_notify.rs` — canale di notifica degli agenti (niente semaforo)

- Gli hook dei coding agent chiamano questo stesso exe:
  `cli-companion --notify <evento> [<agente>]` (ramo con early-return in
  `main.rs`, come `--check-update`).
- Eventi (`KNOWN_EVENTS`): `test` (self-test), `session-start`, `finished`,
  `needs-input`. Gli sconosciuti (es. i vecchi `stop`/`session-end` dei plugin
  Muse) sono ignorati in silenzio con exit 0.
- Il tab di appartenenza arriva dalla variabile `CLI_COMPANION_SESSION`: un
  evento diverso da `test` senza (o con valore non valido) e' ignorato con exit 0,
  perche' la sessione dell'agente gira fuori da cli-companion
  (`should_send(event, tab_id)`, pura). Solo l'invio fallito di un evento da
  inviare da' exit 1.
- `run_notify` e' solo il wrapper che legge l'ambiente reale (`CLI_COMPANION_SESSION`,
  `CLI_COMPANION_TOKEN`) e usa la porta `NOTIFY_PORT`; la logica sta in `notify_target`, che
  riceve tab, token, porta, lettura di stdin e stdout espliciti. I test usano solo
  `notify_target` con porte effimere: la suite gira anche dal terminale di un worker, dove
  quelle variabili sono vere, e un evento partito da un test arriverebbe all'app a nome del
  worker (un `session-end` gli faceva fallire il task). Protezione: notifiche e RPC si connettono
  solo da `connect_local`, che nei build di test (`cfg(test)`) fallisce con un panic se la porta
  e' `NOTIFY_PORT`, prima di connettersi; due test `should_panic` lo verificano.
- Se stdin non e' un terminale (`IsTerminal`) viene letta (tetto 1 MB, solo dopo
  la decisione di inviare) e parsata come JSON, con tolleranza ai fallimenti:
  `session_id` -> `agent_session_id`, `notification_type`, `message`
  (troncato), `source` (`session-start`: `startup|resume|clear|compact|fork`), `reason`
  (`session-end`: `clear|resume|logout|prompt_input_exit|other`), `stop_hook_active` (bool) e
  `background_work` (numero di
  `background_tasks` con stato non concluso, tollerante e senza maiuscole, piu' le
  voci di `session_crons`; `None` se i due array mancano). Il self-test lancia
  l'exe con stdin nulla.
- Payload (una riga JSON su TCP `127.0.0.1:17421`, poi uguale nell'evento Tauri
  `agent-event`):
  `{event, agent, tab_id?, agent_session_id?, notification_type?, message?, token?,
  stop_hook_active?, background_work?, source?, reason?}` (campi opzionali: i client vecchi
  restano validi).
  `token` viene da `CLI_COMPANION_TOKEN`; il listener lo toglie prima dell'`emit`
  verso il frontend e, se valido, passa l'evento hook anche ad `automation`.
- Sullo stesso socket passano due tipi di riga, distinti dalla chiave `rpc`:
  - **notify**: il payload sopra, limite 64 KB. Il listener risponde con una riga JSON,
    `{}` oppure `{"block":"<motivo>"}`, scritta dopo che `automation` ha elaborato l'hook
    (scrittura best-effort: un client che ha gia' chiuso non e' un errore). Solo
    `--notify finished` con token legge la riga (timeout 4 s; timeout, chiusura o
    risposta illeggibile = exit 0 senza output, come prima): se contiene `block` stampa su
    stdout esattamente `{"decision":"block","reason":"<motivo>"}` (formato dello Stop hook
    di Claude Code, che fa proseguire il turno) ed esce con 0. Nient'altro viene mai
    scritto su stdout da `--notify`;
  - **RPC** (nuova): `{"rpc":1,"token","session","method","params"}`, limite 1 MB,
    risposta di una riga `{"ok":true,"result":..}` o `{"ok":false,"error":".."}`.
    Token errato o `session` non valida (`is_valid_tab_id`) -> `ok:false`.
    `automation::rpc::dispatch` la esegue. Il client `rpc_call(port, line,
    timeout)` e' usato da `mcp.rs`; il timeout di lettura della richiesta e' 2 s.
    Nessuna RPC resta in attesa: rispondono tutte subito.
- Da WSL l'exe gira sull'host via interop: il TCP resta locale, nessun
  networking cross-VM.
- Comando: `test_agent_notify` (self-test via --notify).

### `toast.rs` — notifica cliccabile (Windows)

- `show_agent_notification(tab_id, title, body)`: toast con
  `tauri-winrt-notification` (il plugin notification non espone il click su
  desktop). `tab_id` validato con `is_valid_tab_id`; audio della toast
  disattivato (suona gia' l'app).
- AUMID: l'`identifier` quando installata; da `target\debug|release` ripiega
  su `Toast::POWERSHELL_APP_ID` (come tauri-plugin-notification), quindi in dev
  la toast risulta di PowerShell ma il click funziona.
- `on_activated`: `unminimize` + `show` + `set_focus` sulla finestra `main`,
  poi evento `notification-activated` `{tab_id}`. Il frontend applica
  `focusTab` (layout.ts): workspace, pannello attivo, de-massimizzazione.
  Fuori da Windows il comando da' errore e `systemNotify.ts` usa il plugin.

### `agents/` — plugin dei coding agent

- `mod.rs`: `Env` (Windows | distro WSL; `command()` = `cmd /c prog` su Windows,
  `wsl.exe -d D -- prog` su WSL, sempre `CREATE_NO_WINDOW`), il trait
  `AgentProvider` (`id`, `display_name`, `detect`, `is_installed`, `install`,
  `uninstall`), il registro `PROVIDERS` e i comandi Tauri
  `agent_env_status` (sonda in parallelo, con `std::thread::scope`, ogni coppia
  agente x env: Windows + ogni distro, escluse quelle `docker-desktop*`),
  `install_agent_plugin(agent_id, env_key)`, `remove_agent_plugin(agent_id, env_key)`.
- Aggiungere un agente: un file in `agents/` che implementa `AgentProvider` + una
  riga in `PROVIDERS`. La UI (Impostazioni -> Coding agent) raggruppa per agente
  da sola.
- Automazione (facoltativa): `AgentProvider::automation_command(env_kind, &AutomationRequest)
  -> Option<String>` (default `None`) costruisce il comando da digitare nel tab per
  avviare una sessione automatica. `AutomationRequest { bootstrap_prompt,
  permission_mode: PermissionMode (Default | AcceptEdits | Plan | Bypass), model }`.
  Helper pubblico `automation_command(agent_id, env_kind, req)`.
  Claude Code: `claude "<bootstrap>" --permission-mode <m> --allowedTools
  mcp__plugin_cli-companion-notify_cli-companion [--model <m>]` (prompt prima dei flag:
  `--allowedTools` e' variadico; la regola senza nome tool abilita tutti i tool del server,
  necessario perche' `acceptEdits` non approva i tool MCP). `bypassPermissions` al primo
  avvio interattivo mostra un dialogo di conferma (salta con
  `skipDangerousModePermissionPrompt: true` in `~/.claude/settings.json`).
- Profili di input: `AgentProvider::input_profile() -> InputProfile` (default vuoto)
  con regole dichiarative `InputRule { event: InputEvent, env_kind: Option<"windows"|"wsl">,
  send }`. `InputEvent` (kebab-case) e' il punto di estensione; oggi solo
  `ImagePaste`. Claude Code: `ESC v` (Alt+V) per windows e wsl. Il comando
  `agent_profiles` li espone (nessun processo); l'App li carica all'avvio e
  `resolveInputOverride(profiles, agentId, envKind, event)` (`lib/agents.ts`) sceglie
  la sequenza. L'agente del tab e' `tabAgentId`: stato del plugin (`session-start`..
  `session-end`), altrimenti il primo token del comando di avvio/ripristino
  (`claude`/`claude.exe`). `TerminalView` riceve la sequenza gia' risolta
  (`imagePasteSeq`, via ref) e la invia al posto di `` quando gli appunti non
  hanno testo; senza regola il comportamento resta invariato.
- `claude_code.rs`:
  - rilevamento: Windows `cmd /c claude --version`; WSL: path assoluto via
    `bash -lc 'command -v claude'` (ripiego `bash -ic`, ultima riga che inizia con
    `/`), poi `<abs> --version`; il path assoluto serve per tutti i comandi
    successivi nella distro. Parsing in funzioni pure testate.
  - installazione: marketplace locale generato in
    `<app_data>/agent-plugins/claude-code/<env>/` (`cli-companion-local`, plugin
    `cli-companion-notify`), hook in forma exec (`command` = exe corrente con slash
    dritti, o `/mnt/c/...` su WSL; `args` = `["--notify", evento, "claude-code"]`,
    `timeout` 10): `SessionStart` -> `session-start`, `Stop` -> `finished`,
    `Notification` (matcher `permission_prompt|idle_prompt|elicitation_dialog`) ->
    `needs-input`. `PermissionRequest` (no matcher, no output, exit 0: it never decides, the
    normal dialog still shows) -> `permission-request`: stdin `tool_name` + `tool_input` are
    summarised (Bash command / file path / URL, max 200 chars) into
    `Permesso richiesto: Bash — <command>`; the backend turns it into a `needs-input`
    (`permission_prompt`) carrying that text, and drops the generic `permission_prompt`
    Notification that follows for the same tab within 60 s (no duplicate event/sound).
    The hook fires only when a prompt is actually shown (not in bypass, not when a rule
    already allows). Il plugin contiene anche `.mcp.json` con il server `cli-companion`
    (`command` = exe come sopra, `args` = `["--mcp"]`) e una skill per ogni preset di
    `agents/presets.rs` (`Preset { id, name, skill }`): `skills/<id>/SKILL.md`, sorgente
    `agents/presets/<id>.md` inclusa con `include_str!` e normalizzata a LF, in inglese,
    invocabile come `/cli-companion-notify:<id>`. Oggi tre preset: `task-intake` (il
    metodo dell'agente Intake: vedi [BOARD.MD](BOARD.MD)), `skill-wizard` (intervista l'utente
    e crea una skill globale con i tool MCP `save_skill`, `list_skills`, `delete_skill`; le skill
    utente sono archiviate in `<app data>/user-skills` e sincronizzate come destinazione in
    `~/.claude/skills`, dove le cartelle gestite portano il marcatore `.cli-companion-managed`) e
    `project-setup` (esplora il repository, legge la configurazione con `get_workspace_settings` e
    la propone con `propose_settings`: vedi [BOARD.MD](BOARD.MD));
    aggiungere un preset = una voce in `PRESETS` + un file md. Le skill `orchestrate` e `workflow` non esistono piu': a ogni
    installazione `remove_obsolete_skills` toglie da `skills/` le cartelle che non sono
    preset. Passi: `plugin validate`,
    pulizia best-effort, `plugin marketplace add`, `plugin install --scope user`. La
    versione del plugin (`<release>-h<hash>`) deriva da hook + `.mcp.json` + le skill dei preset
    (Claude cache per versione: exe spostato o skill cambiata = versione nuova; i plugin con
    una versione diversa da quella attesa sono segnalati da `agent_plugins_outdated` e
    reinstallati da `agent_plugins_update`). La cartella e' caricata sul posto e deve restare su disco.
  - stato installato: `claude plugin list --json` contiene `id` ==
    `cli-companion-notify@cli-companion-local` (array o oggetto con `plugins`).
  - rimozione: `plugin uninstall` + `marketplace remove` + cartella generata.

### `automation.rs` — Task Manager: coda, scheduler e worker

Ogni workspace ha una coda di task; l'app la esegue con uno scheduler deterministico e un
worker (tab PTY marcato automatico) per task. Il worker consegna da solo e riferisce con
`complete_task`; l'Intake (una sessione normale) propone task. Contratto completo (modello
dati, consegna, scheduler, tool, UI): [BOARD.MD](BOARD.MD). Struttura a strati, in `automation/`:

- `automation.rs`: `Registry`, nucleo **puro** (niente `AppHandle`, tempo iniettato): worker
  (stati, hook, `get_task`, `complete_task`, chiusura del tab), board e `Effect` da eseguire.
  Tetto di 256 worker tracciati in memoria. Qui anche i comandi Tauri.
- `state.rs`: `AutomationState { Mutex<Registry> }` (managed state). `run` modifica sotto lock,
  schedula le board toccate (`flush_boards`) e applica gli effetti **fuori** dal lock;
  pubblicazione ordinata per `rev` (evento `board-update` + salvataggio coalescente, max 1/s).
- `host.rs`: trait `Host` (eventi Tauri, git, file, timer, workspace) e `TauriHost`; i test usano
  un host finto con i timer in linea.
- `boards.rs`: parte del `Registry` che tiene le board (proposte, azioni sui task, scheduler,
  guardiano degli avvii) e la logica dei comandi e dei tool di sessione.
- `worktrees.rs`: avvio di un task (base, `git fetch`, worktree, setup, spawn), pulizia a
  consegna avvenuta, rilevamento dei repository, esecuzione di git (timeout 120 s, WSL via
  `wsl.exe -e`). `hooks.rs`: setup e pulizia per repository (`RepoConfig`).
- `rpc.rs`: `dispatch` smista per ruolo. Worker: `get_task`, `complete_task`. Sessione normale
  (qualunque altro tab di cli-companion, workspace = quello del tab): `propose_tasks`,
  `list_tasks`, `get_task_details`, `update_task`, `delete_task`, `get_workspace_settings`,
  `propose_settings` (`settings.rs`: patch validata in `workspaces/settings_patch.rs`, proposte
  in `board/settings.rs`). `whoami` non e' un tool:
  e' l'handshake con cui il server MCP sceglie i tool.
- `auto.rs`: fasce orarie e standby (sotto).
- `agent_session.rs`: legame worker <-> sessione dell'agente e riapertura dopo un
  `session-end` (sotto).

Stati del worker: `starting` -> `running` (`get_task`) -> `completed` | `failed`
(`complete_task`) -> `closed` (tab chiuso). In mezzo, dagli hook di `--notify` (solo con token
valido): `needs_input` (hook needs-input, tranne `idle_prompt`), `background` e `idle`.
`Registry::on_hook` su `finished`: con `background_work > 0` -> `background`; altrimenti, con
`stop_hook_active != true`, risposta `block` con `STOP_BLOCK_REASON` (promemoria in inglese);
con `stop_hook_active == true` -> `idle`. `needs_input` e `idle` = "richiede attenzione" e
mandano la notifica. Nessun hook segnala la risposta dell'utente: `pty_write` avvisa il backend
quando nel terminale di un worker in attesa si preme Invio (`Registry::user_input`) e il worker
torna `running`.

Sessione dell'agente (`agent_session.rs`). Un processo secondario avviato dal worker (un
`claude` figlio, un subagent) eredita `CLI_COMPANION_SESSION` e gli hook del plugin: i suoi
eventi arrivano col tab del worker ma con un altro `session_id`. Il worker lega a se'
(`AgentSession`) il primo `agent_session_id` che vede (di norma dal suo `session-start`). Gli
eventi con un id diverso (`session-end`, `finished`, `needs-input`, ...) sono ignorati: niente
fallimento, niente promemoria dello Stop, niente attenzione. Un `session-start` con un altro id
rilega il worker solo se `source` e' `clear`, `resume` o `compact` (la stessa sessione utente che
continua); `startup` o `fork` di un altro id sono processi secondari. Un `session-end` della
sessione legata con `reason` `clear` o `resume` non chiude il worker: l'agente prosegue nello
stesso processo con una nuova sessione (che poi rilega col suo `session-start`). Eventi senza
id: ammessi finche' il worker non e' legato (plugin vecchi, comportamento invariato), estranei
dopo il legame, per tutti gli eventi. Ogni hook di Claude Code (`SessionStart`, `Stop`,
`SessionEnd`, `Notification`, `PermissionRequest`) porta `session_id` nello stdin, quindi un
evento senza id accanto a un id legato non viene dal worker (es. `--notify` lanciato a mano o da
un test nel suo terminale); estendere la regola anche a `finished` e `needs-input` evita
promemoria dello Stop e stati di attenzione falsi. Un `session-end` ignorato lascia una traccia
nella nota del task (`ignored a session-end: ...`, con l'id estraneo, `it carried no agent
session id` o il `reason`).

Riapertura. Se un `session-end` ha comunque chiuso il worker (stato `closed`, task `failed` con
la nota `the session ended without a complete_task report`) ma il suo tab e' ancora aperto,
il `complete_task` di quel worker riapre il task (`Board::reopen_failed`: solo se e' ancora
`failed` con quella nota e lo stesso `attempts` del worker, cioe' nessun retry l'ha ripreso),
poi lo conclude come sempre con il report (`done`/`failed`, pulizia, chiusura del tab). La
nota diventa `reopened: ...`. Tab chiuso davvero (`tab_closed`) o task ripreso da un retry:
`the session is closed`. La notifica `failed` gia' partita non si ritira; dopo la riapertura
vale la regola normale (notifica solo se il report e' `failed`, nessuna per `done`).

Notifiche di sistema: evento `task-notify` `{workspace_id, task_id, tab_id?, kind:
"attention"|"failed", title, body}` (testo in italiano); il frontend mostra la notifica.

Tutto cio' che legge il modello e' in inglese (`BOOTSTRAP_PROMPT`, `STOP_BLOCK_REASON`,
istruzioni, ambiente e passi di consegna in `board/texts.rs`, `automation/worker_guardrails.md`,
descrizioni dei tool MCP, errori delle RPC). UI e commenti restano in italiano.

```text
Intake: propose_tasks --RPC--> board.proposals --board-update--> UI: Accetta --> coda (queued)
scheduler (dopo ogni modifica) --> running --> thread: base, worktree, setup --> emit automation-spawn
W: claude "<bootstrap>" --> get_task --> lavora, consegna (commit/push/PR/merge) --> complete_task
                                                          dopo 1,5 s: automation-close; chiuso il tab: pulizia
```

Pulizia: a `complete_task completed` il tab si chiude dopo 1,5 s; quando `automation_tab_closed`
lo conferma (e dopo 2 s, perche' Windows rilasci la cartella) l'app esegue i `cleanup` del
repository, `git worktree remove` e `git branch -D` (`pr`) / `-d` (`local_merge`). Gli errori
diventano una nota sul task. `failed` conserva worktree e branch per il retry.

Eventi Tauri verso il frontend: `automation-spawn`, `automation-close`, `automation-update`,
`board-update`, `task-notify`, `schedule-state`. Comandi: `automation_tab_closed`,
`automation_sessions`, `board_get`, `board_set`, `task_create`, `task_update`, `task_action`,
`task_move`, `proposal_decide`, `settings_proposal_decide`, `workspace_detect_repos`. Comando di avvio del worker:
`agents::automation_command`; permessi = `Workspace.worker_permission`, modello = `task.model`
poi `worker_model`.

### `board.rs` — modello, validazione e scheduler della coda

Parte **pura** (nessun I/O, tempo iniettato), con i sottomoduli `board/spec.rs` (`TaskSpec`,
id, repository, grafo delle dipendenze, id generati dal titolo), `board/report.rs` (report di
`complete_task`), `board/texts.rs` (istruzioni, ambiente e consegna per modalita'). Contiene
`Board`, `Task` (stati `queued|running|done|failed`), `Proposal`, `BoardView`, la normalizzazione
dei file della v0.11 (sul JSON, al caricamento), le operazioni sulla coda (accetta/scarta,
modifica, elimina, riprova, riordina) e `schedule`: in modalita' parallela (`pr`/`local_merge`
con `parallel`) fino a `max_parallel` running, altrimenti un running per repository; l'ordine
dell'array e' l'ordine della coda. Persistenza: `<app_data>/boards/<workspace_id>.json`
(`write_atomic`); al riavvio i `running` diventano `failed`.

### Fasce orarie e standby (`schedule.rs`, `automation/auto.rs`)

`<app_data>/schedule.json` (`{enabled, windows:[{start,end}], keep_awake}`, ora locale `HH:MM`,
finestra `[start, end)` che puo' attraversare la mezzanotte, max 12). Logica pura testata
(`window_open`, `next_change`). Sono solo un **gate dello scheduler**: con schedule abilitato,
un workspace con `use_schedule` non avvia task fuori fascia (quelli in corso finiscono);
`BoardView.schedule_blocked` lo espone. Un thread dell'app fa un giro ogni 30 s (guardiano degli
avvii, `schedule-state` se cambia, le board il cui gate e' cambiato ripartono). `keep_awake`: con
il flag attivo l'app impedisce lo standby (Windows `SetThreadExecutionState`, thread dedicato)
finche' c'e' un task `running` in un workspace non archiviato. Comandi: `schedule_get`,
`schedule_set`.

### `mcp.rs` — server MCP stdio (`cli-companion --mcp`)

- Ramo con early-return in `main.rs` come `--notify`. JSON-RPC 2.0 newline-delimited su
  stdin/stdout: `initialize`, `notifications/initialized`, `ping`, `tools/list`,
  `tools/call`; metodo ignoto con id -> errore -32601. Niente altro su stdout.
- E' uno **shim**: ogni `tools/call` apre una connessione TCP a `127.0.0.1:17421`
  (`agent_notify::rpc_call`), invia la riga RPC con `CLI_COMPANION_TOKEN` e
  `CLI_COMPANION_SESSION` dall'ambiente e restituisce il risultato come testo JSON
  (errore RPC -> `isError: true`). Read timeout 30 s.
- Il ruolo si risolve con `whoami` e decide i tool esposti: worker (`get_task`,
  `complete_task`) o sessione normale (`propose_tasks`, `list_tasks`, `get_task_details`,
  `update_task`, `delete_task`, `get_workspace_settings`, `propose_settings`). Le definizioni sono in `mcp/tools.rs`.
  Senza variabili o con l'app non raggiungibile la lista tool e' vuota.
- Logica pura (`handle_message` con `trait Backend`), testata con un backend finto.

### `session.rs` — persistenza della sessione

File nella cartella dati dell'app, scritti con `write_atomic`:
- `session.json`: blob JSON opaco (lo schema è del frontend, campo `version`),
  tetto 2 MB. Comandi `load_session` / `save_session`.
- `scrollback/<tab_id>.txt`: cronologia serializzata di un terminale, tetto 4 MB.
  `save_scrollback`, `load_scrollback`, `delete_scrollback`; il `tab_id` è validato
  con `agent_notify::is_valid_tab_id` (stesso charset del `CLI_COMPANION_SESSION`).
  `prune_scrollback(keep)` elimina i file dei tab non elencati (chiamato dopo il ripristino).

### `lib.rs`

Registra i plugin (`opener`, `dialog`, `clipboard-manager`, `notification`), gli stati condivisi, il caricamento
dello store in `setup` e tutti i comandi. Capability minime in
`capabilities/default.json` (`core:default`, `core:window:allow-destroy` per la chiusura dopo il salvataggio, `opener:default`, `notification:default`, `dialog:allow-open`, clipboard).

## Frontend (`src/`)

- `lib/tauri.ts`: tipi (`Workspace`, eventi PTY) e wrapper `api` sopra `invoke`/`listen`.
- `App.tsx`: layout barra del titolo + sidebar + area pannelli + barra di stato; stato globale
  (`workspaces`, `tabs`, `layout`, dialog). `tabs` è nell'ordine di creazione
  (ordine di render); gruppi, pannello attivo, ordine nella griglia e maximize
  vivono in `layout` (`lib/layout.ts`).
- `lib/layout.ts`: funzioni pure zero-dep, testate da `tests/layout.test.mjs`.
  `gridRects(n)` = griglia automatica (cols = ceil(sqrt n), rows = ceil(n/cols), ultima
  riga incompleta allargata a tutta larghezza, rettangoli in %), `reorderIds`,
  e le transizioni di stato dei gruppi (`openTab`, `closeTabIn`, `selectGroup`,
  `setActivePane`, `toggleMaximize`, `reorderTabs`, `computePaneLayouts`).
- **Gruppi per workspace**: la barra del titolo ha un tab per ogni workspace con almeno un
  terminale (nome, numero di terminali, stato aggregato: ❗ > ✓ > pallino giallo).
  Ordine = prima apertura; aprire un terminale ne attiva il gruppo; il click su una card
  della Sidebar (non sui pulsanti) seleziona il gruppo; l'ultimo terminale chiuso fa
  sparire il gruppo e ne seleziona un altro.
- **Pannelli**: ogni terminale ha un'intestazione sottile (etichetta, stato, massimizza,
  chiudi; `PaneHeader.tsx`). Click ovunque nel pannello = pannello attivo + focus xterm.
  Trascinando l'intestazione su un altro pannello dello stesso gruppo (HTML5 DnD, per
  questo `dragDropEnabled: false` in `tauri.conf.json`) si sposta in quella posizione.
  Maximize: bottone, doppio click sull'intestazione o Ctrl+Maiusc+M (listener in
  cattura su `window`; niente Esc, serve alle CLI); per gruppo, gli altri pannelli
  restano vivi e nascosti.
- **Vista globale**: il 📌 nell'intestazione fissa un terminale; i fissati compaiono nel
  tab "Vista globale (N)" (primo della barra, visibile solo con N ≥ 1, con l'indicatore
  aggregato ❗/✓/in corso). È un gruppo speciale in `layout` (`GLOBAL_ID`, `global`:
  `order`/`activeTabId`/`maximizedTabId`, più `activeView`) con griglia, riordino,
  maximize e Ctrl+Maiusc+M propri; le intestazioni mostrano "workspace · etichetta". Il
  tab resta anche nel suo gruppo: ha un solo rettangolo, preso dalla vista mostrata
  (`computePaneLayouts`), le altre viste lo nascondono (`active`/`rect`, come al cambio
  gruppo, quindi fit + `pty_resize` automatici). Chiudere un tab lo toglie dalla vista.
  `focusTab` (click su notifica) e il click sulla Sidebar vanno sempre al gruppo del
  workspace. Sessione: `global` e `activeView` opzionali in v1 (nessun bump di versione).
- **Nessun rimontaggio**: tutti i `TerminalView` di tutti i gruppi sono figli diretti di un
  solo contenitore, con `key` = tabId e nell'ordine di creazione. Riordino, cambio
  gruppo, maximize e ripristino cambiano solo lo stile (rettangolo in % /
  `display: none`), mai l'albero DOM: smontare un `TerminalView` chiude il pty. `active`
  = visibile (gruppo attivo e non nascosto da un maximize), `focused` = riceve la tastiera.
- **Persistenza** (`lib/session.ts`, zero-dep, `tests/session.test.mjs`): schema
  `{ version: 1, activeWorkspaceId, groups: [{ workspaceId, order, activeTabId,
  maximizedTabId }], tabs: [{ tabId, workspaceId, launchId?, command, title?, agentId?,
  agentSessionId? }] }`. Salvataggio con debounce di 500 ms a ogni modifica e alla
  chiusura (`onCloseRequested`, con tetto di 3 s). `parseSession` tollera garbage, versioni
  sconosciute, id non validi e tab di workspace spariti (li scarta, ricostruendo i gruppi).
  Al primo avvio dopo il caricamento dei workspace i tab si ricreano con gli stessi tabId:
  ogni `TerminalView` scrive prima la cronologia salvata e la riga
  `— sessione ripristinata —`, poi lancia il pty. Il comando è `restoreCommand(tab)`:
  con `agentSessionId` valido e comando `claude`/`claude.exe` è il comando originale senza
  `--resume/-r/--continue/-c` più `--resume <id>`; con id ma senza comando claude è
  `claude --resume <id>`; altrimenti il comando originale. Poi `prune_scrollback`.
- **Scrollback**: `@xterm/addon-serialize` (ultime 2000 righe con colori, senza buffer
  alternativo né modi) salvato ~5 s dopo l'output e alla chiusura dell'app; eliminato
  alla chiusura del tab (anche se la shell esce da sola).
- **Finestra senza decorazioni** (`decorations: false`, `shadow: true`; il resize dai
  bordi lo gestisce tao): `components/TitleBar.tsx` (44px) ha logo, tab dei gruppi
  (`★ Globale (N)` per primo, poi i workspace; tab attivo fuso col contenuto sotto, tinta
  arancio se serve una risposta; scroll orizzontale in overflow) e i pulsanti
  minimizza / massimizza-ripristina / chiudi (46px, chiudi rosso). Le zone libere sono
  `data-tauri-drag-region` (trascina; doppio clic = massimizza); i controlli no.
  Chiudi usa `close()`, quindi passa dall'`onCloseRequested` di `App.tsx` (salva sessione
  e scrollback), mai `destroy()` diretto. Capability: `core:window:allow-{minimize,
  toggle-maximize,close,start-dragging,is-maximized}`.
- `components/Sidebar.tsx` (260px): intestazione "WORKSPACE" + "+" (aggiungi workspace);
  riga = chip ambiente (`WIN`/`WSL`), nome, percorso mono. Le righe non attive sono
  compatte (al passaggio del mouse: "+" nuovo terminale, cartella, menu "⋯"); il
  workspace selezionato e' una card con l'elenco dei suoi terminali (stato, etichetta,
  pin; il click usa `focusTab`) e il pulsante diviso "+ Terminale" (avvio predefinito) con
  caret sul menu degli avvii. Menu "⋯": avvii, "Gestisci avvii…", Rinomina, Elimina
  (`DropdownMenu modal={false}`; le Impostazioni si aprono con un `setTimeout` a menu
  chiuso). Rinomina inline, elimina con doppia conferma. Footer: Impostazioni (pallino
  accent se c'e' un update) e versione (`getVersion()`).
- `components/PaneHeader.tsx` (30px): stato, etichetta mono (prefisso "workspace /" nella vista
  globale), parola di stato dell'agente, pin / massimizza / chiudi. Area dei pannelli su
  `--pane-area` con 6px di margine; pannello attivo col bordo accent; bersaglio di
  trascinamento tratteggiato con "Rilascia qui per spostare"; stato vuoto con logo.
  Resta valido: i `TerminalView` sono figli diretti di un unico contenitore fisso.
- `components/StatusBar.tsx` (26px): "N terminali", "K in attesa di risposta" e, a destra, gli
  ambienti con il plugin installato (`installedAgentsSummary`). `agent_env_status` lancia
  processi: e' chiamato una volta, in background ~1,5 s dopo il ripristino della sessione, e
  aggiornato dalle Impostazioni dopo installa/rimuovi (`onAgentEnvs`).
- **Tema e brand**: tema dark unico in `src/index.css` (palette `base`, `surface`, `surface-2`,
  `border-strong`, `text-secondary`, `faint` (solo decorativo), `pane-area`, `pane-header`,
  stati `status-running/attention/finished`), font Geist / Geist Mono bundlati (offline),
  logo "C" in `components/Logo.tsx` (icone in `src-tauri/icons`, sorgenti in `icons/source`).
- `components/AddWorkspaceDialog.tsx`: modal gluestack con toggle Windows/WSL.
  Windows: picker cartella nativo (`@tauri-apps/plugin-dialog`) o path manuale.
  WSL: lista distro con caricamento async, input percorso con bottone "Verifica".
- `components/SettingsDialog.tsx`: due tab, "Avvii personalizzati" (selettore
  workspace, lista/modifica/elimina/predefinito, form di aggiunta; ogni
  modifica è salvata subito via comandi) e "Coding agent" (`AgentSettingsTab.tsx`: per agente, una riga per env con
  badge, stato, versione e Installa/Reinstalla/Rimuovi; "Prova notifica" e log).
- `lib/agents.ts`: logica pura zero-dep (stato per env, raggruppamento, macchina a
  stati dell'agente per tab, testi delle notifiche), testata da
  `tests/agents.test.mjs`. Stato per tab in `App.tsx` (`Tab.agent`, chiave = `tab_id`):
  `session-start` marca il tab come agente (idle) e salva `agentSessionId`; Invio
  -> running; `finished` -> ✓; `needs-input` -> ❗; qualsiasi input lo toglie. Per
  i tab agente sostituisce il pallino busy/idle. Gli eventi con `tab_id` senza
  tab sono ignorati (tranne `test`: solo suono). Suoni in `notifySound.ts`
  (`playDing`, `playAttention`); a finestra senza focus anche una notifica di
  sistema (`systemNotify.ts`, `tauri-plugin-notification`).
- **Sessioni automatiche** (`lib/automation.ts`, zero-dep, `tests/automation.test.mjs`):
  etichette/colori di stato, tooltip, filtro dei tab automatici. `App.tsx` ascolta
  `automation-spawn` (apre il tab con `startCommand` = comando di avvio, `cwd` e
  `Tab.automation` con padre/titolo/stato; usa `openTabBackground` di `layout.ts`, che
  aggiunge il pannello al gruppo del workspace senza cambiare gruppo attivo né focus),
  `automation-close` (stesso percorso di `closeTab`) e `automation-update` (stato e
  ultimo report); alla chiusura di un tab automatico chiama `automation_tab_closed`.
  All'avvio `automation_sessions()` riallinea lo stato. `PaneHeader` mostra il badge AUTO
  (INTAKE per i tab avviati da un preset) e la parola di stato. I tab automatici sono esclusi dal salvataggio di sessione e
  scrollback e non vengono ripristinati.
- **Tasks Manager** (`components/tasks/`, `lib/board.ts`, `lib/boardLayout.ts`, `lib/useBoards.ts`):
  `lib/board.ts` (zero-dep, testato) ha i tipi del contratto (`BoardView`, `Task`, `Proposal`,
  `Delivery`), i quattro stati (+ "Richiede attenzione") con etichette e colori, selettori, ricerca/filtri,
  formati di durata e consegna, `mergeBoard` per `rev`. `useBoards` (in `App.tsx`) carica `board_get`,
  ascolta `board-update` e `task-notify` (notifica di sistema); i componenti chiamano `api` e passano la
  `BoardView` restituita a `storeBoard`. `TaskPanel` / `GlobalTaskPanel` stanno a sinistra della griglia
  dei terminali (compresso/espanso per workspace in localStorage, `lib/taskPanelPrefs.ts`); `TasksManager`
  (Ctrl+Maiusc+B o pulsante "Tasks" della sidebar) copre pannello e griglia: i `TerminalView` restano
  montati (regola del DOM sopra). `DependencyGraph` = React Flow (`@xyflow/react`) + auto-layout con
  `@dagrejs/dagre` (`boardLayout.ts`). "Task Intake" (`openIntake` in `App.tsx`) apre un nuovo tab del
  preset `builtin:preset:task-intake` a ogni richiesta (più chat in parallelo); `PaneHeader` mostra il badge INTAKE (il `launchId` del
  tab è già persistito nella sessione). Dettagli in [BOARD.MD](BOARD.MD).
- `components/ui/dropdown-menu.tsx`: wrapper shadcn di Radix, usato dal menu
  "⋯" della card workspace.
- `lib/launch.ts`: pure functions zero-dep (sanitizza, trigger, input pty
  con , costanti di timing, `defaultLaunch`), coperte da
  `tests/launch.test.mjs`.
- `components/TerminalView.tsx`: un'istanza xterm per tab, con la sua cornice
  (posizione assoluta da `rect`, intestazione, bordo evidenziato se attivo).
  Il comando dell'avvio (catturato al mount, `initialCommand`) viene iniettato via `pty_write`
  al primo output della shell + 150 ms, con fallback a 2,5 s per gli avvii
  freddi; una sola iniezione per tab, timer puliti all'unmount.
  - Tema Campbell in `src/lib/terminalTheme.ts` (stessi 16 ANSI di Windows
    Terminal); l'host di xterm è un div interno senza padding (FitAddon
    misura il contenitore: il padding esterno sposterebbe l'ultima riga
    fuori vista). Refit di recupero dopo lo spawn e a font pronti.
  - Lo spawn avviene al mount; i tab inattivi restano montati ma nascosti
    (`display: none`) per non perdere lo stato della shell; ogni pannello visibile
    si adatta (`fit()` + `pty_resize`) quando cambia il suo rettangolo.
  - `ResizeObserver` + evento "torna attivo" tengono sincronizzate dimensioni
    xterm e PTY (`fit()` + `pty_resize`).
  - Cleanup completo all'unmount (unlisten, `pty_close`, `dispose`), robusto
    anche al double-mount di `StrictMode` in dev.

## Scelte e limiti noti

- Solo Windows + WSL per ora: il codice WSL/PTY è Windows-first
  (`wsl.exe`, ConPTY via portable-pty).
- Le board sono file JSON per workspace (`boards/`), come lo store dei workspace.
- Lo schedule e' un file a parte (`schedule.json`): le fasce sono globali, ogni workspace
  sceglie se rispettarle (`use_schedule`).
- JSON invece di SQLite: adeguato per poche centinaia di workspace; lo store
  è isolato in `workspaces.rs`, migrabile in futuro.
- Nessuna autenticazione/cifratura: i path sono dati locali non sensibili.
- Bundle JS ~1 MB (gluestack + RN-web): accettabile per app desktop,
  valutabile code-splitting se cresce.

## Comandi utili

- `npm run tauri dev` — avvio app in sviluppo
- `npm run build` — typecheck + build frontend
- `cargo check` (in `src-tauri/`) — typecheck backend
