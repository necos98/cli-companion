// Ufficio: dove sta e cosa fa ogni bot (posizione, posa, velocita'). Funzione
// pura a zero dipendenze, riesportata da `office.ts`. Importa da `office.ts`
// solo tipi: nessun import circolare a runtime.

import { findSpot } from "./officeLayout.ts";
import type { Facing, Point, RoomLayout, SpotKind } from "./officeLayout.ts";
import type { IdleGoal, OfficeRoomBot } from "./office.ts";

export type BotPose =
  | "typing"
  | "raising-hand"
  | "waiting"
  | "celebrating"
  | "slumped"
  | "coffee"
  | "chatting"
  | "sitting"
  | "looking"
  | "standing";
export type BotSpeed = "walk" | "run";
export interface Placement extends Point {
  facing: Facing;
  pose: BotPose;
  speed: BotSpeed;
}

/** Posa di un bot in idle in base al tipo di meta. */
const GOAL_POSE: Readonly<Record<SpotKind | "chat", BotPose>> = {
  coffee: "coffee",
  chat: "chatting",
  sofa: "sitting",
  window: "looking",
  plant: "looking",
  floor: "standing",
};

/**
 * Dove sta e cosa fa un bot. `serverSlot` = indice del bot tra quelli in
 * background della stanza (oltre gli spot del server: in piedi alla scrivania).
 */
export function botPlacement(
  bot: Pick<OfficeRoomBot, "activity" | "desk">,
  layout: RoomLayout,
  goal: IdleGoal | undefined,
  serverSlot: number,
): Placement {
  const desk = layout.desks[bot.desk] ?? layout.desks[0];
  const at = (
    p: Point,
    pose: BotPose,
    facing: Facing = desk.facing,
    speed: BotSpeed = "walk",
  ): Placement => ({ x: p.x, y: p.y, facing, pose, speed });

  switch (bot.activity) {
    case "working":
      return at(desk.seat, "typing");
    case "failed":
      return at(desk.seat, "slumped");
    case "attention":
      return at(desk.standing, "raising-hand", desk.facing, "run");
    case "completed":
      return at(desk.standing, "celebrating");
    case "background": {
      const spot = layout.server[serverSlot];
      return spot ? at(spot, "waiting", spot.facing) : at(desk.standing, "waiting");
    }
    case "idle": {
      const spot = goal && findSpot(layout, goal.spotId);
      return goal && spot ? at(spot, GOAL_POSE[goal.kind], spot.facing) : at(desk.standing, "standing");
    }
  }
}
