import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { Loader2, SlidersHorizontal } from "lucide-react";
import Sidebar from "./components/Sidebar";
import type { SidebarTerminal } from "./components/Sidebar";
import AddWorkspaceDialog from "./components/AddWorkspaceDialog";
import SettingsDialog from "./components/SettingsDialog";
import PluginUpdateDialog from "./components/PluginUpdateDialog";
import type { SettingsTab } from "./components/SettingsDialog";
import TitleBar from "./components/TitleBar";
import type { TitleTab } from "./components/TitleBar";
import StatusBar from "./components/StatusBar";
import Logo from "./components/Logo";
import TerminalView from "./components/TerminalView";
import { Button } from "./components/ui/button";
import {
  api,
  onAgentEvent,
  onAutomationClose,
  onAutomationSpawn,
  onAutomationUpdate,
  onNotificationActivated,
} from "./lib/tauri";
import { taskCounts } from "./lib/board";
import { useBoards, useTaskNotify } from "./lib/useBoards";
import { useTaskPanelCollapsed } from "./lib/useTaskPanelCollapsed";
import TaskPanel from "./components/tasks/TaskPanel";
import GlobalTaskPanel from "./components/tasks/GlobalTaskPanel";
import TasksManager from "./components/tasks/TasksManager";
import type { TasksManagerIntent } from "./components/tasks/TasksManager";
import type {
  AgentEnvStatus,
  AgentProfile,
  BuiltinLaunch,
  Launch,
  OutdatedPlugin,
  Workspace,
} from "./lib/tauri";
import { allLaunches, launchCommands, resolveDefaultLaunch } from "./lib/launch";
import {
  agentEnvBadge,
  applyAgentEvent,
  GROUP_STATUS,
  applyUserInput,
  countNeedingInput,
  installedAgentsSummary,
  notificationBody,
  resolveInputOverride,
  shouldSystemNotify,
  soundForEvent,
  tabAgentId,
  tabStatus,
} from "./lib/agents";
import type { AgentTabState, TabStatus } from "./lib/agents";
import { limitGroups, usageText, usageTooltip } from "./lib/usage";
import { useUsage } from "./lib/useUsage";
import {
  INTAKE_LAUNCH_ID,
  applyAutomationUpdate,
  automationTooltip,
  isAutomated,
  persistableTabs,
} from "./lib/automation";
import type { AutomationMeta } from "./lib/automation";
import {
  EMPTY_LAYOUT,
  GLOBAL_ID,
  activePaneId,
  closeTabIn,
  computePaneLayouts,
  focusTab,
  groupIndicator,
  openTab,
  openTabBackground,
  reorderTabs,
  selectGlobal,
  selectGroup,
  setActivePane,
  toggleMaximize,
  togglePin,
} from "./lib/layout";
import type { LayoutState } from "./lib/layout";
import {
  CLOSE_FLUSH_TIMEOUT_MS,
  CLAUDE_AGENT_ID,
  SESSION_SAVE_MAX_MS,
  SESSION_SAVE_MS,
  debounceDelay,
  parseSession,
  restoreCommand,
  serializeSession,
} from "./lib/session";
import type { SessionTab } from "./lib/session";
import { playAttention, playDing } from "./lib/notifySound";
import { notifyIfUnfocused } from "./lib/systemNotify";

interface Tab {
  key: string;
  workspace: Workspace;
  /** Avvio da cui è stato aperto il tab (assente = shell semplice). */
  launchId?: string;
  /** Comando dell'avvio ("" = nessuno): è quello che si salva nella sessione. */
  command: string;
  /**
   * Comando iniettato all'apertura del terminale: uguale a `command`, tranne
   * nei tab ripristinati (ripresa della conversazione dell'agente).
   */
  startCommand: string;
  /** Tab ripristinato da una sessione precedente (mostra la cronologia salvata). */
  restored?: boolean;
  title?: string;
  /**
   * Stato dell'agente (assente = tab normale con euristica busy/idle).
   * `agent.agentSessionId` serve a riprendere la sessione (`--resume`).
   */
  agent?: AgentTabState;
  /**
   * Sessione automatica (worker di un task): non si salva nella sessione ne'
   * si ripristina. Assente = tab normale.
   */
  automation?: AutomationMeta;
}

/** Etichetta mostrata per un tab (titolo OSC > nome avvio > nome workspace). */
function tabLabel(t: Tab, builtins: BuiltinLaunch[]): string {
  // Un worker mostra il titolo del task: il titolo OSC dell'agente cambia di continuo.
  if (t.automation?.title) return t.automation.title;
  const launchName = t.launchId
    ? allLaunches(t.workspace, builtins).find((l) => l.id === t.launchId)?.name
    : undefined;
  return t.title ?? launchName ?? t.workspace.name;
}

function toSessionTab(t: Tab): SessionTab {
  return {
    tabId: t.key,
    workspaceId: t.workspace.id,
    launchId: t.launchId,
    command: t.command,
    title: t.title,
    agentId: t.agent?.agentId,
    agentSessionId: t.agent?.agentSessionId,
  };
}

function newTabKey(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }
}

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general-launches");
  const [settingsWorkspaceId, setSettingsWorkspaceId] = useState<string | undefined>();
  // `tabs` resta nell'ordine di creazione ed è l'ordine di render dei
  // TerminalView (mai riordinato: smontarli ucciderebbe la sessione pty).
  // L'ordine nella griglia vive solo in `layout`.
  const [tabs, setTabs] = useState<Tab[]>([]);
  // Avviso temporaneo (es. preset Task Intake non disponibile), mostrato nella sidebar.
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [layout, setLayout] = useState<LayoutState>(EMPTY_LAYOUT);
  const [busyByKey, setBusyByKey] = useState<Record<string, boolean>>({});
  const [drag, setDrag] = useState<{ from: string | null; over: string | null }>({
    from: null,
    over: null,
  });
  // Profili di input degli agenti (statici): letti una volta all'avvio.
  const [builtinLaunches, setBuiltinLaunches] = useState<BuiltinLaunch[]>([]);
  const builtinsRef = useRef(builtinLaunches);
  builtinsRef.current = builtinLaunches;
  const [globalLaunch, setGlobalLaunch] = useState<string | null>(null);
  const [agentProfiles, setAgentProfiles] = useState<AgentProfile[]>([]);
  const [sessionReady, setSessionReady] = useState(false);
  // Salvataggio abilitato solo se la lettura della sessione e' riuscita (file
  // assente compreso): dopo un errore di I/O non si sovrascrive session.json.
  const [saveEnabled, setSaveEnabled] = useState(false);
  // Plugin degli agenti installati per ambiente (barra di stato): caricati una
  // volta, in background, dopo l'avvio; aggiornati dalle Impostazioni.
  const [agentEnvs, setAgentEnvs] = useState<AgentEnvStatus[]>([]);
  const [outdatedPlugins, setOutdatedPlugins] = useState<OutdatedPlugin[]>([]);
  const pluginCheckStarted = useRef(false);
  // Utilizzo (limiti e token): interruttore globale delle Impostazioni.
  const [showUsage, setShowUsage] = useState(true);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  // Code dei task per workspace (pannello Task, sidebar, Tasks Manager).
  const { boards, boardErrors, storeBoard, loadBoard } = useBoards(workspaces);
  useTaskNotify(workspaces);
  // Pannello Task compresso/espanso, per workspace e per la vista Globale (localStorage).
  const taskPanel = useTaskPanelCollapsed();
  // Modalità per workspace: true = si mostra il Tasks Manager al posto dei terminali.
  const [managerOpen, setManagerOpen] = useState<Record<string, boolean>>({});
  // Richiesta di apertura dal pannello Task (dettaglio di un task / "Nuovo task").
  const [managerIntent, setManagerIntent] = useState<TasksManagerIntent | null>(null);
  const intentNonceRef = useRef(0);
  // Workspace mostrato senza avere un gruppo di terminali (il layout non lo
  // conosce): azzerato appena cambia la selezione del layout.
  const [focusedWorkspaceId, setFocusedWorkspaceId] = useState<string | null>(null);

  useEffect(() => {
    api
      .agentProfiles()
      .then(setAgentProfiles)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    api
      .listWorkspaces()
      .then((ws) => {
        setWorkspaces(ws);
        setLoading(false);
      })
      .catch((e) => {
        setLoadError(String(e));
        setLoading(false);
      });
  }, []);

  // Copia sempre aggiornata dei tab per il listener degli eventi (registrato
  // una volta sola).
  const tabsRef = useRef<Tab[]>([]);
  tabsRef.current = tabs;
  const workspacesRef = useRef<Workspace[]>([]);
  workspacesRef.current = workspaces;

  // Gli archiviati non compaiono in nessun elenco (restano solo nelle impostazioni).
  const visibleWorkspaces = useMemo(() => workspaces.filter((w) => !w.archived), [workspaces]);

  const paneLayouts = useMemo(() => computePaneLayouts(layout), [layout]);
  const paneLayoutsRef = useRef(paneLayouts);
  paneLayoutsRef.current = paneLayouts;

  // Ripristino della sessione precedente, una volta sola dopo il caricamento
  // dei workspace. Stessi tabId di prima: così combaciano lo scrollback
  // salvato e CLI_COMPANION_SESSION.
  const restoredRef = useRef(false);
  useEffect(() => {
    if (loading || loadError || restoredRef.current) return;
    restoredRef.current = true;
    void (async () => {
      let raw: string | null = null;
      let loaded = true;
      try {
        raw = await api.loadSession();
      } catch (e) {
        console.error("Lettura della sessione fallita, salvataggio disattivato:", e);
        loaded = false;
      }
      const data = parseSession(
        raw,
        workspaces.filter((w) => !w.archived).map((w) => w.id),
      );
      if (data) {
        const byId = new Map(workspaces.map((w) => [w.id, w]));
        setTabs(
          data.tabs.map(
            (t): Tab => ({
              key: t.tabId,
              workspace: byId.get(t.workspaceId)!,
              launchId: t.launchId,
              command: t.command,
              startCommand: restoreCommand(t),
              restored: true,
              title: t.title,
              agent: t.agentSessionId
                ? { phase: "idle", agentSessionId: t.agentSessionId, agentId: t.agentId }
                : undefined,
            }),
          ),
        );
        setLayout({
          groups: data.groups,
          activeWorkspaceId: data.activeWorkspaceId,
          global: data.global,
          activeView: data.activeView,
        });
      }
      // Via lo scrollback dei tab che non esistono più (solo se la lettura è riuscita).
      if (loaded) {
        void api
          .pruneScrollback(data ? data.tabs.map((t) => t.tabId) : [])
          .catch(() => undefined);
      }
      setSaveEnabled(loaded);
      setSessionReady(true);
    })();
  }, [loading, loadError, workspaces]);

  // Salvataggio della sessione (debounce) a ogni modifica rilevante.
  // La chiave di confronto NON include i titoli (cambiano di continuo con lo
  // spinner degli agenti): da sola una variazione di titolo non fa salvare.
  // Il JSON scritto su disco viene invece costruito al momento del salvataggio
  // (anche in chiusura) con i titoli piu' recenti.
  const buildSession = (withTitles: boolean) =>
    serializeSession({
      activeWorkspaceId: layout.activeWorkspaceId,
      activeView: layout.activeView,
      groups: layout.groups,
      global: layout.global,
      tabs: persistableTabs(tabs).map((t) => {
        const st = toSessionTab(t);
        return withTitles ? st : { ...st, title: undefined };
      }),
    });
  const sessionJson = useMemo(() => buildSession(false), [tabs, layout]);
  const buildSessionRef = useRef(buildSession);
  buildSessionRef.current = buildSession;
  const savedJsonRef = useRef<string | null>(null);
  // Debounce con attesa massima: le modifiche frequenti (titoli OSC dello
  // spinner) non rimandano il salvataggio oltre SESSION_SAVE_MAX_MS.
  const pendingSinceRef = useRef<number | null>(null);
  useEffect(() => {
    if (!saveEnabled || sessionJson === savedJsonRef.current) {
      pendingSinceRef.current = null;
      return;
    }
    const now = Date.now();
    if (pendingSinceRef.current === null) pendingSinceRef.current = now;
    const delay = debounceDelay(
      now,
      pendingSinceRef.current,
      SESSION_SAVE_MS,
      SESSION_SAVE_MAX_MS,
    );
    const id = setTimeout(() => {
      pendingSinceRef.current = null;
      savedJsonRef.current = sessionJson;
      void api.saveSession(buildSessionRef.current(true)).catch(() => {
        savedJsonRef.current = null;
      });
    }, delay);
    return () => clearTimeout(id);
  }, [sessionJson, saveEnabled]);

  // Salvataggi dello scrollback registrati dai TerminalView.
  const flushersRef = useRef(new Map<string, () => Promise<void>>());
  const registerFlush = useCallback(
    (tabId: string, flush: (() => Promise<void>) | null) => {
      if (flush) flushersRef.current.set(tabId, flush);
      else flushersRef.current.delete(tabId);
    },
    [],
  );

  // Alla chiusura della finestra: salva sessione e scrollback, con un tetto di
  // tempo così la chiusura non resta mai appesa. Senza preventDefault Tauri
  // chiude la finestra appena l'handler termina.
  const closingRef = useRef(false);
  const saveEnabledRef = useRef(false);
  saveEnabledRef.current = saveEnabled;
  // Usato anche prima di un riavvio (`relaunch` non passa da onCloseRequested).
  const flushBeforeExit = useCallback(async () => {
    closingRef.current = true;
    const work = Promise.all([
      saveEnabledRef.current
        ? api.saveSession(buildSessionRef.current(true))
        : Promise.resolve(),
      ...[...flushersRef.current.values()].map((f) => f()),
    ]).catch(() => undefined);
    await Promise.race([
      work,
      new Promise((resolve) => setTimeout(resolve, CLOSE_FLUSH_TIMEOUT_MS)),
    ]);
  }, []);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void getCurrentWindow()
        .onCloseRequested(flushBeforeExit)
        .then((u) => {
          if (cancelled) u();
          else unlisten = u;
        })
        .catch(() => undefined);
    } catch {
      // fuori da Tauri (anteprima nel browser): nessun hook di chiusura
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Workspace mostrato e modalità: il Tasks Manager copre l'area del pannello
  // Task e dei terminali di quel workspace (mai nella vista Globale).
  const viewWorkspaceId =
    focusedWorkspaceId ?? (layout.activeView === "global" ? null : layout.activeWorkspaceId);
  const managerShown = viewWorkspaceId !== null && managerOpen[viewWorkspaceId] === true;
  const modalOpenRef = useRef(false);
  modalOpenRef.current = settingsOpen || dialogOpen;
  const managerOpenRef = useRef(managerOpen);
  managerOpenRef.current = managerOpen;
  const viewWorkspaceIdRef = useRef<string | null>(null);
  viewWorkspaceIdRef.current = viewWorkspaceId;
  const layoutRef = useRef(layout);
  layoutRef.current = layout;

  // Il layout cambia selezione (gruppo, vista globale): il focus "senza gruppo" decade.
  useEffect(() => {
    setFocusedWorkspaceId(null);
  }, [layout.activeWorkspaceId, layout.activeView]);

  // Un terminale messo a fuoco riporta ai terminali del suo workspace: il
  // Tasks Manager di quel workspace si chiude e il focus "senza gruppo" decade.
  const showTerminalsOf = useCallback((owner: string | undefined) => {
    if (owner) setManagerOpen((prev) => (prev[owner] ? { ...prev, [owner]: false } : prev));
    setFocusedWorkspaceId(null);
  }, []);

  // Apre il Tasks Manager di un workspace (lo seleziona se non è quello
  // mostrato), con un'eventuale richiesta (dettaglio di un task, "Nuovo task").
  // Solo stato: i terminali restano montati.
  const openManager = useCallback(
    (id: string, intent?: Omit<TasksManagerIntent, "nonce">) => {
      setManagerIntent(intent ? { ...intent, nonce: ++intentNonceRef.current } : null);
      if (viewWorkspaceIdRef.current !== id) {
        if (layoutRef.current.groups.some((g) => g.workspaceId === id)) {
          setFocusedWorkspaceId(null);
          setLayout((l) => selectGroup(l, id));
        } else {
          setFocusedWorkspaceId(id);
        }
      }
      loadBoard(id);
      setManagerOpen((prev) => ({ ...prev, [id]: true }));
    },
    [loadBoard],
  );

  // Sidebar e Ctrl+Maiusc+B: apre il manager, o lo chiude se è già aperto.
  const toggleManager = useCallback(
    (id: string) => {
      if (viewWorkspaceIdRef.current === id && managerOpenRef.current[id] === true) {
        setManagerOpen((prev) => ({ ...prev, [id]: false }));
        return;
      }
      openManager(id);
    },
    [openManager],
  );
  const toggleManagerRef = useRef(toggleManager);
  toggleManagerRef.current = toggleManager;

  // La richiesta di apertura vale una volta: il manager l'ha letta al montaggio
  // (gli effect dei figli girano prima), poi si azzera, così una riapertura
  // senza richiesta non riseleziona un task vecchio o riapre "Nuovo task".
  useEffect(() => {
    if (managerShown && managerIntent) setManagerIntent(null);
  }, [managerShown, managerIntent]);

  // Il manager copre i terminali: nessuno deve tenere il focus della tastiera.
  useEffect(() => {
    if (!managerShown) return;
    const el = document.activeElement;
    if (el instanceof HTMLElement && el.closest(".xterm")) el.blur();
  }, [managerShown]);

  // Un tab messo a fuoco da pannello Task, Tasks Manager o notifica: torna ai
  // terminali del suo workspace (la chiave del tab è l'id di sessione del worker).
  const focusTerminal = useCallback(
    (key: string) => {
      // Tab chiuso nel frattempo: niente da mettere a fuoco, la vista resta com'è.
      const tab = tabsRef.current.find((t) => t.key === key);
      if (!tab) return;
      showTerminalsOf(tab.workspace.id);
      setLayout((l) => focusTab(l, key));
    },
    [showTerminalsOf],
  );

  // Ctrl+Maiusc+B: apre/chiude il Tasks Manager del workspace attivo (non nella
  // vista Globale). In fase di cattura, come Ctrl+Maiusc+M.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey) return;
      if (e.key.toLowerCase() !== "b") return;
      // Con un dialogo aperto la scorciatoia non fa nulla.
      if (modalOpenRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      const id = viewWorkspaceIdRef.current;
      if (id) toggleManagerRef.current(id);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Ctrl+Maiusc+M: massimizza/ripristina il pannello attivo della vista mostrata.
  // In fase di cattura, così xterm non vede il tasto. Niente Esc: serve alle CLI.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey) return;
      if (e.key.toLowerCase() !== "m") return;
      e.preventDefault();
      e.stopPropagation();
      setLayout((l) => {
        const id = activePaneId(l);
        return id ? toggleMaximize(l, id) : l;
      });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Polling dell'utilizzo: limiti per ambiente e token per sessione.
  const usage = useUsage(showUsage, agentEnvs, tabs);
  const usageEventRef = useRef(usage.onAgentEvent);
  usageEventRef.current = usage.onAgentEvent;

  // Eventi dei coding agent: stato del tab, suono e notifica di sistema.
  // `test` e' il self-test delle Impostazioni: solo il suono. Gli eventi il
  // cui tab non esiste (piu') sono ignorati.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onAgentEvent((ev) => {
      if (ev.event === "test") {
        playDing();
        return;
      }
      const tab = tabsRef.current.find((t) => t.key === ev.tab_id);
      if (!tab) return;
      setTabs((prev) =>
        prev.map((t) =>
          t.key === tab.key ? { ...t, agent: applyAgentEvent(t.agent, ev) } : t,
        ),
      );
      usageEventRef.current(tab.key, ev.event, ev.transcript_path, ev.agent);
      const sound = soundForEvent(ev.event);
      if (sound === "attention") playAttention();
      else if (sound === "ding") playDing();
      // I worker dei task notificano solo via `task-notify` (attenzione, fallito).
      if (shouldSystemNotify(ev.event) && !tab.automation) {
        void notifyIfUnfocused(
          tab.key,
          tab.workspace.name,
          notificationBody(ev.event, tabLabel(tab, builtinsRef.current), ev.message),
        );
      }
    })
      .then((u) => {
        // Cleanup gia' eseguito (StrictMode): niente listener orfano.
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Click su una notifica di sistema: la finestra e' gia' in primo piano
  // (backend); qui si seleziona workspace e pannello. Solo stato del layout:
  // i TerminalView non si smontano ne' cambiano genitore. Tab chiuso: niente.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onNotificationActivated((tabId) => focusTerminal(tabId))
      .then((u) => {
        if (cancelled) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [focusTerminal]);

  // Senza `launch` parte l'avvio predefinito del workspace, se esiste;
  // altrimenti una shell semplice. Il nuovo terminale attiva il suo gruppo.
  const spawnTab = useCallback((ws: Workspace, launch?: Launch | BuiltinLaunch) => {
    const chosen = launch ?? resolveDefaultLaunch(ws, builtinLaunches, globalLaunch);
    const key = newTabKey();
    // Per gli integrati si salva il comando di ripristino, si esegue quello di avvio.
    const { command, startCommand } = launchCommands(chosen);
    showTerminalsOf(ws.id);
    setTabs((prev) => [
      ...prev,
      { key, workspace: ws, launchId: chosen?.id, command, startCommand },
    ]);
    setLayout((l) => openTab(l, ws.id, key));
  }, [showTerminalsOf, builtinLaunches, globalLaunch]);

  // Avviso temporaneo nella sidebar.
  const showNotice = useCallback((text: string) => {
    setNotice(text);
    clearTimeout(noticeTimerRef.current);
    noticeTimerRef.current = setTimeout(() => setNotice(null), 6000);
  }, []);

  // Task Intake: ogni richiesta apre un nuovo tab del preset nel workspace
  // (più chat in parallelo), con la stessa via delle voci rapide della sidebar.
  // Senza il preset tra gli avvii integrati (backend non aggiornato) avvisa.
  const openIntake = useCallback(
    (workspaceId: string) => {
      const ws = workspacesRef.current.find((w) => w.id === workspaceId);
      if (!ws) return;
      const launch = builtinsRef.current.find((b) => b.id === INTAKE_LAUNCH_ID);
      if (!launch) {
        showNotice("Task Intake non è disponibile: aggiorna il plugin dei coding agent");
        return;
      }
      spawnTab(ws, launch);
    },
    [showNotice, spawnTab],
  );

  // Controllo silenzioso all'avvio (plugin updater): se c'e' un update compare
  // il pallino su "Impostazioni". Gli errori (rete, release assente) non fanno rumore.
  useEffect(() => {
    void (async () => {
      try {
        setUpdateAvailable((await check()) !== null);
      } catch {
        /* silenzioso */
      }
    })();
  }, []);

  // Avvii integrati e avvio predefinito globale: una volta, errori silenziosi.
  useEffect(() => {
    api.builtinLaunches().then(setBuiltinLaunches).catch(() => undefined);
    api
      .getLaunchSettings()
      .then((s) => {
        setGlobalLaunch(s.default_launch);
        setShowUsage(s.show_usage !== false);
      })
      .catch(() => undefined);
  }, []);

  // Versione dell'app (footer della sidebar).
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => undefined);
  }, []);

  // `agent_env_status` lancia processi (wsl.exe, claude): una sola volta, in
  // background e con un po' di ritardo, quando la sessione e' gia' ripristinata.
  // Non blocca la UI; dopo install/rimozione lo aggiornano le Impostazioni.
  useEffect(() => {
    if (!sessionReady) return;
    let cancelled = false;
    const id = setTimeout(() => {
      api
        .agentEnvStatus()
        .then((envs) => {
          if (!cancelled) setAgentEnvs(envs);
        })
        .catch(() => undefined);
    }, 1500);
    return () => {
      cancelled = true;
      clearTimeout(id);
    };
  }, [sessionReady]);

  // Plugin installati con versione diversa dall'app: controllo una sola volta
  // per avvio (il ref evita il doppio effect di StrictMode); errori ignorati.
  useEffect(() => {
    if (!sessionReady || pluginCheckStarted.current) return;
    pluginCheckStarted.current = true;
    // Niente cleanup del timer: col ref, il rerun StrictMode non lo riarmerebbe.
    setTimeout(() => {
      api
        .agentPluginsOutdated()
        .then((list) => {
          if (list.length > 0) setOutdatedPlugins(list);
        })
        .catch((e) => console.error("agent_plugins_outdated", e));
    }, 1500);
  }, [sessionReady]);

  const closePluginUpdate = useCallback(() => {
    setOutdatedPlugins([]);
    // Dopo un aggiornamento non seguito da riavvio lo stato va riletto.
    api
      .agentEnvStatus()
      .then(setAgentEnvs)
      .catch(() => undefined);
  }, []);

  const openLaunchSettings = useCallback((ws: Workspace) => {
    setSettingsTab("launches");
    setSettingsWorkspaceId(ws.id);
    setSettingsOpen(true);
  }, []);

  // Chiusura di un terminale (dall'utente o perché la shell è uscita): la sua
  // cronologia salvata non serve più.
  // Un tab automatico chiuso va comunicato al backend una volta sola, da
  // qualunque via arrivi la chiusura (utente, `automation-close`, shell uscita).
  const reportedClosedRef = useRef(new Set<string>());
  const closeTab = useCallback((key: string) => {
    const closing = tabsRef.current.find((t) => t.key === key);
    const automated = closing !== undefined && isAutomated(closing);
    if (automated && !reportedClosedRef.current.has(key)) {
      reportedClosedRef.current.add(key);
      void api.automationTabClosed(key).catch(() => undefined);
    }
    setTabs((prev) => prev.filter((t) => t.key !== key));
    setLayout((l) => closeTabIn(l, key));
    setBusyByKey((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
    // Gli automatici non salvano lo scrollback: niente da cancellare.
    if (!automated) void api.deleteScrollback(key).catch(() => undefined);
  }, []);

  // La shell è uscita da sola. Durante la chiusura dell'app i pty vengono
  // chiusi: non è un'uscita dell'utente, la sessione va conservata.
  const handleExit = useCallback(
    (key: string) => {
      if (closingRef.current) return;
      closeTab(key);
    },
    [closeTab],
  );

  const closeTabRef = useRef(closeTab);
  closeTabRef.current = closeTab;

  // Sessioni automatiche: il backend chiede di aprire/chiudere i tab dei worker
  // e ne comunica gli stati. Registrato una volta sola (stato letto dai ref).
  useEffect(() => {
    const unlistens: Array<() => void> = [];
    let cancelled = false;
    const keep = (p: Promise<() => void>) =>
      p
        .then((u) => {
          if (cancelled) u();
          else unlistens.push(u);
        })
        .catch(() => undefined);

    keep(
      onAutomationSpawn((ev) => {
        // Workspace sconosciuto (rimosso nel frattempo) o tab gia' aperto: ignora.
        const ws = workspacesRef.current.find((w) => w.id === ev.workspace_id);
        if (!ws || tabsRef.current.some((t) => t.key === ev.tab_id)) return;
        const tab: Tab = {
          key: ev.tab_id,
          workspace: ws,
          command: "",
          startCommand: ev.command,
          automation: {
            title: ev.title,
            status: "starting",
            agentId: ev.agent_id,
            cwd: ev.cwd ?? undefined,
          },
        };
        // Aggiornamento sincrono del ref: un secondo evento identico non riapre il tab.
        tabsRef.current = [...tabsRef.current, tab];
        setTabs((prev) => (prev.some((t) => t.key === tab.key) ? prev : [...prev, tab]));
        // In secondo piano: non toglie all'utente la vista in cui sta lavorando.
        setLayout((l) => openTabBackground(l, ws.id, tab.key));
      }),
    );

    keep(onAutomationClose((ev) => closeTabRef.current(ev.tab_id)));

    keep(
      onAutomationUpdate((ev) =>
        setTabs((prev) =>
          prev.map((t) =>
            t.key === ev.tab_id && t.automation
              ? { ...t, automation: applyAutomationUpdate(t.automation, ev) }
              : t,
          ),
        ),
      ),
    );

    // Riallinea i metadati dei tab gia' aperti (es. dopo un ricaricamento).
    api
      .automationSessions()
      .then((infos) => {
        if (cancelled) return;
        const byKey = new Map(infos.map((i) => [i.tab_id, i]));
        setTabs((prev) =>
          prev.map((t) => {
            const info = byKey.get(t.key);
            return info && t.automation
              ? { ...t, automation: applyAutomationUpdate(t.automation, info) }
              : t;
          }),
        );
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, []);

  const handleTitle = useCallback((key: string, title: string) => {
    setTabs((prev) => {
      const i = prev.findIndex((t) => t.key === key);
      if (i < 0 || prev[i].title === title) return prev;
      const copy = prev.slice();
      copy[i] = { ...prev[i], title };
      return copy;
    });
  }, []);

  // Input dell'utente: Invio => l'agente lavora, altro input toglie ✓/❗.
  const handleInput = useCallback((key: string, enter: boolean) => {
    setTabs((prev) => {
      const i = prev.findIndex((t) => t.key === key);
      if (i < 0) return prev;
      const next = applyUserInput(prev[i].agent, enter);
      if (next === prev[i].agent) return prev;
      const copy = prev.slice();
      copy[i] = { ...prev[i], agent: next };
      return copy;
    });
  }, []);

  const handleStatus = useCallback((key: string, busy: boolean) => {
    setBusyByKey((prev) => (prev[key] === busy ? prev : { ...prev, [key]: busy }));
  }, []);

  // Trascinamento delle intestazioni: l'origine sta in un ref (letta anche
  // negli handler), lo stato `drag` serve solo all'evidenziazione.
  const dragFromRef = useRef<string | null>(null);

  const handlePaneDragStart = useCallback((id: string) => {
    dragFromRef.current = id;
    setDrag({ from: id, over: null });
  }, []);

  const handleActivate = useCallback(
    (key: string) => setLayout((l) => setActivePane(l, key)),
    [],
  );
  const handleToggleMaximize = useCallback(
    (key: string) => setLayout((l) => toggleMaximize(l, key)),
    [],
  );
  const handleTogglePin = useCallback((key: string) => setLayout((l) => togglePin(l, key)), []);
  const handlePaneDragLeave = useCallback(
    (key: string) => setDrag((d) => (d.over === key ? { ...d, over: null } : d)),
    [],
  );

  const handlePaneDragEnd = useCallback(() => {
    dragFromRef.current = null;
    setDrag({ from: null, over: null });
  }, []);

  const handlePaneDragOver = useCallback((id: string) => {
    const from = dragFromRef.current;
    const pl = paneLayoutsRef.current;
    const ok =
      from !== null &&
      from !== id &&
      pl[from]?.groupWorkspaceId === pl[id]?.groupWorkspaceId;
    const over = ok ? id : null;
    setDrag((d) => (d.over === over ? d : { ...d, over }));
  }, []);

  const handlePaneDrop = useCallback(
    (id: string) => {
      const from = dragFromRef.current;
      if (from && from !== id) setLayout((l) => reorderTabs(l, from, id));
      handlePaneDragEnd();
    },
    [handlePaneDragEnd],
  );

  const handleAdded = useCallback((ws: Workspace) => {
    setWorkspaces((prev) => [...prev, ws]);
  }, []);

  // Aggiorna lo stato con il Workspace restituito dal backend (rinomina,
  // modifica degli avvii) anche nei tab già aperti.
  const handleWorkspaceUpdated = useCallback((updated: Workspace) => {
    setWorkspaces((prev) => prev.map((w) => (w.id === updated.id ? updated : w)));
    setTabs((prev) =>
      prev.map((t) =>
        t.workspace.id === updated.id ? { ...t, workspace: updated } : t,
      ),
    );
  }, []);

  const handleRename = useCallback(
    async (id: string, name: string) => {
      handleWorkspaceUpdated(await api.renameWorkspace(id, name));
    },
    [handleWorkspaceUpdated],
  );

  const handleRemove = useCallback(
    async (id: string) => {
      await api.removeWorkspace(id);
      // Stessa via della chiusura da parte dell'utente: pty (smontaggio del
      // TerminalView), scrollback, gruppo e selezione di un altro gruppo.
      for (const t of tabsRef.current) {
        if (t.workspace.id === id) closeTab(t.key);
      }
      setWorkspaces((prev) => prev.filter((w) => w.id !== id));
    },
    [closeTab],
  );

  const handleSelectWorkspace = useCallback((id: string) => {
    // Un workspace senza terminali non ha un gruppo: lo si "mostra" comunque
    // (con la sua modalità, utile per aprirne il Tasks Manager).
    const hasGroup = layoutRef.current.groups.some((g) => g.workspaceId === id);
    setFocusedWorkspaceId(hasGroup ? null : id);
    setLayout((l) => selectGroup(l, id));
  }, []);

  const handleReorder = useCallback(async (ids: string[]) => {
    setWorkspaces(await api.reorderWorkspaces(ids));
  }, []);

  const handleArchive = useCallback(
    async (id: string) => {
      // Prima il backend (la coda si ferma), poi la chiusura dei terminali.
      handleWorkspaceUpdated(await api.setWorkspaceArchived(id, true));
      for (const t of tabsRef.current) {
        if (t.workspace.id === id) closeTab(t.key);
      }
      if (viewWorkspaceIdRef.current === id || layoutRef.current.activeWorkspaceId === id) {
        const others = workspacesRef.current.filter((w) => !w.archived && w.id !== id);
        const next =
          others.find((w) => layoutRef.current.groups.some((g) => g.workspaceId === w.id)) ??
          others[0];
        setFocusedWorkspaceId(null);
        if (next) handleSelectWorkspace(next.id);
      }
    },
    [closeTab, handleWorkspaceUpdated, handleSelectWorkspace],
  );

  const globalActive = layout.activeView === "global";
  const globalCount = layout.global.order.length;

  const statusOf = (t: Tab): TabStatus => tabStatus(t.agent, busyByKey[t.key] === true);
  const statuses = tabs.map(statusOf);
  const statusByKey = new Map(tabs.map((t, i) => [t.key, statuses[i]]));
  const indicatorStatus = (keys: string[]): TabStatus | null => {
    const ind = groupIndicator(
      keys.flatMap((k) => {
        const s = statusByKey.get(k);
        return s ? [s] : [];
      }),
    );
    return ind === "none" ? null : GROUP_STATUS[ind];
  };

  // Tab della barra del titolo: vista globale (se ha tab) + un tab per gruppo.
  const globalTab: TitleTab | null =
    globalCount > 0
      ? {
          id: GLOBAL_ID,
          name: "Globale",
          count: globalCount,
          status: indicatorStatus(layout.global.order),
        }
      : null;
  const groupTabs: TitleTab[] = layout.groups.map((g) => ({
    id: g.workspaceId,
    name:
      tabs.find((t) => t.workspace.id === g.workspaceId)?.workspace.name ?? g.workspaceId,
    count: g.order.length,
    status: indicatorStatus(g.order),
  }));

  // Terminali di ogni workspace per la sidebar (ordine della griglia).
  const tabByKey = new Map(tabs.map((t) => [t.key, t]));
  const sidebarTerminals: Record<string, SidebarTerminal[]> = {};
  for (const g of layout.groups) {
    sidebarTerminals[g.workspaceId] = g.order.flatMap((key) => {
      const t = tabByKey.get(key);
      if (!t) return [];
      return [
        {
          key,
          label: tabLabel(t, builtinLaunches),
          status: statusByKey.get(key)!,
          pinned: layout.global.order.includes(key),
          active: g.activeTabId === key,
        },
      ];
    });
  }

  // Contatori dei task per la sidebar (in corso · in coda).
  const sidebarTaskCounts = useMemo(
    () => Object.fromEntries(Object.entries(boards).map(([id, b]) => [id, taskCounts(b)])),
    [boards],
  );
  // Workspace del pannello Task: quello mostrato (anche senza terminali).
  const panelWorkspace = workspaces.find((w) => w.id === viewWorkspaceId && !w.archived);
  const managerWorkspace = managerShown ? panelWorkspace : undefined;

  const attentionCount = countNeedingInput(statuses);
  const agentsSummary = installedAgentsSummary(agentEnvs);
  const limitEntries = showUsage
    ? limitGroups(
        agentEnvs
          .filter((e) => e.agent_id === CLAUDE_AGENT_ID && e.found && e.installed)
          .map((e) => ({
            envKey: e.env_key,
            label: agentEnvBadge(e),
            reply: usage.limits[e.env_key],
          })),
      )
    : [];
  const anyPaneVisible = Object.values(paneLayouts).some((p) => p.visible);

  return (
    <div className="flex h-screen flex-col">
      <TitleBar
        globalTab={globalTab}
        globalActive={globalActive}
        groups={groupTabs}
        activeWorkspaceId={layout.activeWorkspaceId}
        onSelectGlobal={() => setLayout(selectGlobal)}
        onSelectGroup={handleSelectWorkspace}
      />

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[260px] flex-shrink-0 flex-col overflow-hidden border-r bg-base">
          {loadError ? (
            <div className="p-4">
              <p className="text-[13px] text-destructive">{loadError}</p>
            </div>
          ) : loading || !sessionReady ? (
            <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              Caricamento…
            </div>
          ) : (
            <Sidebar
              workspaces={visibleWorkspaces}
              activeWorkspaceId={viewWorkspaceId}
              terminals={sidebarTerminals}
              taskCounts={sidebarTaskCounts}
              managerShownId={managerShown ? viewWorkspaceId : null}
              onToggleManager={toggleManager}
              onSelect={handleSelectWorkspace}
              // Un terminale scelto dalla sidebar riporta ai terminali del suo workspace.
              onFocusTerminal={focusTerminal}
              builtinLaunches={builtinLaunches}
              globalLaunch={globalLaunch}
              onSpawn={spawnTab}
              notice={notice}
              onManageLaunches={openLaunchSettings}
              onRename={handleRename}
              onRemove={handleRemove}
              onArchive={handleArchive}
              onReorder={handleReorder}
              onAdd={() => setDialogOpen(true)}
            />
          )}
          <div className="flex items-center gap-2 border-t px-3 py-2.5">
            <Button
              variant="ghost"
              onClick={() => {
                setSettingsTab(updateAvailable ? "updates" : "general-launches");
                setSettingsWorkspaceId(undefined);
                setSettingsOpen(true);
              }}
              className="h-auto flex-1 justify-start gap-2 px-2 py-[7px] text-[13px] font-normal text-text-secondary hover:text-foreground"
            >
              <SlidersHorizontal className="!size-[15px]" />
              Impostazioni
              {updateAvailable && (
                <span
                  className="h-1.5 w-1.5 rounded-full bg-primary"
                  title="Aggiornamento disponibile"
                  role="img"
                  aria-label="Aggiornamento disponibile"
                />
              )}
            </Button>
            {appVersion && (
              <span className="font-mono text-[11px] text-faint">v{appVersion}</span>
            )}
          </div>
        </aside>

        <div className="relative flex min-w-0 flex-1">
          {/* Pannello Task (a sinistra della griglia): del workspace mostrato, o
              della vista Globale. Il contenitore resta sempre al suo posto, prima
              dell'area dei pannelli: cambiare pannello non tocca i terminali. */}
          {/* Il pannello è una scheda arrotondata nello sfondo dell'area terminali:
              stesso margine esterno dei pannelli (6px), e 8px di distanza dalla
              griglia (2px qui + 6px di margine della griglia). */}
          <div
            className={`flex flex-shrink-0 ${
              panelWorkspace || globalActive ? "bg-pane-area py-[6px] pl-[6px] pr-[2px]" : ""
            }`}
            aria-hidden={managerShown || undefined}
            ref={(el) => {
              // Coperto dal Tasks Manager: non raggiungibile con Tab (solo attributo).
              if (!el) return;
              if (managerShown) el.setAttribute("inert", "");
              else el.removeAttribute("inert");
            }}
          >
            {panelWorkspace ? (
              <TaskPanel
                key={panelWorkspace.id}
                workspace={panelWorkspace}
                board={boards[panelWorkspace.id] ?? null}
                loadError={boardErrors[panelWorkspace.id] ?? null}
                collapsed={taskPanel.isCollapsed(panelWorkspace.id)}
                onToggleCollapsed={() => taskPanel.toggle(panelWorkspace.id)}
                onBoard={storeBoard}
                onWorkspaceUpdated={handleWorkspaceUpdated}
                onOpenManager={(taskId) => openManager(panelWorkspace.id, taskId ? { taskId } : undefined)}
                onOpenIntake={() => openIntake(panelWorkspace.id)}
                onNewTask={() => openManager(panelWorkspace.id, { newTask: true })}
                onFocusTab={focusTerminal}
              />
            ) : globalActive ? (
              <GlobalTaskPanel
                workspaces={visibleWorkspaces}
                boards={boards}
                onOpenWorkspace={handleSelectWorkspace}
                onFocusTab={focusTerminal}
                collapsed={taskPanel.isCollapsed(GLOBAL_ID)}
                onToggleCollapsed={() => taskPanel.toggle(GLOBAL_ID)}
              />
            ) : null}
          </div>
          {/* Area dei pannelli. TUTTI i TerminalView di TUTTI i gruppi sono figli
              diretti di questo contenitore, con chiave stabile e nell'ordine di
              creazione: layout, riordino, cambio gruppo e maximize cambiano solo
              lo stile (rettangolo / display), mai la struttura del DOM, quindi
              nessun terminale viene smontato o spostato (ucciderebbe il pty).
              Il contenitore interno e' fisso (inset 3px + 3px per pannello =
              6px di margine e di spazio fra i pannelli). */}
          <div className="relative min-h-0 min-w-0 flex-1 bg-pane-area">
            <div
              className="absolute inset-[3px]"
              aria-hidden={managerShown || undefined}
              ref={(el) => {
                // `inert` solo come attributo (niente remount): i terminali
                // nascosti dal Tasks Manager non restano raggiungibili con Tab.
                if (!el) return;
                if (managerShown) el.setAttribute("inert", "");
                else el.removeAttribute("inert");
              }}
            >
              {!anyPaneVisible && (
                <div className="absolute inset-[3px] flex flex-col items-center justify-center gap-3 rounded-[10px] border bg-pane-area p-5">
                  <Logo size={40} />
                  <span className="text-[13px] font-medium">Nessun terminale aperto</span>
                  <span className="text-center text-xs text-muted-foreground">
                    Apri un terminale da un workspace, o con un avvio dal menu ⋯
                  </span>
                </div>
              )}
              {tabs.map((t) => {
                const pl = paneLayouts[t.key];
                return (
                  <TerminalView
                    key={t.key}
                    tabId={t.key}
                    workspace={t.workspace}
                    initialCommand={t.startCommand}
                    imagePasteSeq={resolveInputOverride(
                      agentProfiles,
                      tabAgentId(t),
                      t.workspace.kind,
                      "image-paste",
                    )}
                    restore={t.restored}
                    rect={pl?.rect ?? null}
                    active={pl?.visible ?? false}
                    focused={(pl?.focused ?? false) && !managerShown}
                    label={tabLabel(t, builtinLaunches)}
                    labelPrefix={
                      pl?.groupWorkspaceId === GLOBAL_ID ? t.workspace.name : undefined
                    }
                    status={statusOf(t)}
                    cwd={t.automation?.cwd}
                    launchId={t.launchId}
                    usage={
                      showUsage && usage.sessions[t.key] && !t.agent?.ended
                        ? {
                            text: usageText(usage.sessions[t.key]),
                            title: usageTooltip(usage.sessions[t.key]),
                          }
                        : undefined
                    }
                    automation={
                      t.automation
                        ? {
                            status: t.automation.status,
                            tooltip: automationTooltip(t.automation),
                          }
                        : undefined
                    }
                    maximized={pl?.maximized ?? false}
                    pinned={pl?.pinned ?? false}
                    dropTarget={drag.over === t.key}
                    onActivate={handleActivate}
                    onClose={closeTab}
                    onToggleMaximize={handleToggleMaximize}
                    onTogglePin={handleTogglePin}
                    onPaneDragStart={handlePaneDragStart}
                    onPaneDragOver={handlePaneDragOver}
                    onPaneDragLeave={handlePaneDragLeave}
                    onPaneDrop={handlePaneDrop}
                    onPaneDragEnd={handlePaneDragEnd}
                    registerFlush={registerFlush}
                    onExit={handleExit}
                    onTitle={handleTitle}
                    onStatus={handleStatus}
                    onInput={handleInput}
                  />
                );
              })}
            </div>
          </div>
          {/* Il Tasks Manager copre pannello Task e area dei pannelli senza
              toccarli: i TerminalView qui sopra restano montati e al loro posto. */}
          {managerWorkspace && (
            <div className="absolute inset-[3px] z-20 overflow-hidden rounded-[10px] border">
              <TasksManager
                key={managerWorkspace.id}
                workspace={managerWorkspace}
                board={boards[managerWorkspace.id] ?? null}
                loadError={boardErrors[managerWorkspace.id] ?? null}
                onBoard={storeBoard}
                onWorkspaceUpdated={handleWorkspaceUpdated}
                onBackToTerminals={() => toggleManager(managerWorkspace.id)}
                onOpenTerminal={focusTerminal}
                onOpenIntake={() => openIntake(managerWorkspace.id)}
                intent={managerIntent}
                isTabOpen={(id) => tabs.some((t) => t.key === id)}
              />
            </div>
          )}
        </div>
      </div>

      <StatusBar
        terminalCount={tabs.length}
        attentionCount={attentionCount}
        agentsSummary={agentsSummary}
        limits={limitEntries}
      />

      <AddWorkspaceDialog
        isOpen={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onAdded={handleAdded}
      />
      <PluginUpdateDialog
        outdated={outdatedPlugins}
        onClose={closePluginUpdate}
        onBeforeRestart={flushBeforeExit}
      />
      <SettingsDialog
        isOpen={settingsOpen}
        initialTab={settingsTab}
        initialWorkspaceId={settingsWorkspaceId}
        workspaces={workspaces}
        onClose={() => setSettingsOpen(false)}
        onWorkspaceUpdated={handleWorkspaceUpdated}
        onRemoveWorkspace={handleRemove}
        onUpdateAvailable={setUpdateAvailable}
        updateAvailable={updateAvailable}
        onAgentEnvs={setAgentEnvs}
        onShowUsage={setShowUsage}
        builtinLaunches={builtinLaunches}
        globalLaunch={globalLaunch}
        onGlobalLaunchChange={setGlobalLaunch}
      />
    </div>
  );
}
