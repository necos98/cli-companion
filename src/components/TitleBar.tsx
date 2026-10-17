import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Star } from "lucide-react";
import Logo from "./Logo";
import StatusIcon from "./StatusIcon";
import type { TabStatus } from "../lib/agents";
import { cn } from "../lib/utils";

export interface TitleTab {
  /** Id del workspace (o GLOBAL_ID per la vista globale). */
  id: string;
  name: string;
  count: number;
  /** Stato aggregato del gruppo (null = nessun indicatore). */
  status: TabStatus | null;
}

interface Props {
  /** Vista globale: presente solo con almeno un tab fissato. */
  globalTab: TitleTab | null;
  globalActive: boolean;
  groups: TitleTab[];
  activeWorkspaceId: string | null;
  onSelectGlobal: () => void;
  onSelectGroup: (id: string) => void;
}

/** Esegue un'azione sulla finestra; fuori da Tauri (anteprima nel browser) non fa nulla. */
function withWindow(fn: (w: ReturnType<typeof getCurrentWindow>) => Promise<unknown>) {
  try {
    void fn(getCurrentWindow()).catch(() => undefined);
  } catch {
    /* fuori da Tauri */
  }
}

const CTRL =
  "flex w-[46px] items-center justify-center text-muted-foreground transition-colors hover:bg-surface-2 hover:text-foreground";

function Tab({
  tab,
  active,
  global,
  onClick,
}: {
  tab: TitleTab;
  active: boolean;
  global?: boolean;
  onClick: () => void;
}) {
  const attention = tab.status?.icon === "alert";
  return (
    <button
      type="button"
      onClick={onClick}
      title={global ? "Vista globale: terminali fissati di tutti i workspace" : tab.name}
      aria-label={
        global
          ? `Vista globale: ${tab.count} terminali`
          : `${tab.name}: ${tab.count} terminali`
      }
      aria-current={active ? "page" : undefined}
      className={cn(
        "relative z-10 flex h-9 flex-shrink-0 cursor-pointer select-none items-center gap-2 rounded-t-lg border border-b-0 px-3 text-[13px] transition-colors",
        active
          ? "border-border bg-pane-area text-foreground"
          : "border-transparent text-muted-foreground hover:text-foreground",
        attention && !active && "bg-status-attention/10 text-foreground",
      )}
    >
      {global ? (
        <Star className="h-3.5 w-3.5" aria-hidden="true" />
      ) : (
        active && <span className="h-1.5 w-1.5 rounded-full bg-primary" aria-hidden="true" />
      )}
      <span className={cn("whitespace-nowrap", active && "font-medium")}>
        {global ? "Globale" : tab.name}
      </span>
      <span
        className={cn("font-mono text-[11px]", active ? "text-muted-foreground" : "text-faint")}
      >
        {tab.count}
      </span>
      {tab.status && <StatusIcon status={tab.status} />}
    </button>
  );
}

/**
 * Barra del titolo personalizzata (la finestra non ha decorazioni native):
 * logo, tab dei gruppi di terminali e pulsanti della finestra. Le zone libere
 * sono `data-tauri-drag-region` (trascinamento + doppio clic = massimizza);
 * i controlli interattivi no.
 */
export default function TitleBar({
  globalTab,
  globalActive,
  groups,
  activeWorkspaceId,
  onSelectGlobal,
  onSelectGroup,
}: Props) {
  const [maximized, setMaximized] = useState(false);

  // Segue lo stato di massimizzazione per scambiare l'icona.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      const win = getCurrentWindow();
      const sync = () => {
        void win
          .isMaximized()
          .then((m) => {
            if (!cancelled) setMaximized(m);
          })
          .catch(() => undefined);
      };
      sync();
      void win
        .onResized(sync)
        .then((u) => {
          if (cancelled) u();
          else unlisten = u;
        })
        .catch(() => undefined);
    } catch {
      /* fuori da Tauri */
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="relative flex h-11 flex-shrink-0 select-none items-stretch bg-base"
    >
      {/* Linea di base: i tab attivi la coprono per fondersi col contenuto. */}
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />

      <div
        data-tauri-drag-region
        className="flex w-[260px] flex-shrink-0 items-center gap-2.5 border-r px-4"
      >
        <Logo size={20} className="pointer-events-none" />
        <span className="pointer-events-none text-[13px] font-semibold tracking-tight">
          cli-companion
        </span>
      </div>

      <div
        data-tauri-drag-region
        onWheel={(e) => {
          e.currentTarget.scrollLeft += e.deltaY;
        }}
        className="flex min-w-0 flex-1 items-end gap-0.5 overflow-x-auto px-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {globalTab && (
          <Tab tab={globalTab} global active={globalActive} onClick={onSelectGlobal} />
        )}
        {groups.map((g) => (
          <Tab
            key={g.id}
            tab={g}
            active={!globalActive && g.id === activeWorkspaceId}
            onClick={() => onSelectGroup(g.id)}
          />
        ))}
        {/* Resto della barra: area di trascinamento anche con molti tab. */}
        <div data-tauri-drag-region className="h-full min-w-4 flex-1" />
      </div>

      <div className="flex items-stretch">
        <button
          type="button"
          aria-label="Riduci a icona"
          title="Riduci a icona"
          onClick={() => withWindow((w) => w.minimize())}
          className={CTRL}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M1 6h10" stroke="currentColor" strokeWidth="1" />
          </svg>
        </button>
        <button
          type="button"
          aria-label={maximized ? "Ripristina" : "Ingrandisci"}
          title={maximized ? "Ripristina" : "Ingrandisci"}
          onClick={() => withWindow((w) => w.toggleMaximize())}
          className={CTRL}
        >
          {maximized ? (
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <rect x="1.5" y="3.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
              <path d="M3.5 3.5v-2h7v7h-2" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <rect x="1.5" y="1.5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          )}
        </button>
        {/* close() passa dal flusso onCloseRequested di App (salvataggio
            sessione e scrollback), mai destroy() diretto. */}
        <button
          type="button"
          aria-label="Chiudi"
          title="Chiudi"
          onClick={() => withWindow((w) => w.close())}
          className="flex w-[46px] items-center justify-center text-muted-foreground transition-colors hover:bg-[#e81123] hover:text-white"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M1.5 1.5l9 9M10.5 1.5l-9 9" stroke="currentColor" strokeWidth="1" />
          </svg>
        </button>
      </div>
    </div>
  );
}
