import { truncateReport } from "../../lib/automation";
import type { BotActivity, BotPose } from "../../lib/office";

export type BubbleTone = "task" | "alert" | "danger" | "success" | "chat";

export interface Bubble {
  text: string;
  tone: BubbleTone;
}

/** Lunghezza massima del titolo del task nel fumetto. */
const TASK_TITLE_MAX = 24;
/** Simboli delle chiacchierate: ognuno ne ha uno suo, scelto dal nome. */
const CHAT_GLYPHS = ["…", "?", "!", "♪", "☕"] as const;

/** Simbolo stabile per un bot (stesso nome = stesso simbolo). */
function chatGlyph(name: string): string {
  let sum = 0;
  for (let i = 0; i < name.length; i++) sum += name.charCodeAt(i);
  return CHAT_GLYPHS[sum % CHAT_GLYPHS.length];
}

/**
 * Il fumetto sopra la testa del bot, se ne ha uno: il titolo del task per chi
 * lavora per un worker, un segnale per attenzione / fallimento / consegna, un
 * simbolo per chi chiacchiera.
 */
export function botBubble(
  activity: BotActivity,
  pose: BotPose,
  name: string,
  taskTitle: string | undefined,
): Bubble | null {
  switch (activity) {
    case "working":
      return taskTitle ? { text: truncateReport(taskTitle, TASK_TITLE_MAX), tone: "task" } : null;
    case "attention":
      return { text: "❗", tone: "alert" };
    case "failed":
      return { text: "✕", tone: "danger" };
    case "completed":
      return { text: "✓", tone: "success" };
    case "idle":
      return pose === "chatting" ? { text: chatGlyph(name), tone: "chat" } : null;
    default:
      return null;
  }
}
