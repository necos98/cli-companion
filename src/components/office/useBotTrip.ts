import { useCallback, useState } from "react";
import type { TransitionEvent } from "react";
import type { BotSpeed, Facing } from "../../lib/office";

/** Velocita' del bot (unita' stanza al secondo): passo normale e corsa verso la scrivania. */
const WALK_UNITS_PER_S = 3;
const RUN_UNITS_PER_S = 8;
/** Durata minima e massima di un tragitto (ms). */
const MIN_TRIP_MS = 300;
const MAX_TRIP_MS = 5000;
/** Sotto questo spostamento orizzontale (u) il bot non si gira. */
const TURN_THRESHOLD = 0.05;

const SPEED: Record<BotSpeed, number> = { walk: WALK_UNITS_PER_S, run: RUN_UNITS_PER_S };

interface Trip {
  /** Meta a cui si riferisce il tragitto. */
  x: number;
  y: number;
  walking: boolean;
  /** Verso di marcia durante il tragitto. */
  heading: Facing;
  durationMs: number;
}

export interface BotTrip {
  walking: boolean;
  heading: Facing;
  /** Durata della transizione CSS di left/top. */
  durationMs: number;
  /** Fine della transizione: il bot e' arrivato. */
  onTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
}

/**
 * Il tragitto di un bot quando la sua meta cambia: durata (distanza / velocita',
 * limitata), verso di marcia e se sta camminando. Il movimento vero e' una
 * transizione CSS su left/top: nessun timer, nessun frame. La camminata finisce
 * quando la transizione termina. Con il movimento ridotto il bot si teletrasporta.
 */
export function useBotTrip(
  x: number,
  y: number,
  facing: Facing,
  speed: BotSpeed,
  reduced: boolean,
): BotTrip {
  const [trip, setTrip] = useState<Trip>({ x, y, walking: false, heading: facing, durationMs: 0 });

  // Meta cambiata: si ricalcola il tragitto durante il render (derivato dalle props).
  if (trip.x !== x || trip.y !== y) {
    const dx = x - trip.x;
    const distance = Math.hypot(dx, y - trip.y);
    const ms = Math.min(MAX_TRIP_MS, Math.max(MIN_TRIP_MS, (distance / SPEED[speed]) * 1000));
    setTrip({
      x,
      y,
      walking: !reduced,
      heading: Math.abs(dx) < TURN_THRESHOLD ? trip.heading : dx > 0 ? 1 : -1,
      durationMs: reduced ? 0 : ms,
    });
  }

  const onTransitionEnd = useCallback((e: TransitionEvent<HTMLElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.propertyName !== "left" && e.propertyName !== "top") return;
    setTrip((t) => (t.walking ? { ...t, walking: false } : t));
  }, []);

  return {
    walking: trip.walking && !reduced,
    heading: trip.heading,
    durationMs: reduced ? 0 : trip.durationMs,
    onTransitionEnd,
  };
}
