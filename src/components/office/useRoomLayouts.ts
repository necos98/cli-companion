import { useMemo } from "react";
import { roomLayout } from "../../lib/office";
import type { RoomLayout } from "../../lib/office";

export type LayoutFor = (deskCount: number) => RoomLayout;

/** Layout delle stanze, calcolati una volta per numero di scrivanie (le istanze restano le stesse). */
export function useRoomLayouts(): LayoutFor {
  return useMemo(() => {
    const cache = new Map<number, RoomLayout>();
    return (deskCount) => {
      let layout = cache.get(deskCount);
      if (!layout) {
        layout = roomLayout(deskCount);
        cache.set(deskCount, layout);
      }
      return layout;
    };
  }, []);
}
