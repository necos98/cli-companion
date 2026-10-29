import { useMemo } from "react";
import type { Facing } from "../../lib/office";
import { botLook, botPixels } from "../../lib/bots";
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
  armsOf,
  isSeated,
  shadeOf,
} from "./spriteParts";
import type { Part, PartTone, SpritePose } from "./spriteParts";

interface Props {
  /** Nome del bot: da lui dipende l'aspetto della testa (come in `BotAvatar`). */
  name: string;
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

function Parts({ parts, fills }: { parts: readonly Part[]; fills: Record<PartTone, string> }) {
  return (
    <>
      {parts.map((part, i) => (
        <rect key={i} x={part.x} y={part.y} width={part.w} height={part.h} fill={fills[part.tone]} />
      ))}
    </>
  );
}

/**
 * Lo sprite del bot: la testa e' l'avatar del nome, il corpo (busto, braccia,
 * gambe) cambia con la posa. Le parti hanno classi `office-*` che office.css
 * anima per posa; lo specchio per chi guarda a sinistra e' solo sull'SVG (il
 * nome del bot e' fuori, quindi non si specchia).
 */
export default function BotSprite({ name, pose, facing, intake, running }: Props) {
  const look = useMemo(() => botLook(name), [name]);
  const head = useMemo(() => botPixels(look), [look]);
  const fills = useMemo(() => fillsOf(look.color), [look.color]);
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
            <Parts parts={TORSO} fills={fills} />
            <g className="office-arm-l">
              <Parts parts={arms.left} fills={fills} />
            </g>
            <g className="office-arm-r">
              <Parts parts={arms.right} fills={fills} />
            </g>
            {intake && <Parts parts={CLIPBOARD} fills={fills} />}
            <g transform={`translate(${HEAD_X} ${headDrop})`}>
              <g className="office-head">
                <Parts parts={head} fills={fills} />
              </g>
            </g>
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
