import { useMemo } from "react";
import { officeRooms } from "../../lib/office";
import type { OfficeSession } from "../../lib/office";
import Logo from "../Logo";
import OfficeHeader from "./OfficeHeader";
import OfficeRoom from "./OfficeRoom";
import { NO_GOALS, useIdleGoals } from "./useIdleGoals";
import { useReducedMotion } from "./useReducedMotion";
import { useRoomLayouts } from "./useRoomLayouts";
import { useStableDesks } from "./useStableDesks";
import "./office.css";

/** Larghezza massima di una stanza sola e minima di ognuna quando sono piu' d'una. */
const SINGLE_ROOM_MAX_WIDTH = 760;
const ROOM_MIN_WIDTH = 360;

interface Props {
  sessions: readonly OfficeSession[];
  /** Ordine dei workspace nella Sidebar: le stanze lo seguono. */
  workspaceOrder: readonly string[];
  /** Click su un bot: porta al suo terminale (deve essere stabile). */
  onOpenBot: (key: string) => void;
  onClose: () => void;
}

/**
 * L'Ufficio: una stanza per workspace con un bot per ogni tab aperto. Copre
 * l'area dei terminali (App lo monta solo mentre e' aperto, quindi desk, mete e
 * timer nascono a ogni apertura e muoiono alla chiusura).
 */
export default function OfficeView({ sessions, workspaceOrder, onOpenBot, onClose }: Props) {
  const reduced = useReducedMotion();
  const desks = useStableDesks(sessions);
  const rooms = useMemo(() => officeRooms(sessions, workspaceOrder, desks), [sessions, workspaceOrder, desks]);
  const layoutFor = useRoomLayouts();
  const goals = useIdleGoals(rooms, layoutFor, reduced);
  const single = rooms.length === 1;

  return (
    <div className="flex h-full min-h-0 flex-col bg-pane-area">
      <OfficeHeader botCount={sessions.length} onClose={onClose} />
      {rooms.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-5">
          <Logo size={40} />
          <span className="text-[13px] font-medium">L'ufficio è vuoto</span>
          <span className="text-center text-xs text-muted-foreground">
            Apri un terminale: il suo bot arriverà qui
          </span>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          <div
            className="mx-auto grid items-start gap-4"
            style={{
              maxWidth: single ? SINGLE_ROOM_MAX_WIDTH : undefined,
              gridTemplateColumns: single
                ? undefined
                : `repeat(auto-fit, minmax(min(100%, ${ROOM_MIN_WIDTH}px), 1fr))`,
            }}
          >
            {rooms.map((room) => (
              <OfficeRoom
                key={room.workspaceId}
                room={room}
                layout={layoutFor(room.deskCount)}
                goals={goals.get(room.workspaceId) ?? NO_GOALS}
                reduced={reduced}
                onOpenBot={onOpenBot}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
