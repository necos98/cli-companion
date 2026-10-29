import { memo, useMemo } from "react";
import { botPlacement } from "../../lib/office";
import type { IdleGoal, IdleGoals, OfficeRoom as RoomData, Placement, RoomLayout } from "../../lib/office";
import OfficeBackdrop from "./OfficeBackdrop";
import OfficeBot from "./OfficeBot";
import OfficeDesk from "./OfficeDesk";
import type { ScreenState } from "./OfficeDesk";
import OfficeFurniture from "./OfficeFurniture";
import { ROOM_UNIT_STYLE, percent, roomUnits } from "./units";

/** Margine del nome dentro la lavagna e altezza del testo (u). */
const BOARD_PADDING = 0.45;
const BOARD_LABEL_HEIGHT = 1.1;
const BOARD_FONT = 0.7;

interface Props {
  room: RoomData;
  layout: RoomLayout;
  goals: IdleGoals;
  reduced: boolean;
  onOpenBot: (key: string) => void;
}

interface PlacedBot {
  placement: Placement;
  /** Posto nella chiacchierata (0 se il bot non chiacchiera). */
  seat: number;
}

/** Indice del posto di una chiacchierata dall'id (`chat-0-1` -> 1). */
function chatSeat(goal: IdleGoal | undefined): number {
  if (!goal?.chat) return 0;
  return Number(goal.spotId.slice(goal.chat.length + 1)) || 0;
}

/** Schermo di ogni scrivania: acceso se ci lavora un bot, in errore se e' fallito. */
function screenOf(room: RoomData, desk: number): ScreenState {
  const activity = room.bots.find((b) => b.desk === desk)?.activity;
  if (activity === "working") return "on";
  return activity === "failed" ? "error" : "off";
}

/** Una stanza: scena SVG (parete, mobili, scrivanie) con i bot sopra, nel suo rapporto d'aspetto. */
function OfficeRoom({ room, layout, goals, reduced, onOpenBot }: Props) {
  const board = layout.furniture.find((f) => f.kind === "board");
  const placed = useMemo<PlacedBot[]>(() => {
    let background = 0;
    return room.bots.map((bot) => {
      const goal = goals.get(bot.key);
      const slot = bot.activity === "background" ? background++ : 0;
      return { placement: botPlacement(bot, layout, goal, slot), seat: chatSeat(goal) };
    });
  }, [room, layout, goals]);

  return (
    <section
      aria-label={`Stanza ${room.name}`}
      className="office-room relative isolate overflow-hidden rounded-lg ring-1 ring-border-strong"
      style={{ ...ROOM_UNIT_STYLE, containerType: "inline-size", aspectRatio: `${layout.width} / ${layout.height}` }}
    >
      <svg
        className="absolute inset-0 size-full"
        viewBox={`0 0 ${layout.width} ${layout.height}`}
        preserveAspectRatio="none"
        shapeRendering="crispEdges"
        aria-hidden="true"
      >
        <OfficeBackdrop width={layout.width} height={layout.height} />
        {layout.furniture.map((item) => (
          <OfficeFurniture key={item.kind} item={item} />
        ))}
        {layout.desks.map((desk, i) => (
          <OfficeDesk key={i} desk={desk} screen={screenOf(room, i)} />
        ))}
      </svg>
      {board && (
        <div
          className="office-board-label font-mono"
          style={{
            left: percent(board.x + BOARD_PADDING, layout.width),
            top: percent(board.y + BOARD_PADDING, layout.height),
            width: percent(board.w - 2 * BOARD_PADDING, layout.width),
            height: percent(BOARD_LABEL_HEIGHT, layout.height),
            fontSize: roomUnits(BOARD_FONT),
          }}
        >
          {room.name}
        </div>
      )}
      {room.bots.map((bot, i) => (
        <OfficeBot
          key={bot.key}
          botKey={bot.key}
          name={bot.bot}
          workspaceName={bot.workspaceName}
          label={bot.label}
          activity={bot.activity}
          taskTitle={bot.taskTitle}
          intake={bot.intake}
          x={placed[i].placement.x}
          y={placed[i].placement.y}
          roomHeight={layout.height}
          facing={placed[i].placement.facing}
          pose={placed[i].placement.pose}
          speed={placed[i].placement.speed}
          seat={placed[i].seat}
          reduced={reduced}
          onOpen={onOpenBot}
        />
      ))}
    </section>
  );
}

export default memo(OfficeRoom);
