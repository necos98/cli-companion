import { useEffect, useMemo, useRef, useState } from "react";
import { planIdle, restingGoals } from "../../lib/office";
import type { IdleGoals, OfficeRoom } from "../../lib/office";
import type { LayoutFor } from "./useRoomLayouts";

/** Ogni quanto si ripianifica l'idle: l'unico timer dell'ufficio. */
export const IDLE_TICK_MS = 1000;

/** Mete di ogni stanza, per id del workspace. */
export type RoomGoals = ReadonlyMap<string, IdleGoals>;

/** Nessuna meta (stanza senza bot in idle). */
export const NO_GOALS: IdleGoals = new Map();
const NO_ROOM_GOALS: RoomGoals = new Map();

const idleKeys = (room: OfficeRoom): string[] =>
  room.bots.filter((b) => b.activity === "idle").map((b) => b.key);

/** Una passata su tutte le stanze; stessa istanza di `prev` se nessuna stanza e' cambiata. */
function planRooms(
  prev: RoomGoals,
  rooms: readonly OfficeRoom[],
  layoutFor: LayoutFor,
  now: number,
  random: () => number,
): RoomGoals {
  const next = new Map<string, IdleGoals>();
  let changed = prev.size !== rooms.length;
  for (const room of rooms) {
    const before = prev.get(room.workspaceId);
    const after = planIdle({
      layout: layoutFor(room.deskCount),
      idle: idleKeys(room),
      goals: before ?? NO_GOALS,
      now,
      random,
    });
    if (before === undefined || after !== before) changed = true;
    next.set(room.workspaceId, after);
  }
  return changed ? next : prev;
}

/**
 * Dove vanno i bot in pausa. Con il timer: ogni `IDLE_TICK_MS` pianifica le
 * stanze (e subito all'avvio) e aggiorna lo stato solo se qualcuna e' cambiata;
 * il timer gira solo se c'e' almeno un bot in idle. Con il movimento ridotto:
 * posti fissi, nessun timer. Le stanze e i layout piu' recenti si leggono da un
 * ref, cosi' l'intervallo non si ricrea a ogni render.
 */
export function useIdleGoals(
  rooms: readonly OfficeRoom[],
  layoutFor: LayoutFor,
  reduced: boolean,
): RoomGoals {
  const [goals, setGoals] = useState<RoomGoals>(NO_ROOM_GOALS);
  const goalsRef = useRef(goals);
  goalsRef.current = goals;
  const latest = useRef({ rooms, layoutFor });
  latest.current = { rooms, layoutFor };

  const hasIdle = rooms.some((r) => r.bots.some((b) => b.activity === "idle"));

  useEffect(() => {
    if (reduced || !hasIdle) return;
    const tick = () => {
      const next = planRooms(
        goalsRef.current,
        latest.current.rooms,
        latest.current.layoutFor,
        Date.now(),
        Math.random,
      );
      if (next !== goalsRef.current) setGoals(next);
    };
    tick();
    const id = window.setInterval(tick, IDLE_TICK_MS);
    return () => window.clearInterval(id);
  }, [reduced, hasIdle]);

  const resting = useMemo<RoomGoals>(
    () =>
      reduced
        ? new Map(rooms.map((r) => [r.workspaceId, restingGoals(layoutFor(r.deskCount), idleKeys(r))]))
        : NO_ROOM_GOALS,
    [reduced, rooms, layoutFor],
  );

  return reduced ? resting : goals;
}
