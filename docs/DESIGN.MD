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
- Persistenza: **JSON** (`workspaces.json` nella app-data dir di Tauri)

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

- `Workspace { id, name, kind, path, distro?, shell?, created_at, launches }`
  - `kind`: `"windows"` o `"wsl"`
  - `path`: percorso Windows (`C:\...`) oppure percorso Linux assoluto (`/...`)
  - `distro`: solo per WSL; `shell`: override opzionale
  - `launches`: `Launch { id, name, command, is_default }` (`#[serde(default)]`,
    id `ln-<millis>-<n>`), salvati dentro `workspaces.json`; al massimo uno
    predefinito per workspace; rimossi insieme al workspace.
- Stato condiviso `WorkspaceStore` (in memoria + salvataggio su ogni modifica).
- File: `<app_data>/workspaces.json`. Se corrotto, backup in `.json.bak` e riparto vuoto.
- Comandi: `list_workspaces`, `add_workspace`, `rename_workspace`, `remove_workspace`,
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
  - ricontrolla che la cartella esista al momento dello spawn.
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
- Se stdin non e' un terminale (`IsTerminal`) viene letta (tetto 1 MB, solo dopo
  la decisione di inviare) e parsata come JSON, con tolleranza ai fallimenti:
  `session_id` -> `agent_session_id`, `notification_type`, `message`
  (troncato). Il self-test lancia l'exe con stdin nulla.
- Payload (una riga JSON su TCP `127.0.0.1:17421`, poi uguale nell'evento Tauri
  `agent-event`):
  `{event, agent, tab_id?, agent_session_id?, notification_type?, message?}`.
- Da WSL l'exe gira sull'host via interop: il TCP resta locale, nessun
  networking cross-VM.
- Comando: `test_agent_notify` (self-test via --notify).

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
    `needs-input`. Passi: `plugin validate`, pulizia best-effort, `plugin marketplace
    add`, `plugin install --scope user`. La versione del plugin deriva dagli hook
    (Claude cache per versione: exe spostato = versione nuova). La cartella e'
    caricata sul posto e deve restare su disco.
  - stato installato: `claude plugin list --json` contiene `id` ==
    `cli-companion-notify@cli-companion-local` (array o oggetto con `plugins`).
  - rimozione: `plugin uninstall` + `marketplace remove` + cartella generata.

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
- `App.tsx`: layout sidebar + barra dei workspace + area pannelli; stato globale
  (`workspaces`, `tabs`, `layout`, dialog). `tabs` è nell'ordine di creazione
  (ordine di render); gruppi, pannello attivo, ordine nella griglia e maximize
  vivono in `layout` (`lib/layout.ts`).
- `lib/layout.ts`: funzioni pure zero-dep, testate da `tests/layout.test.mjs`.
  `gridRects(n)` = griglia automatica (cols = ceil(sqrt n), rows = ceil(n/cols), ultima
  riga incompleta allargata a tutta larghezza, rettangoli in %), `reorderIds`,
  e le transizioni di stato dei gruppi (`openTab`, `closeTabIn`, `selectGroup`,
  `setActivePane`, `toggleMaximize`, `reorderTabs`, `computePaneLayouts`).
- **Gruppi per workspace**: la barra in alto ha un tab per ogni workspace con almeno un
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
- `components/Sidebar.tsx`: card workspace con badge `WIN`/`distro`, icona
  "Apri cartella" (`open_workspace_folder` -> explorer.exe), menu "⋯" (avvii del
  workspace, "Gestisci avvii…", Rinomina, Elimina) e "+ Terminale" che esegue
  l'avvio predefinito se presente. Rinomina inline, elimina con doppia
  conferma (niente `confirm()` nativo, per robustezza nella WebView).
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
- JSON invece di SQLite: adeguato per poche centinaia di workspace; lo store
  è isolato in `workspaces.rs`, migrabile in futuro.
- Nessuna autenticazione/cifratura: i path sono dati locali non sensibili.
- Bundle JS ~1 MB (gluestack + RN-web): accettabile per app desktop,
  valutabile code-splitting se cresce.

## Comandi utili

- `npm run tauri dev` — avvio app in sviluppo
- `npm run build` — typecheck + build frontend
- `cargo check` (in `src-tauri/`) — typecheck backend
