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

/** Variables set by cli-companion when the session is routed through the Headroom proxy. */
const PROXY_URL_VAR = "COMPANION_PROXY_URL"
const PROXY_UPSTREAM_VAR = "COMPANION_PROXY_OPENAI_UPSTREAM"
const ANTHROPIC_UPSTREAM = "https://api.anthropic.com"
const OPENAI_APIS = ["openai-completions", "openai-responses"]

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

/** URL without case, trailing slashes and a trailing /v1, to compare upstreams. */
const normalizeUrl = (url: string): string =>
  url.trim().toLowerCase().replace(/\/+$/, "").replace(/\/v1$/, "").replace(/\/+$/, "")

/**
 * Points the providers whose upstream is the one the proxy forwards to at the proxy
 * (only `baseUrl`: models, auth and API of each provider stay as they are).
 * Anthropic Messages clients append /v1/messages, OpenAI clients append the route
 * (/chat/completions, /responses) to a base that already ends in /v1.
 */
function redirectProviders(pi: any, ctx: any): void {
  const proxy = text(process.env[PROXY_URL_VAR], 300)?.replace(/\/+$/, "")
  if (!proxy) return
  const openai = normalizeUrl(text(process.env[PROXY_UPSTREAM_VAR], 300) ?? "https://api.openai.com")
  const done = new Set<string>()
  for (const model of ctx?.modelRegistry?.getAll?.() ?? []) {
    try {
      const provider = text(model?.provider, 200)
      const base = text(model?.baseUrl, 500)
      if (!provider || !base || done.has(provider)) continue
      let target: string | undefined
      if (model.api === "anthropic-messages" && normalizeUrl(base) === ANTHROPIC_UPSTREAM) target = proxy
      else if (OPENAI_APIS.includes(model.api) && normalizeUrl(base) === openai) target = `${proxy}/v1`
      if (!target) continue
      done.add(provider)
      pi.registerProvider(provider, { baseUrl: target })
    } catch {}
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
      redirectProviders(pi, ctx)
    } catch {}
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
