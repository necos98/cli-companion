import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Background, Controls, ReactFlow } from "@xyflow/react";
import type { Edge, ReactFlowInstance } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { GitBranch, Loader2, Minus, Moon, Pause, Play, Plus } from "lucide-react";
import { Button } from "./ui/button";
import TaskDetails from "./TaskDetails";
import TaskNode from "./TaskNode";
import type { TaskFlowNode } from "./TaskNode";
import { api } from "../lib/tauri";
import { cn } from "../lib/utils";
import {
  COLUMNS,
  EMPTY_BOARD_TEXT,
  MAX_PARALLEL_MAX,
  MAX_PARALLEL_MIN,
  clampMaxParallel,
  columnCounts,
  heldCount,
  columnDotClass,
  columnHex,
  columnLabel,
  progress,
  taskIdsKey,
} from "../lib/board";
import { automationBadgeText, automationBadgeTone } from "../lib/schedule";
import type { BoardView as BoardData, TaskAction } from "../lib/board";
import { layoutBoard, layoutSignature } from "../lib/boardLayout";

// Definito fuori dal componente: React Flow richiede un oggetto stabile.
const NODE_TYPES = { task: TaskNode };
const FIT_OPTIONS = { padding: 0.15, maxZoom: 1 };

interface Props {
  workspaceId: string;
  /** Board corrente (null = non ancora caricata o comando fallito). */
  board: BoardData | null;
  /** Errore del caricamento iniziale, se `boardGet` è fallito. */
  loadError: string | null;
  /** Aggiorna lo stato della board in App (risposta di un comando). */
  onBoard: (board: BoardData) => void;
  /** Torna ai terminali e mette a fuoco il tab del worker. */
  onOpenTerminal: (sessionId: string) => void;
}

/** Vista a grafo della board di un workspace (React Flow, sinistra → destra). */
export default function BoardView({
  workspaceId,
  board,
  loadError,
  onBoard,
  onOpenTerminal,
}: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const tasks = board?.tasks ?? [];

  // Cambia l'insieme dei task (non il loro stato): riadatta la vista al nuovo grafo.
  const flowRef = useRef<ReactFlowInstance<TaskFlowNode, Edge> | null>(null);
  const idsKey = taskIdsKey(tasks);
  const prevIdsKey = useRef<string | null>(null);
  useEffect(() => {
    const prev = prevIdsKey.current;
    prevIdsKey.current = idsKey;
    if (prev === null || prev === idsKey) return;
    const id = setTimeout(() => void flowRef.current?.fitView({ ...FIT_OPTIONS, duration: 200 }), 60);
    return () => clearTimeout(id);
  }, [idsKey]);

  // Il layout dipende solo dalla struttura del grafo: gli aggiornamenti di
  // stato non spostano i nodi.
  const signature = layoutSignature(tasks);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const layout = useMemo(() => layoutBoard(tasks), [signature]);

  const nodes = useMemo<TaskFlowNode[]>(() => {
    const byId = new Map(tasks.map((t) => [t.id, t]));
    return layout.nodes.flatMap((n) => {
      const task = byId.get(n.id);
      if (!task) return [];
      return [
        {
          id: n.id,
          type: "task" as const,
          position: n.position,
          data: { task, selected: n.id === selectedId },
        },
      ];
    });
  }, [layout, tasks, selectedId]);

  const edges = useMemo<Edge[]>(() => {
    const byId = new Map(tasks.map((t) => [t.id, t]));
    return layout.edges.map((e) => {
      const src = byId.get(e.source);
      const done = src?.status === "done";
      return {
        id: e.id,
        source: e.source,
        target: e.target,
        animated: byId.get(e.target)?.status === "running",
        style: { stroke: done ? columnHex("done") : "#3a4857", strokeWidth: 1.5 },
      };
    });
  }, [layout, tasks]);

  const selected = tasks.find((t) => t.id === selectedId) ?? null;

  const run = useCallback(
    async (op: () => Promise<BoardData>) => {
      setError(null);
      setBusy(true);
      try {
        onBoard(await op());
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [onBoard],
  );

  const setMaxParallel = (n: number) =>
    void run(() => api.boardSet(workspaceId, { maxParallel: clampMaxParallel(n) }));
  const setPaused = (paused: boolean) => void run(() => api.boardSet(workspaceId, { paused }));
  const doAction = (a: TaskAction) => {
    if (!selected) return;
    void run(() => api.boardTaskAction(workspaceId, selected.id, a));
  };

  const setTaskModel = (model: string | null) => {
    if (!selected) return;
    void run(() => api.boardTaskSetModel(workspaceId, selected.id, model));
  };

  const prog = progress(tasks);
  const counts = columnCounts(tasks);
  const held = heldCount(tasks);
  const maxParallel = board?.max_parallel ?? 3;
  const paused = board?.paused ?? false;
  const shownError = error ?? loadError;
  const badge = automationBadgeText(board?.automation);
  const tone = automationBadgeTone(board?.automation);
  // Branch di integrazione gestiti dalla board (repo → branch), in un tooltip.
  const integrations = useMemo(() => {
    const entries = Object.entries(board?.integrations ?? {});
    if (entries.length === 0) return null;
    return {
      count: entries.length,
      title: entries.map(([repo, i]) => `${repo === "." ? "(root)" : repo} → ${i.branch}\n${i.path}`).join("\n"),
    };
  }, [board?.integrations]);

  return (
    <div className="flex h-full w-full flex-col bg-pane-area">
      {/* Barra: avanzamento, parallelismo, pausa e legenda. */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 border-b bg-pane-header px-3 py-2">
        <div className="flex items-center gap-2" title="Task fatti sul totale">
          <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-faint">
            Board
          </span>
          <span className="font-mono text-[12px] text-foreground">
            {prog.done}/{prog.total}
          </span>
          <span className="h-1 w-20 overflow-hidden rounded-full bg-surface-2">
            <span
              className="block h-full bg-status-finished transition-[width]"
              style={{ width: `${prog.total === 0 ? 0 : (prog.done * 100) / prog.total}%` }}
            />
          </span>
        </div>

        <div className="flex items-center gap-1.5" title="Worker della board in esecuzione insieme">
          <span className="text-[11px] text-muted-foreground">Parallelismo</span>
          <button
            type="button"
            aria-label="Meno worker in parallelo"
            disabled={busy || maxParallel <= MAX_PARALLEL_MIN}
            onClick={() => setMaxParallel(maxParallel - 1)}
            className="flex h-5 w-5 items-center justify-center rounded border border-border-strong text-muted-foreground hover:bg-surface-2 hover:text-foreground disabled:opacity-40 [&_svg]:size-3"
          >
            <Minus />
          </button>
          <span className="w-5 text-center font-mono text-[12px]">{maxParallel}</span>
          <button
            type="button"
            aria-label="Più worker in parallelo"
            disabled={busy || maxParallel >= MAX_PARALLEL_MAX}
            onClick={() => setMaxParallel(maxParallel + 1)}
            className="flex h-5 w-5 items-center justify-center rounded border border-border-strong text-muted-foreground hover:bg-surface-2 hover:text-foreground disabled:opacity-40 [&_svg]:size-3"
          >
            <Plus />
          </button>
        </div>

        <Button
          size="sm"
          variant="secondary"
          disabled={busy || board === null}
          onClick={() => setPaused(!paused)}
          className="h-6 gap-1.5 px-2 text-[11px]"
        >
          {busy ? (
            <Loader2 className="!size-3 animate-spin" />
          ) : paused ? (
            <Play className="!size-3" />
          ) : (
            <Pause className="!size-3" />
          )}
          {paused ? "Riprendi la board" : "Metti in pausa la board"}
        </Button>
        {paused && <span className="text-[11px] text-status-attention">Board in pausa</span>}

        {integrations && (
          <span
            title={integrations.title}
            className="flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] text-muted-foreground"
          >
            <GitBranch className="h-3 w-3" />
            Integrazione
            <span className="font-mono text-faint">{integrations.count}</span>
          </span>
        )}

        {badge && (
          <span
            title="Workspace con orchestratore automatico (Impostazioni › Fasce orarie)"
            className={cn(
              "flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px]",
              tone === "open" ? "border-status-running/50 text-status-running" : tone === "waiting" ? "border-primary/40 text-primary" : "text-muted-foreground",
            )}
          >
            <Moon className="h-3 w-3" />
            {badge}
          </span>
        )}

        <ul className="ml-auto flex flex-wrap items-center gap-x-3 gap-y-1" aria-label="Legenda">
          {COLUMNS.map((c) => (
            <li key={c} className="flex items-center gap-1 text-[11px] text-muted-foreground">
              <span className={cn("h-2 w-2 rounded-full", columnDotClass(c))} />
              {columnLabel(c)}
              {counts[c] > 0 && <span className="font-mono text-faint">{counts[c]}</span>}
              {c === "ready" && held > 0 && (
                <span className="text-status-attention" title="Task trattenuti: non partono finché non vengono rilasciati">
                  · {held} in pausa
                </span>
              )}
            </li>
          ))}
        </ul>
      </div>

      {shownError && (
        <p className="border-b px-3 py-1.5 text-xs text-destructive">{shownError}</p>
      )}

      <div className="flex min-h-0 flex-1">
        <div className="relative min-w-0 flex-1">
          {board === null && !loadError ? (
            <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              Caricamento…
            </div>
          ) : tasks.length === 0 ? (
            <div className="flex h-full items-center justify-center p-6">
              <p className="max-w-[360px] text-center text-[13px] text-muted-foreground">
                {EMPTY_BOARD_TEXT}
              </p>
            </div>
          ) : (
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={NODE_TYPES}
              colorMode="dark"
              fitView
              fitViewOptions={FIT_OPTIONS}
              onInit={(i) => {
                flowRef.current = i;
              }}
              minZoom={0.2}
              nodesDraggable={false}
              nodesConnectable={false}
              elementsSelectable={false}
              proOptions={{ hideAttribution: true }}
              onNodeClick={(_, n) => setSelectedId((cur) => (cur === n.id ? null : n.id))}
              onPaneClick={() => setSelectedId(null)}
              className="board-flow"
            >
              <Background color="#24303d" gap={20} size={1} />
              <Controls showInteractive={false} />
            </ReactFlow>
          )}
        </div>

        {selected && (
          <TaskDetails
            task={selected}
            busy={busy}
            onAction={doAction}
            onSetModel={setTaskModel}
            onOpenTerminal={onOpenTerminal}
            onClose={() => setSelectedId(null)}
          />
        )}
      </div>
    </div>
  );
}
