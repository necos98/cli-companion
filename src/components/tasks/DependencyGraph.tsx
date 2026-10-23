import { useEffect, useMemo, useRef } from "react";
import { Background, Controls, ReactFlow } from "@xyflow/react";
import type { Edge, ReactFlowInstance } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import TaskNode from "./TaskNode";
import type { TaskFlowNode } from "./TaskNode";
import { statusHex, taskIdsKey } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import { layoutBoard, layoutSignature } from "../../lib/boardLayout";

// Definito fuori dal componente: React Flow richiede un oggetto stabile.
const NODE_TYPES = { task: TaskNode };
const FIT_OPTIONS = { padding: 0.15, maxZoom: 1 };

interface Props {
  board: BoardView;
  /** Task evidenziato (quello aperto nel dettaglio). */
  selectedId?: string | null;
  /** Click su un nodo (o Invio/Spazio): il chiamante apre il dettaglio. */
  onSelect: (id: string) => void;
}

/**
 * Grafo delle dipendenze di un workspace (React Flow + dagre, sinistra → destra).
 * Nodi con i quattro stati (e "Richiede attenzione"); l'arco va dalla
 * dipendenza al dipendente ed è verde quando la dipendenza è Fatta. Riempie il
 * contenitore: il genitore gli dà un'altezza.
 */
export default function DependencyGraph({ board, selectedId = null, onSelect }: Props) {
  const tasks = board.tasks;

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
    return layout.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      animated: byId.get(e.target)?.status === "running",
      style: {
        stroke: byId.get(e.source)?.status === "done" ? statusHex("done") : "#3a4857",
        strokeWidth: 1.5,
      },
    }));
  }, [layout, tasks]);

  if (tasks.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <p className="text-[13px] text-muted-foreground">Nessun task</p>
      </div>
    );
  }

  return (
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
      onNodeClick={(_, n) => onSelect(n.id)}
      className="board-flow"
    >
      <Background color="#24303d" gap={20} size={1} />
      <Controls showInteractive={false} />
    </ReactFlow>
  );
}
