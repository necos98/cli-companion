import { BOT_GRID, botLook, botPixels } from "../lib/bots";
import type { PixelTone } from "../lib/bots";
import { cn } from "../lib/utils";

interface Props {
  /** Nome del bot: da lui dipende tutto l'aspetto. */
  name: string;
  /** Lato in pixel (16-20 nelle intestazioni). */
  size?: number;
  className?: string;
}

const DARK = "#0f172a";
const LIGHT = "rgba(255, 255, 255, 0.6)";

/** Avatar pixel-art di un bot, generato dal nome (SVG inline, nessuna immagine). */
export default function BotAvatar({ name, size = 18, className }: Props) {
  const look = botLook(name);
  const fill: Record<PixelTone, string> = { body: look.color, dark: DARK, light: LIGHT };
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${BOT_GRID} ${BOT_GRID}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label={name}
      className={cn("flex-shrink-0", className)}
    >
      {botPixels(look).map((r, i) => (
        <rect key={i} x={r.x} y={r.y} width={r.w} height={r.h} fill={fill[r.tone]} />
      ))}
    </svg>
  );
}
