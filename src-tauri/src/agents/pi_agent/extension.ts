import { spawn } from "node:child_process"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const NOTIFY_COMMAND: string[] = __NOTIFY_COMMAND__
const AGENT_ID = __AGENT_ID__
const MCP_SERVER = __MCP_SERVER__
const MCP_COMMAND: string[] = __MCP_COMMAND__
const HOOK_TIMEOUT_MS = 10000
const STDOUT_MAX = 65536
const TEXT_MAX = 300
const PROMPT_MAX = 2000

const HERE = dirname(fileURLToPath(import.meta.url))

/** Active only in a terminal managed by cli-companion. */
const active = (): boolean => Boolean(process.env.CLI_COMPANION_SESSION)

const text = (value: unknown, max = TEXT_MAX): string | undefined =>
  typeof value === "string" && value.trim() !== "" ? value.trim().slice(0, max) : undefined

const notifyArgs = (event: string): string[] => [...NOTIFY_COMMAND.slice(1), "--notify", event, AGENT_ID]

function notify(event: string, payload: Record<string, unknown>): Promise<string> {
  return new Promise((resolve) => {
    let out = ""
    let settled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const done = () => {
      if (settled) return
      settled = true
      if (timer) clearTimeout(timer)
      resolve(out)
    }
    try {
      const child = spawn(NOTIFY_COMMAND[0], notifyArgs(event), { stdio: ["pipe", "pipe", "ignore"], windowsHide: true })
      timer = setTimeout(() => {
        child.kill()
        done()
      }, HOOK_TIMEOUT_MS)
      child.stdout?.on("data", (chunk) => {
        if (out.length < STDOUT_MAX) out += String(chunk)
      })
      child.on("error", done)
      child.on("close", done)
      child.stdin?.on("error", () => {})
      child.stdin?.end(JSON.stringify(payload))
    } catch {
      done()
    }
  })
}

/** Text of a block decision written by --notify on stdout, if any. */
function continuation(stdout: string): string | undefined {
  const line = stdout.trim().split(/\r?\n/).pop() ?? ""
  try {
    return text(JSON.parse(line)?.continue, PROMPT_MAX)
  } catch {
    return undefined
  }
}

const sessionId = (ctx: any): string | undefined => {
  try {
    return text(ctx?.sessionManager?.getSessionId?.(), 200)
  } catch {
    return undefined
  }
}

const transcriptPath = (ctx: any): string | undefined => {
  try {
    return text(ctx?.sessionManager?.getSessionFile?.(), 1024)
  } catch {
    return undefined
  }
}

export default function (pi: any) {
  if (!active()) return
  try {
    pi.registerMcpServer(MCP_SERVER, { command: MCP_COMMAND[0], args: MCP_COMMAND.slice(1), exposure: "direct" })
  } catch {}

  pi.on("resources_discover", async () => ({ skillPaths: [join(HERE, "skills")] }))

  let continuing = false

  pi.on("session_start", async (e: any, ctx: any) => {
    try {
      const id = sessionId(ctx)
      if (!id) return
      await notify("session-start", { session_id: id, source: e?.reason, transcript_path: transcriptPath(ctx) })
    } catch {}
  })

  pi.on("agent_before_settle", async (e: any, ctx: any) => {
    try {
      const id = sessionId(ctx)
      if (!id) return
      if (e?.outcome !== "completed") {
        continuing = false
        const message = e?.outcome === "aborted" ? "The run was interrupted." : "The run ended with an error."
        await notify("needs-input", { session_id: id, wait: "other", message })
        return
      }
      const prompt = continuation(
        await notify("finished", { session_id: id, continuing, transcript_path: transcriptPath(ctx) }),
      )
      if (prompt) {
        continuing = true
        pi.sendUserMessage(prompt, { deliverAs: "followUp" })
        return { continue: true }
      }
      continuing = false
    } catch {
      continuing = false
    }
  })

  pi.on("ui_prompt_start", async (e: any, ctx: any) => {
    try {
      const id = sessionId(ctx)
      if (!id) return
      await notify("needs-input", { session_id: id, wait: "question", message: text(e?.title) })
    } catch {}
  })

  pi.on("session_shutdown", async (e: any, ctx: any) => {
    try {
      const id = sessionId(ctx)
      if (!id) return
      await notify("session-end", { session_id: id, reason: e?.reason })
    } catch {}
  })
}
