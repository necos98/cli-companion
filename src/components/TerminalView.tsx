import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import "@xterm/xterm/css/xterm.css";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Channel } from "@tauri-apps/api/core";
import { api } from "../lib/tauri";
import { isTerminalReply } from "../lib/agents";
import type { TabStatus } from "../lib/agents";
import type { Rect } from "../lib/layout";
import {
  RESTORE_SEPARATOR,
  SCROLLBACK_LINES,
  SCROLLBACK_SAVE_MS,
} from "../lib/session";
import { cn } from "../lib/utils";
import PaneHeader from "./PaneHeader";
import type { PtyEvent, Workspace } from "../lib/tauri";
import {
  clipboardKeyAction,
  createBusyTracker,
  resolvePasteAction,
  sanitizeTerminalTitle,
} from "../lib/terminal";
import { TERMINAL_THEME } from "../lib/terminalTheme";
import {
  LAUNCH_AFTER_OUTPUT_MS,
  LAUNCH_FALLBACK_MS,
  buildLaunchInput,
  shouldInjectLaunch,
} from "../lib/launch";

interface Props {
  /** Chiave del tab: diventa `CLI_COMPANION_SESSION` per gli hook degli agenti. */
  tabId: string;
  workspace: Workspace;
  /** Comando dell'avvio da eseguire all'apertura ("" = shell semplice). */
  initialCommand: string;
  /** Tab ripristinato da una sessione precedente: mostra prima la cronologia salvata. */
  restore?: boolean;
  /** Posizione e dimensione nell'area del gruppo, in percentuale. */
  rect: Rect | null;
  /** Visibile: gruppo attivo e non nascosto da un pannello massimizzato. */
  active: boolean;
  /** Riceve la tastiera (pannello attivo del gruppo attivo). */
  focused: boolean;
  label: string;
  /** Prefisso "workspace /" mostrato nella vista globale. */
  labelPrefix?: string;
  status: TabStatus;
  maximized: boolean;
  /** Fissato nella vista globale. */
  pinned: boolean;
  /** Evidenzia il pannello come bersaglio di un trascinamento. */
  dropTarget: boolean;
  onActivate: () => void;
  onClose: () => void;
  onToggleMaximize: () => void;
  onTogglePin: () => void;
  onPaneDragStart: () => void;
  onPaneDragOver: () => void;
  onPaneDragLeave: () => void;
  onPaneDrop: () => void;
  onPaneDragEnd: () => void;
  /** Registra (o rimuove, con null) il salvataggio dello scrollback di questo tab. */
  registerFlush?: (tabId: string, flush: (() => Promise<void>) | null) => void;
  onExit: () => void;
  onTitle?: (title: string) => void;
  onStatus?: (busy: boolean) => void;
  /** Input dell'utente nel terminale (`enter` = contiene Invio). */
  onInput?: (enter: boolean) => void;
  /**
   * Sequenza da inviare al pty al posto di Ctrl+V quando gli appunti non
   * hanno testo (profilo di input dell'agente del tab, già risolto per
   * l'ambiente del workspace); null = Ctrl+V grezzo.
   */
  imagePasteSeq?: string | null;
}

export default function TerminalView({
  tabId,
  workspace,
  initialCommand,
  restore = false,
  rect,
  active,
  focused,
  label,
  labelPrefix,
  status: paneStatus,
  maximized,
  pinned,
  dropTarget,
  onActivate,
  onClose,
  onToggleMaximize,
  onTogglePin,
  onPaneDragStart,
  onPaneDragOver,
  onPaneDragLeave,
  onPaneDrop,
  onPaneDragEnd,
  registerFlush,
  onExit,
  onTitle,
  onStatus,
  onInput,
  imagePasteSeq = null,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const onExitRef = useRef(onExit);
  onExitRef.current = onExit;
  const onTitleRef = useRef(onTitle);
  onTitleRef.current = onTitle;
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
  const onInputRef = useRef(onInput);
  onInputRef.current = onInput;
  const imagePasteSeqRef = useRef(imagePasteSeq);
  imagePasteSeqRef.current = imagePasteSeq;
  const registerFlushRef = useRef(registerFlush);
  registerFlushRef.current = registerFlush;
  const restoreRef = useRef(restore);
  const activeRef = useRef(active);
  activeRef.current = active;
  const sessionRef = useRef(0);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // Catturato al mount: modifiche successive agli avvii valgono
  // solo per i terminali aperti dopo.
  const pendingCommandRef = useRef(initialCommand);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      fontSize: 13,
      fontFamily: '"Geist Mono Variable", "Geist Mono", Consolas, "Cascadia Code", monospace',
      theme: TERMINAL_THEME,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const serialize = new SerializeAddon();
    term.loadAddon(serialize);
    term.open(el);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    let disposed = false;

    // Il font è bundlato ma caricato in modo lazy: xterm misura la cella al
    // primo render, quindi a font arrivato rilanciamo la misura (riassegnando
    // fontFamily) e rifacciamo il fit.
    void document.fonts
      .load('13px "Geist Mono Variable"')
      .then(() => document.fonts.ready)
      .then(() => {
        if (disposed) return;
        term.options.fontFamily = term.options.fontFamily;
        fit.fit();
      })
      .catch(() => {});

    // Scrollback: salvato ~5 s dopo l'output (e alla chiusura dell'app, via
    // registerFlush) cosi' un riavvio ripristina la cronologia.
    let dirty = false;
    let saveTimer: ReturnType<typeof setTimeout> | undefined;
    const flush = async () => {
      if (saveTimer !== undefined) {
        clearTimeout(saveTimer);
        saveTimer = undefined;
      }
      if (disposed || !dirty) return;
      dirty = false;
      try {
        const data = serialize.serialize({
          scrollback: SCROLLBACK_LINES,
          excludeAltBuffer: true,
          excludeModes: true,
        });
        if (data.trim() !== "" && !disposed) await api.saveScrollback(tabId, data);
      } catch {
        dirty = true;
      }
    };
    registerFlushRef.current?.(tabId, flush);

    const status = createBusyTracker({
      idleMs: 2000,
      onChange: (busy) => onStatusRef.current?.(busy),
    });

    // Iniezione del comando di avvio: al primo output della shell
    // (prompt pronto) + breve attesa, con fallback temporizzato per gli
    // avvii freddi che tardano a stampare. Una sola iniezione per tab.
    let injectTimer: ReturnType<typeof setTimeout> | undefined;
    let fallbackTimer: ReturnType<typeof setTimeout> | undefined;
    let injected = false;
    // Solo trim: il comando e' gia' limitato alla fonte e quello di ripristino
    // (`--resume <id>`) puo' superare il tetto senza dover essere troncato.
    const wantedCommand = pendingCommandRef.current.trim();

    const injectLaunch = () => {
      if (injected || disposed) return;
      const s = sessionRef.current;
      if (s === 0 || !shouldInjectLaunch(wantedCommand)) return;
      injected = true;
      void api
        .ptyWrite(s, buildLaunchInput(wantedCommand))
        .catch(() => undefined);
      status.markEnter();
    };

    const titleDisp = term.onTitleChange((t) => {
      const next = sanitizeTerminalTitle(t);
      if (next !== null) onTitleRef.current?.(next);
    });

    term.onData((data) => {
      const s = sessionRef.current;
      if (s !== 0) void api.ptyWrite(s, data).catch(() => undefined);
      const enter = data.includes("\r") || data.includes("\n");
      if (enter) status.markEnter();
      // Le risposte automatiche del terminale (focus, CPR...) non sono input.
      if (!isTerminalReply(data)) onInputRef.current?.(enter);
    });

    // Copia/incolla da tastiera: xterm non li gestisce da solo (senza
    // questo handler Ctrl+C arriva alla shell come SIGINT anche con testo
    // selezionato e Ctrl+V non legge gli appunti). Ritornare false blocca
    // l'invio del tasto al PTY; term.paste() riusa onData qui sopra.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const action = clipboardKeyAction(e, term.hasSelection());
      if (action === "copy") {
        const sel = term.getSelection();
        if (sel !== "") void writeText(sel).catch(() => undefined);
        return false;
      }
      if (action === "paste") {
        // Nel percorso normale xterm annulla l'evento (preventDefault):
        // ritornare false salta anche quell'annullamento, quindi senza
        // queste due righe il browser fa partire l'evento `paste` nativo e
        // il testo viene incollato due volte (evento nativo + term.paste).
        e.preventDefault();
        e.stopPropagation();
        void (async () => {
          let text = "";
          try {
            text = await readText();
          } catch {
            // Appunti non testuali (es. sola immagine): text resta "".
          }
          if (disposed) return;
          const resolved = resolvePasteAction(text);
          if (resolved.kind === "paste") {
            term.paste(resolved.text);
            return;
          }
          // Nessun testo (es. immagine copiata): inoltra Ctrl+V grezzo al
          // PTY, come faceva xterm prima della v0.3.1, così la CLI (Claude Code ecc.)
          // lo gestisce leggendo l'immagine dagli appunti di sistema. Se
          // l'agente del tab ha un tasto proprio (profilo di input, es.
          // Alt+V di Claude Code su Windows) si invia quello.
          const s = sessionRef.current;
          if (s !== 0)
            void api
              .ptyWrite(s, imagePasteSeqRef.current ?? resolved.key)
              .catch(() => undefined);
        })();
        return false;
      }
      return true;
    });

    const initialCols = term.cols;
    const initialRows = term.rows;

    // Le metriche della cella dipendono dal font: se il font si assesta dopo
    // il primo fit(), righe/colonne risultano stale. Questo refit riallinea
    // xterm e PTY; se scatta prima dello spawn viene saltato e ci pensa il
    // refit post-spawn qui sotto (i due coprono entrambi gli ordini).
    document.fonts.ready
      .then(() => {
        if (disposed) return;
        if (!activeRef.current || sessionRef.current === 0) return;
        try {
          fit.fit();
          void api
            .ptyResize(sessionRef.current, term.cols, term.rows)
            .catch(() => undefined);
        } catch {
          // ignora: succede durante lo smontaggio
        }
      })
      .catch(() => undefined);

    void (async () => {
      try {
        // Ripristino: cronologia salvata + separatore PRIMA dello spawn, cosi'
        // l'output della nuova shell le segue.
        if (restoreRef.current) {
          let saved: string | null = null;
          try {
            saved = await api.loadScrollback(tabId);
          } catch {
            // nessuna cronologia: resta il solo separatore
          }
          if (disposed) return;
          const text = `${saved ? saved + "\r\n" : ""}\x1b[2m${RESTORE_SEPARATOR}\x1b[0m\r\n`;
          await new Promise<void>((resolve) => term.write(text, resolve));
          if (disposed) return;
        }
        // Canale dedicato a questa sessione: riceve solo il suo output, anche
        // quello emesso prima che ptySpawn ritorni (prompt iniziale).
        const channel = new Channel<PtyEvent>();
        channel.onmessage = (e) => {
          if (disposed) return;
          if (e.kind === "exit") {
            status.markIdle();
            onExitRef.current();
            return;
          }
          term.write(e.data);
          status.markOutput();
          dirty = true;
          if (saveTimer === undefined) {
            saveTimer = setTimeout(() => {
              saveTimer = undefined;
              void flush();
            }, SCROLLBACK_SAVE_MS);
          }
          if (
            !injected &&
            injectTimer === undefined &&
            shouldInjectLaunch(wantedCommand)
          ) {
            injectTimer = setTimeout(
              injectLaunch,
              LAUNCH_AFTER_OUTPUT_MS,
            );
          }
        };
        const id = await api.ptySpawn(
          workspace.id,
          tabId,
          initialCols,
          initialRows,
          channel,
        );
        if (disposed) {
          void api.ptyClose(id).catch(() => undefined);
          return;
        }
        sessionRef.current = id;
        if (shouldInjectLaunch(wantedCommand)) {
          fallbackTimer = setTimeout(
            injectLaunch,
            LAUNCH_FALLBACK_MS,
          );
        }
        // Recupera eventuali resize avvenuti prima dello spawn (il
        // ResizeObserver li ignora finché la sessione è 0): riadatta xterm
        // al contenitore reale e risincronizza il PTY.
        if (activeRef.current) {
          try {
            fit.fit();
            void api
              .ptyResize(id, term.cols, term.rows)
              .catch(() => undefined);
          } catch {
            // ignora: succede durante lo smontaggio
          }
        }
      } catch (err) {
        term.writeln(`\x1b[1;31m${String(err)}\x1b[0m`);
      }
    })();

    const ro = new ResizeObserver(() => {
      if (!activeRef.current || sessionRef.current === 0) return;
      try {
        fit.fit();
        void api
          .ptyResize(sessionRef.current, term.cols, term.rows)
          .catch(() => undefined);
      } catch {
        // ignora: succede durante lo smontaggio
      }
    });
    ro.observe(el);

    return () => {
      disposed = true;
      registerFlushRef.current?.(tabId, null);
      if (saveTimer !== undefined) clearTimeout(saveTimer);
      ro.disconnect();
      if (injectTimer !== undefined) clearTimeout(injectTimer);
      if (fallbackTimer !== undefined) clearTimeout(fallbackTimer);
      titleDisp.dispose();
      status.dispose();
      const s = sessionRef.current;
      sessionRef.current = 0;
      if (s !== 0) void api.ptyClose(s).catch(() => undefined);
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, []);

  // Quando il pannello torna visibile, riadatta il terminale alla dimensione reale.
  useEffect(() => {
    if (!active) return;
    const term = termRef.current;
    const fit = fitRef.current;
    const s = sessionRef.current;
    if (!term || !fit || s === 0) return;
    requestAnimationFrame(() => {
      try {
        fit.fit();
        void api.ptyResize(s, term.cols, term.rows).catch(() => undefined);
      } catch {
        // ignora
      }
    });
  }, [active]);

  // La tastiera va solo al pannello attivo del gruppo attivo.
  useEffect(() => {
    if (!focused || !active) return;
    const id = requestAnimationFrame(() => termRef.current?.focus());
    return () => cancelAnimationFrame(id);
  }, [focused, active]);

  const style: React.CSSProperties =
    active && rect
      ? {
          display: "block",
          left: `${rect.left}%`,
          top: `${rect.top}%`,
          width: `${rect.width}%`,
          height: `${rect.height}%`,
        }
      : { display: "none" };

  return (
    <div
      className="absolute p-[3px]"
      style={style}
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        onPaneDragOver();
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) onPaneDragLeave();
      }}
      onDrop={(e) => {
        e.preventDefault();
        onPaneDrop();
      }}
    >
      <div
        className={cn(
          "terminal-pane relative flex h-full w-full flex-col overflow-hidden rounded-lg border bg-base",
          focused ? "border-primary" : "border-border",
          dropTarget && "border-dashed border-primary",
        )}
        data-focused={focused}
        onMouseDown={() => {
          onActivate();
          termRef.current?.focus();
        }}
      >
        <PaneHeader
          label={label}
          prefix={labelPrefix}
          focused={focused}
          status={paneStatus}
          maximized={maximized}
          pinned={pinned}
          onTogglePin={onTogglePin}
          onToggleMaximize={onToggleMaximize}
          onClose={onClose}
          onDragStart={(e) => {
            e.dataTransfer.setData("text/plain", tabId);
            e.dataTransfer.effectAllowed = "move";
            onPaneDragStart();
          }}
          onDragEnd={onPaneDragEnd}
        />
        {/* L'host di xterm non deve avere padding: FitAddon misura il
            contenitore e una riga/colonna in più finirebbe fuori vista. */}
        <div className="min-h-0 flex-1 py-1 pl-2">
          <div ref={containerRef} className="h-full w-full" />
        </div>
        {dropTarget && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-primary/[0.06] text-xs text-primary">
            Rilascia qui per spostare
          </div>
        )}
      </div>
    </div>
  );
}
