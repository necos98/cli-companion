import { memo } from "react";
import type { CSSProperties } from "react";
import type { BotImage } from "../../lib/bots";
import { ACTIVITY_LABELS, BOT_HEIGHT, BOT_NAME_MAX_WIDTH, BOT_WIDTH, ROOM_WIDTH } from "../../lib/office";
import type { BotActivity, BotPose, BotSpeed, Facing } from "../../lib/office";
import BotSprite from "./BotSprite";
import { botBubble } from "./botBubble";
import { percent, roomUnits } from "./units";
import { useBotTrip } from "./useBotTrip";

/** Quanto pesa la y nel z-index: chi sta piu' in basso (piu' vicino) sta davanti. */
const Z_PER_UNIT = 10;

interface Props {
  /** Chiave del tab (identifica il bot verso `onOpen`). */
  botKey: string;
  name: string;
  /** Immagine del nome (se c'e'): il corpo dello sprite. */
  image?: BotImage;
  workspaceName: string;
  /** Etichetta del tab (titolo del task per i worker). */
  label: string;
  activity: BotActivity;
  /** Titolo del task (solo worker): finisce nel fumetto di chi lavora. */
  taskTitle?: string;
  intake: boolean;
  /** Piedi del bot (unita' stanza) e altezza della stanza, per le percentuali. */
  x: number;
  y: number;
  roomHeight: number;
  facing: Facing;
  pose: BotPose;
  speed: BotSpeed;
  /** Posto nella chiacchierata (sfasa il fumetto tra i partecipanti). */
  seat: number;
  reduced: boolean;
  onOpen: (key: string) => void;
}

/**
 * Un bot nella stanza: bottone posizionato coi piedi sul punto (x, y). Si muove
 * con una transizione CSS su left/top; mentre cammina mostra la camminata e
 * guarda dove va, all'arrivo prende la posa e il verso del posto.
 */
function OfficeBot(p: Props) {
  const trip = useBotTrip(p.x, p.y, p.facing, p.speed, p.reduced);
  const bubble = botBubble(p.activity, p.pose, p.name, p.taskTitle);
  const tooltip = [
    p.name,
    p.workspaceName,
    p.label,
    `Stato: ${ACTIVITY_LABELS[p.activity]}`,
  ].join("\n");
  const style = {
    left: percent(p.x, ROOM_WIDTH),
    top: percent(p.y, p.roomHeight),
    width: roomUnits(BOT_WIDTH),
    height: roomUnits(BOT_HEIGHT),
    transitionDuration: `${trip.durationMs}ms`,
    "--office-z": Math.round(p.y * Z_PER_UNIT),
    "--office-seat": p.seat,
  } as CSSProperties;

  return (
    <button
      type="button"
      className="office-bot"
      style={style}
      title={tooltip}
      aria-label={`Apri il terminale di ${p.name}`}
      data-seat={p.pose === "chatting" ? p.seat : undefined}
      onClick={() => p.onOpen(p.botKey)}
      onTransitionEnd={trip.onTransitionEnd}
    >
      {bubble && (!trip.walking || bubble.tone === "alert") && (
        <span className={`office-bubble font-mono office-bubble--${bubble.tone}`} aria-hidden="true">
          <span className="office-bubble-text">{bubble.text}</span>
        </span>
      )}
      <BotSprite
        name={p.name}
        image={p.image}
        pose={trip.walking ? "walking" : p.pose}
        facing={trip.walking ? trip.heading : p.facing}
        intake={p.intake}
        running={p.speed === "run"}
      />
      <span
        className="office-bot-name font-mono"
        style={{ maxWidth: roomUnits(BOT_NAME_MAX_WIDTH) }}
        aria-hidden="true"
      >
        {p.name}
      </span>
    </button>
  );
}

export default memo(OfficeBot);
