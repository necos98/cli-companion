import { useMemo } from "react";
import type { Facing } from "../../lib/office";
import { botImageHref, botLook, botPixels } from "../../lib/bots";
import type { BotImage } from "../../lib/bots";
import { BOT_IMAGE_MAX_H, BOT_IMAGE_MAX_W, pngSize } from "../../lib/botImage";
import { DARK, LIGHT } from "../BotAvatar";
import { SPRITE_PROPS } from "./palette";
import {
  CLIPBOARD,
  HEAD_X,
  LEGS_SEATED,
  LEG_LEFT,
  LEG_RIGHT,
  SIT_DROP,
  SLUMP_DROP,
  SPRITE_H,
  SPRITE_W,
  STEAM,
  TORSO,
  armShiftOf,
  armsOf,
  imageBodyBox,
  isSeated,
  shadeOf,
} from "./spriteParts";
import type { ArmShift, Part, PartTone, SpritePose } from "./spriteParts";

interface Props {
  /** Nome del bot: da lui dipende l'aspetto della testa (come in `BotAvatar`). */
  name: string;
  /** Immagine del nome: sostituisce busto e testa (braccia e gambe restano). */
  image?: BotImage;
  pose: SpritePose;
  facing: Facing;
  /** Bot Task Intake: porta una cartellina. */
  intake: boolean;
  /** Camminata veloce (passo piu' rapido). */
  running: boolean;
}

function fillsOf(color: string): Record<PartTone, string> {
  return {
    body: color,
    shade: shadeOf(color),
    dark: DARK,
    light: LIGHT,
    mug: SPRITE_PROPS.mug,
    steam: SPRITE_PROPS.steam,
    clipboard: SPRITE_PROPS.clipboard,
    paper: SPRITE_PROPS.paper,
  };
}

/** Dimensione di ripiego se l'intestazione del PNG non si legge. */
const FALLBACK_IMAGE_SIZE = { w: BOT_IMAGE_MAX_W, h: BOT_IMAGE_MAX_H };

function Parts({ parts, fills }: { parts: readonly Part[]; fills: Record<PartTone, string> }) {
  return (
    <>
      {parts.map((part, i) => (
        <rect key={i} x={part.x} y={part.y} width={part.w} height={part.h} fill={fills[part.tone]} />
      ))}
    </>
  );
}

interface ArmProps {
  className: string;
  parts: readonly Part[];
  fills: Record<PartTone, string>;
  /** Con un corpo-immagine: spostamento che tiene il braccio attaccato. */
  shift?: ArmShift;
}

/** Un braccio animato; lo spostamento sta in un gruppo esterno (le animazioni CSS usano il transform dell'interno). */
function Arm({ className, parts, fills, shift }: ArmProps) {
  const arm = (
    <g className={className}>
      <Parts parts={parts} fills={fills} />
    </g>
  );
  return shift ? <g transform={`translate(${shift.dx} ${shift.dy})`}>{arm}</g> : arm;
}

/**
 * Lo sprite del bot: la testa e' l'avatar del nome, il corpo (busto, braccia,
 * gambe) cambia con la posa. Con un'immagine, questa prende il posto di busto e
 * testa (braccia e gambe restano). Le parti hanno classi `office-*` che
 * office.css anima per posa; lo specchio per chi guarda a sinistra e' solo
 * sull'SVG (il nome del bot e' fuori, quindi non si specchia).
 */
export default function BotSprite({ name, image, pose, facing, intake, running }: Props) {
  const look = useMemo(() => botLook(name), [name]);
  const head = useMemo(() => botPixels(look), [look]);
  const fills = useMemo(() => fillsOf(image?.color ?? look.color), [image?.color, look.color]);
  const box = useMemo(
    () => (image ? imageBodyBox(pngSize(image.png) ?? FALLBACK_IMAGE_SIZE) : undefined),
    [image],
  );
  const shift = useMemo(() => (box ? armShiftOf(box) : undefined), [box]);
  const seated = isSeated(pose);
  const arms = armsOf(pose);
  const headDrop = pose === "slumped" ? SLUMP_DROP : 0;

  return (
    <svg
      className="office-sprite"
      viewBox={`0 0 ${SPRITE_W} ${SPRITE_H}`}
      shapeRendering="crispEdges"
      aria-hidden="true"
      data-pose={pose}
      data-running={running || undefined}
      style={facing === -1 ? { transform: "scaleX(-1)" } : undefined}
    >
      <g className="office-jump">
        {seated ? (
          <Parts parts={LEGS_SEATED} fills={fills} />
        ) : (
          <>
            <g className="office-leg-l">
              <Parts parts={LEG_LEFT} fills={fills} />
            </g>
            <g className="office-leg-r">
              <Parts parts={LEG_RIGHT} fills={fills} />
            </g>
          </>
        )}
        <g transform={seated ? `translate(0 ${SIT_DROP})` : undefined}>
          <g className="office-bob">
            {image && box ? (
              <g transform={`translate(0 ${headDrop})`}>
                <image
                  href={botImageHref(image)}
                  x={box.x}
                  y={box.y}
                  width={box.w}
                  height={box.h}
                  preserveAspectRatio="none"
                  style={{ imageRendering: "pixelated" }}
                />
              </g>
            ) : (
              <Parts parts={TORSO} fills={fills} />
            )}
            <Arm className="office-arm-l" parts={arms.left} fills={fills} shift={shift?.left} />
            <Arm className="office-arm-r" parts={arms.right} fills={fills} shift={shift?.right} />
            {intake && <Parts parts={CLIPBOARD} fills={fills} />}
            {!image && (
              <g transform={`translate(${HEAD_X} ${headDrop})`}>
                <g className="office-head">
                  <Parts parts={head} fills={fills} />
                </g>
              </g>
            )}
            {pose === "coffee" && (
              <g className="office-steam">
                <Parts parts={STEAM} fills={fills} />
              </g>
            )}
          </g>
        </g>
      </g>
    </svg>
  );
}
