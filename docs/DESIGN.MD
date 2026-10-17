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
|  - Sidebar       | <-- eventi ---------|  - workspaces.rs |
|  - Tab terminali |  pty-output/pTy-exit |  - wsl.rs        |
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

- `Workspace { id, name, kind, path, distro?, shell?, created_at }`
  - `kind`: `"windows"` o `"wsl"`
  - `path`: percorso Windows (`C:\...`) oppure percorso Linux assoluto (`/...`)
  - `distro`: solo per WSL; `shell`: override opzionale
- Stato condiviso `WorkspaceStore` (in memoria + salvataggio su ogni modifica).
- File: `<app_data>/workspaces.json`. Se corrotto, backup in `.json.bak` e riparto vuoto.
- Comandi: `list_workspaces`, `add_workspace`, `rename_workspace`, `remove_workspace`.
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
- `pty_spawn(workspace_id, cols, rows)`:
  - Windows: `powershell.exe -NoLogo` (o shell custom) con cwd = cartella;
  - WSL: `wsl.exe -d <distro> --cd <path> [-e <shell>]`;
  - entrambi i rami impostano `TERM=xterm-256color` + `COLORTERM=truecolor`
    (senza questi hint molti CLI disabilitano i colori perché l'app GUI
    non ne eredita alcuno); niente `FORCE_COLOR`, per rispettare
    `NO_COLOR`/`--no-color`;
  - ricontrolla che la cartella esista al momento dello spawn.
- Un thread di lettura per sessione inoltra i chunk al frontend con l'evento
  `pty-output { session_id, data }`; alla chiusura emette `pty-exit { session_id }`.
- Decodifica UTF-8 **incrementale**: i chunk possono spezzare un carattere
  multibyte, quindi i byte incompleti restano in coda fino al chunk dopo
  (niente `�` su accenti/box-drawing).
- Comandi: `pty_spawn`, `pty_write`, `pty_resize`, `pty_close`.
  `pty_close` uccide il processo figlio; il thread di lettura emette comunque
  `pty-exit`, così frontend e backend non divergono.

### `settings.rs` — impostazioni globali

- `Settings { default_command }`: comando eseguito automaticamente a ogni
  nuovo terminale (stringa vuota = nessuno).
- Stesso pattern di `workspaces.rs`: `SettingsStore` in memoria,
  file `<app_data>/settings.json`, backup `.json.bak` se corrotto.
- Comandi: `get_settings`, `set_default_command` (sanitizza: trim, max
  1000 caratteri, come il frontend in `src/lib/settings.ts`).

### `muse_notify.rs` — segnale di fine lavoro Muse (niente semaforo)

- Il plugin Muse installato negli env dichiara hook `Stop`/`SessionEnd` il cui
  comando e' questo stesso exe: `cli-companion --notify <evento> <sorgente>`
  (ramo con early-return in `main.rs`, come `--check-update`).
- `--notify` spedisce una riga JSON su TCP `127.0.0.1:17421` all'istanza GUI;
  il listener (thread in `setup`) emette l'evento Tauri `muse-event` e il
  frontend suona il ding (Web Audio, `src/lib/notifySound.ts`).
- Da WSL l'exe gira sull'host via interop: il TCP resta locale, nessun
  networking cross-VM. Il Manifest e' generato per-env con il path exe bakato
  (`C:/...` su Windows, `/mnt/...` su WSL, mai backslash/BOM).
- Comandi: `muse_env_status`, `install_muse_plugin`, `remove_muse_plugin`,
  `test_muse_notify`. Tab "Muse" in `SettingsDialog` per installare negli env.

### `lib.rs`

Registra i plugin (`opener`, `dialog`), gli stati condivisi, il caricamento
dello store in `setup` e tutti i comandi. Capability minime in
`capabilities/default.json` (`core:default`, `opener:default`, `dialog:allow-open`).

## Frontend (`src/`)

- `lib/tauri.ts`: tipi (`Workspace`, eventi PTY) e wrapper `api` sopra `invoke`/`listen`.
- `App.tsx`: layout sidebar + area tab; stato globale minimale
  (`workspaces`, `tabs`, `activeKey`, dialog). Quando il tab attivo viene chiuso,
  seleziona l'ultimo rimasto.
- `components/Sidebar.tsx`: lista workspace con badge `WIN`/`distro`, azioni
  "+ Terminale", rinomina inline, elimina con doppia conferma (niente
  `confirm()` nativo, per robustezza nella WebView).
- `components/AddWorkspaceDialog.tsx`: modal gluestack con toggle Windows/WSL.
  Windows: picker cartella nativo (`@tauri-apps/plugin-dialog`) o path manuale.
  WSL: lista distro con caricamento async, input percorso con bottone "Verifica".
- `components/SettingsDialog.tsx`: modale impostazioni con anteprima di
  sicurezza (mostra cosa verrà eseguito, o che i terminali si apriranno vuoti).
- `lib/settings.ts`: pure functions zero-dep (sanitizza, trigger, input pty
  con `\r`, costanti di timing), coperte da `tests/settings.test.mjs`.
- `components/TerminalView.tsx`: un'istanza xterm per tab.
  Il comando di default (catturato al mount) viene iniettato via `pty_write`
  al primo output della shell + 150 ms, con fallback a 2,5 s per gli avvii
  freddi; una sola iniezione per tab, timer puliti all'unmount.
  - Tema Campbell in `src/lib/terminalTheme.ts` (stessi 16 ANSI di Windows
    Terminal); l'host di xterm è un div interno senza padding (FitAddon
    misura il contenitore: il padding esterno sposterebbe l'ultima riga
    fuori vista). Refit di recupero dopo lo spawn e a font pronti.
  - Lo spawn avviene al mount; i tab inattivi restano montati ma nascosti
    (`display: none`) per non perdere lo stato della shell.
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
