// Auto-layout del grafo delle dipendenze: i task diventano nodi e archi di React Flow,
// disposti da sinistra a destra con dagre (arco dalla dipendenza al dipendente).
// Il modulo non importa React Flow (solo tipi propri): testabile con node --test.

import dagre from "@dagrejs/dagre";
import type { Task } from "./board.ts";

export const NODE_WIDTH = 240;
export const NODE_HEIGHT = 88;

export interface LayoutNode {
  id: string;
  position: { x: number; y: number };
}

export interface LayoutEdge {
  id: string;
  source: string;
  target: string;
}

export interface BoardLayout {
  nodes: LayoutNode[];
  edges: LayoutEdge[];
}

/**
 * Firma della struttura del grafo (id e dipendenze): cambia solo se cambia la
 * forma, non lo stato dei task, così il layout non si ricalcola a ogni update.
 */
export function layoutSignature(tasks: Pick<Task, "id" | "depends_on">[]): string {
  return tasks.map((t) => `${t.id}<${t.depends_on.join(",")}`).join("|");
}

/** Archi validi: la dipendenza deve esistere nella board. Niente duplicati. */
export function boardEdges(tasks: Pick<Task, "id" | "depends_on">[]): LayoutEdge[] {
  const ids = new Set(tasks.map((t) => t.id));
  const seen = new Set<string>();
  const edges: LayoutEdge[] = [];
  for (const t of tasks) {
    for (const dep of t.depends_on) {
      const id = `${dep}->${t.id}`;
      if (!ids.has(dep) || dep === t.id || seen.has(id)) continue;
      seen.add(id);
      edges.push({ id, source: dep, target: t.id });
    }
  }
  return edges;
}

/** Posizioni dei nodi (angolo in alto a sinistra) e archi, da sinistra a destra. */
export function layoutBoard(tasks: Pick<Task, "id" | "depends_on">[]): BoardLayout {
  const edges = boardEdges(tasks);
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: "LR", nodesep: 28, ranksep: 70, marginx: 20, marginy: 20 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const t of tasks) g.setNode(t.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  for (const e of edges) g.setEdge(e.source, e.target);
  dagre.layout(g);
  const nodes = tasks.map((t) => {
    const n = g.node(t.id);
    return {
      id: t.id,
      position: { x: n.x - NODE_WIDTH / 2, y: n.y - NODE_HEIGHT / 2 },
    };
  });
  return { nodes, edges };
}
