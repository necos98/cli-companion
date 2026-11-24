# Headroom per le sessioni Pi — analisi e raccomandazione

> Solo analisi: nessuna modifica al codice dell'app in questo task.
> Ambiente verificato: Windows, `pi` 1.6.0 (`@earendil-works/pi-coding-agent`),
> `headroom` 0.45.0 (`C:\Users\jacob\.local\bin\headroom.exe`).
> Tutti i path dei pacchetti citati si intendono sotto
> `C:\Users\jacob\AppData\Roaming\npm\node_modules\@earendil-works\` (globale npm).

## Sintesi

- **Pi NON legge `ANTHROPIC_BASE_URL` né `OPENAI_BASE_URL`** per l'URL del
  provider: l'endpoint è deciso dal campo `baseUrl` del provider (default
  cablato nel pacchetto, override solo da `models.json` o da un'estensione).
  L'unica variabile d'ambiente "base URL" riconosciuta è `AZURE_OPENAI_BASE_URL`
  (solo Azure). Verificato nel codice e con una prova negativa.
- **Un'estensione Pi può cambiare il `baseUrl` a runtime** con
  `pi.registerProvider("anthropic", { baseUrl: "…" })` (o `"openai"`), senza
  toccare `models.json`. Verificato nel codice, nella documentazione e con una
  prova pratica positiva.
- **Headroom espone sia l'API Anthropic sia il formato OpenAI** (e route
  passthrough per Gemini/Vertex): `/v1/messages`, `/v1/chat/completions`,
  `/v1/responses`. Verificato dall'output di `headroom proxy`.
- **Prova pratica riuscita**: un `pi -p` instradato con `models.json` temporaneo
  e, in un secondo test, con un'estensione `-e`, ha inviato `POST /v1/messages`
  al proxy locale, che l'ha inoltrato ad `api.anthropic.com` (risposta 401 da
  upstream perché la chiave era finta, ma la richiesta è passata dal proxy).
- **Raccomandazione**: aggiungere un contratto core neutro (es. `ProxyRouting`)
  che restituisce le variabili d'ambiente con cui un agente sceglie l'endpoint
  del proxy. Claude Code restituisce `ANTHROPIC_BASE_URL`; Pi restituisce una
  variabile letta dalla sua estensione `cli-companion-notify`, che a sua volta
  chiama `pi.registerProvider("anthropic", { baseUrl })`. Il core smette di
  fare `agent_id == CLAUDE_CODE_ID`.

---

## 1. Pi legge `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`?

**No.** Il modo in cui Pi sceglie l'URL del provider è il campo `baseUrl` del
provider, non una variabile d'ambiente dedicata.

Prove (verificate nel pacchetto installato):

| Prova | Dove | Cosa dice |
| --- | --- | --- |
| Il provider Anthropic ha un default cablato | `node_modules/@earendil-works/pi-ai/dist/providers/anthropic.js:65` | `baseUrl: "https://api.anthropic.com"` |
| Il provider OpenAI ha un default cablato | `node_modules/@earendil-works/pi-ai/dist/providers/openai.js:11` | `baseUrl: "https://api.openai.com/v1"` |
| Il client Anthropic usa sempre `model.baseUrl` | `node_modules/@earendil-works/pi-ai/dist/api/anthropic-messages.js:972,987,1017,1028` | `baseURL: model.baseUrl` in `createClient` |
| Il client OpenAI Chat Completions usa `model.baseUrl` | `node_modules/@earendil-works/pi-ai/dist/api/openai-completions.js:544` | `baseURL: model.baseUrl` |
| Il client OpenAI Responses usa `model.baseUrl` | `node_modules/@earendil-works/pi-ai/dist/api/openai-responses.js:236` | `baseURL: model.baseUrl` |

Il riferimento a `ANTHROPIC_BASE_URL` esiste solo **dentro l'SDK Anthropic
vendored** (`dist/bundle/chunks/chunk-7CBJINYJ.js`): il costruttore della classe
`BaseAnthropic` ha `baseURL = readEnv("ANTHROPIC_BASE_URL")` come valore di
default. Ma Pi costruisce il client passando **esplicitamente**
`baseURL: model.baseUrl`, quindi quel default dell'SDK non viene mai usato:
`ANTHROPIC_BASE_URL` è di fatto ignorata dal provider Anthropic di Pi.

Per OpenAI la ricerca di `OPENAI_BASE_URL` nel `dist` trova solo occorrenze di
`AZURE_OPENAI_BASE_URL` (substring): non esiste una lettura autonoma di
`OPENAI_BASE_URL`.

Anche l'help di `pi` e la documentazione lo confermano:

- `pi --help` (sezione "Environment Variables") elenca solo chiavi API e
  `AZURE_OPENAI_BASE_URL` / `AZURE_OPENAI_RESOURCE_NAME` / `AZURE_OPENAI_API_VERSION`;
  nessun `ANTHROPIC_BASE_URL` o `OPENAI_BASE_URL` generico.
- `docs/environment-variables.md`: per l'outbound HTTP ci sono solo
  `HTTP_PROXY` / `HTTPS_PROXY` (proxy di rete, non endpoint del modello).
- `docs/providers.md`: per Anthropic sono documentati `ANTHROPIC_API_KEY`,
  `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_OAUTH_TOKEN` e la workload identity
  federation; nessuna variabile base URL. L'unica base URL via ambiente è
  `AZURE_OPENAI_BASE_URL` (Azure).

**Conclusione verificata**: impostare `ANTHROPIC_BASE_URL` o `OPENAI_BASE_URL`
non instrada Pi sul proxy. Il punto 4 lo conferma con una prova negativa.

---

## 2. Un'estensione Pi può cambiare il `baseUrl` a runtime?

**Sì.** L'API pubblica delle estensioni espone `registerProvider`, con un
`ProviderConfig` che ammette `baseUrl`. Si può "ridirezionare" un provider
esistente registrando solo i campi da cambiare, conservando catalogo modelli,
autenticazione e implementazione API built-in.

Prove:

- Tipo `ExtensionAPI` (pubblico): `dist/core/extensions/types.d.ts:1404`
  `registerProvider(name: string, config: ProviderConfig): void;`
  e `:1418` `unregisterProvider(name: string): void;`.
- `ProviderConfig` (pubblico): `dist/core/extensions/types.d.ts:1459-1463`,
  campo `baseUrl?: string;` ("Base URL for the API endpoint").
- Esempio nel docblock: `dist/core/extensions/types.d.ts:1385` commenta
  `// Override baseUrl for an existing provider` con
  `pi.registerProvider("anthropic", { baseUrl: "https://proxy.example.com" });`.
- `docs/custom-provider.md`, sezione "Redirect an existing provider":
  registrare solo i campi da cambiare mantiene cataloghi/autenticazione/API.
- `docs/models.md`, sezione "Configure a compatible endpoint":
  "To route a built-in provider through a proxy, set only its `baseUrl`".

Verifica pratica (punto 4): un'estensione minima caricata con `pi -e` che fa
`pi.registerProvider("anthropic", { baseUrl: "http://127.0.0.1:8791" })` ha
instradato la richiesta sul proxy.

Per cli-companion l'alternativa `models.json` **non è perseguibile**: le regole
dell'integrazione Pi (`agents/pi_agent/mod.rs`) impongono di non modificare mai
`settings.json` o `models.json` dell'utente. L'estensione `cli-companion-notify`
è invece già installata dall'app e non tocca la configurazione: è la strada
compatibile con il vincolo.

Nota di meccanica: la registrazione fatta **nella factory dell'estensione**
(prima dell'avvio della sessione) è disponibile per la selezione dei modelli
all'avvio; dopo il load iniziale `registerProvider` ha effetto immediato
(`dist/core/extensions/runner.js:312-328`, commento "From this point on,
provider registration/unregistration takes effect immediately").

---

## 3. Headroom supporta solo Anthropic o anche OpenAI/altri?

**Entrambi.** L'avvio del proxy stampa la tabella di routing; per il test ho
usato `--no-optimize` (passthrough), ma le route sono le stesse:

```text
Routing:
  /v1/messages                    → https://api.anthropic.com
  /v1/chat/completions            → https://api.openai.com
  /v1/responses                   → https://api.openai.com  (HTTP + WebSocket)
  /v1internal:streamGenerateContent → https://cloudcode-pa.googleapis.com
  /v1/projects/.../publishers/... → https://us-central1-aiplatform.googleapis.com
```

`headroom proxy --help` documenta esplicitamente i due utilizzi principali:

```text
Usage with Claude Code:
    ANTHROPIC_BASE_URL=http://localhost:8787 claude

Usage with OpenAI-compatible clients:
    OPENAI_BASE_URL=http://localhost:8787/v1 your-app
```

In più `--backend` accetta `anthropic` (direct), `bedrock` (AWS), `openrouter`,
`anyllm`, `litellm-<provider>`; esistono opzioni per reindirizzare gli upstream
(`--anthropic-api-url`, `--openai-api-url`, `--gemini-api-url`,
`--vertex-api-url`, …). Quindi Headroom non è un proxy solo-Anthropic: espone
sia il protocollo Anthropic Messages sia OpenAI Chat Completions/Responses (più
route passthrough per Gemini e Vertex).

Endpoints di servizio: `GET /livez`, `/readyz`, `/health`, `/stats`,
`/stats-history`, `/metrics`.

---

## 4. Prova pratica

Obiettivo: avviare `headroom proxy` su una porta libera, far passare una
richiesta di `pi` e osservarla dal lato proxy.

### Setup

```text
headroom proxy --port 8791 --no-optimize --log-messages --log-file ./proxy.log
```

Proxy attivo su `http://127.0.0.1:8791` (verificato con `GET /livez` → 200).
Ho usato la porta 8791 per non toccare il proxy dell'app già in esecuzione
sulla porta configurata (8787).

Chiave Anthropic finta `sk-ant-...` per far arrivare Pi fino alla chiamata HTTP
(senza chiave Pi si ferma prima, con "No API key for provider: anthropic").
L'upstream reale risponde 401 "invalid x-api-key": costa zero ed è la conferma
che la richiesta è stata inoltrata.

### Test A — override con `models.json` in una `PI_CODING_AGENT_DIR` temporanea

```text
agent/models.json:
{ "providers": { "anthropic": { "baseUrl": "http://127.0.0.1:8791" } } }

PI_CODING_AGENT_DIR=<tmp>/agent ANTHROPIC_API_KEY=sk-ant-fake PI_OFFLINE=1 \
  pi -p --no-session "Rispondi solo: OK"
```

Output di `pi`:

```text
401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}, ...}
```

Il log del proxy mostra la richiesta **in entrata** e l'inoltro **in uscita**:

```text
event=proxy_inbound_request method=POST path=/v1/messages ...
  headers={"user-agent": "pi/1.6.0 (win32; node/v24.13.1; x64)", ...}
event=outbound_request forwarder=streaming method=POST
  path=https://api.anthropic.com/v1/messages ... source=passthrough
Upstream provider returned an error; forwarding it to the client
  status=401 url=https://api.anthropic.com/v1/messages
```

→ il percorso `pi → proxy → api.anthropic.com` funziona.

### Test B — override con un'estensione `-e` (senza `models.json`)

```text
redirect.ts:
export default function (pi: any) {
  pi.registerProvider("anthropic", { baseUrl: "http://127.0.0.1:8791" });
}

pi -p --no-session -e ./redirect.ts "Rispondi solo: OK"
```

Seconda richiesta `POST /v1/messages` registrata dal proxy (stesso
`user-agent: pi/1.6.0`, stesso inoltro e 401 upstream).

→ l'API `registerProvider` ridireziona il provider Anthropic a runtime, senza
toccare `models.json`.

### Test C — prova negativa: `ANTHROPIC_BASE_URL` da sola non instrada

```text
PI_CODING_AGENT_DIR=<tmp vuoto> ANTHROPIC_BASE_URL=http://127.0.0.1:8791 \
  ANTHROPIC_API_KEY=sk-ant-fake PI_OFFLINE=1 pi -p --no-session "Rispondi solo: OK"
```

`pi` riceve comunque il 401, ma il proxy **non registra nessuna nuova richiesta**
(il conteggio di `POST /v1/messages` in entrata resta invariato). Pi ha chiamato
direttamente `api.anthropic.com`.

→ conferma del punto 1: `ANTHROPIC_BASE_URL` è ignorata dal provider Anthropic
di Pi.

### Pulizia

Ho terminato **solo** il proxy che ho avviato io (porta 8791); il proxy
dell'app sulla 8787 è rimasto attivo e intatto.

---

## 5. Come estendere il core senza nominare agenti

Il vincolo architetturale (`agents/architecture.rs`, `tests/architecture.test.mjs`)
impedisce di scrivere nel core un `if agent == "pi-agent"` o di conoscere i
dettagli di Pi. Oggi `headroom/launch.rs` fa proprio questo con Claude Code
(`agent_id != CLAUDE_CODE_ID`, righe 5, 81) e con la costante
`BASE_URL_VAR = "ANTHROPIC_BASE_URL"` (riga 10).

### Contratto proposto: `ProxyRouting`

Nuovo file `agents/contracts/proxy_routing.rs` (stesso schema degli altri
contratti: un trait + i tipi, nessun default "non supportato"):

```rust
/// Come un agente si fa instradare verso un proxy del provider.
pub trait ProxyRouting: Sync {
    /// Variabili d'ambiente da impostare sul processo dell'agente affinché
    /// usi `base_url` come endpoint del provider. `None` = l'agente non è
    /// instradabile da questo proxy (la sessione parte diretta).
    fn proxy_env(&self, base_url: &str) -> Option<Vec<(String, String)>>;
}
```

Aggiunte collegate (meccaniche, come per ogni nuovo contratto):

- `agents/contracts/mod.rs`: `mod proxy_routing;` + re-export.
- `agents/contracts/capabilities.rs`: campo
  `pub proxy_routing: Option<&'a dyn ProxyRouting>`, variante
  `Capability::ProxyRouting`, aggiornamento di `declared()` e del test di
  serializzazione.
- `agents/conformance.rs`: aggiungere `proxy_routing` alla lista `present`
  in `core()` (così `declared()` resta coerente) e, volendo, una verifica che
  le variabili restituite superino `is_valid_env_name` / `is_valid_env_value`.
- `agents/testing.rs` (e `test_registry.rs`): aggiungere il campo ai
  `FakeIntegration` usati nei test, come per gli altri contratti.

### Modifica a `headroom/launch.rs`

`launch_env` smette di confrontare `agent_id` con una costante e chiede il
contratto all'integrazione risolta:

```rust
pub fn launch_env(agent_id: &str, kind: TargetKind, patience: Patience) -> HeadroomLaunch {
    let settings = settings::current();
    if !settings.enabled || kind != TargetKind::Windows {
        return HeadroomLaunch::default();
    }
    // Risoluzione dell'agente e del suo contratto di instradamento.
    let Some(proxy) = crate::agents::registry::find(agent_id).ok()
        .and_then(|i| i.capabilities().proxy_routing)
    else {
        return HeadroomLaunch::default();
    };
    let Some(env) = proxy.proxy_env(&settings.base_url()) else {
        return HeadroomLaunch::default();
    };
    // "Già impostato" generico: per ogni variabile restituita.
    if let Some((name, _)) = env.iter().find(|(n, _)| env_override(n).is_some()) {
        return HeadroomLaunch::warn(format!(
            "Headroom is enabled but {name} is already set; the session starts without the Headroom proxy."
        ));
    }
    let url = settings.base_url();
    match ensure_proxy(&settings, patience) {
        Ensure::Ready => {}
        Ensure::Starting => return HeadroomLaunch::warn(/* come oggi */),
        Ensure::Failed(err) => return HeadroomLaunch::warn(unreachable_warning(&settings, &url, &err)),
    }
    HeadroomLaunch { env, warning: None }
}
```

`decide` resta la parte pura testabile, ma invece di `agent_id` e
`parent_base_url` riceve l'esito del contratto (o una closure), così i test
unitari coprono: disabilitato, non-Windows, agente senza contratto, contratto
che dà `None`, variabile già presente, proxy irraggiungibile/che parte, proxy
pronto. `merge_env` non cambia.

### Modifica a Claude Code

Nuovo file `agents/claude_code/proxy_routing.rs` che sposta la conoscenza
specifica (oggi in `headroom/launch.rs`) dentro l'integrazione:

```rust
impl ProxyRouting for ClaudeCode {
    fn proxy_env(&self, base_url: &str) -> Option<Vec<(String, String)>> {
        Some(vec![("ANTHROPIC_BASE_URL".to_string(), base_url.to_string())])
    }
}
```

Aggiungere `proxy_routing: Some(self)` in `capabilities()` di `claude_code` e
rimuovere `BASE_URL_VAR`/`CLAUDE_CODE_ID` da `headroom/launch.rs` (il warning
diventa generico, usa il nome restituito dal contratto).

### Modifica a Pi

Nuovo file `agents/pi_agent/proxy_routing.rs`:

```rust
/// Variabile letta dall'estensione `cli-companion-notify` per ridirezionare il
/// provider Anthropic (Pi non legge ANTHROPIC_BASE_URL/OPENAI_BASE_URL).
const HEADROOM_BASE_URL: &str = "CLI_COMPANION_HEADROOM_BASE_URL";

impl ProxyRouting for PiAgent {
    fn proxy_env(&self, base_url: &str) -> Option<Vec<(String, String)>> {
        Some(vec![(HEADROOM_BASE_URL.to_string(), base_url.to_string())])
    }
}
```

Aggiungere `proxy_routing: Some(self)` in `capabilities()` di `pi_agent`.

Modifica a `agents/pi_agent/extension.ts`: all'inizio della factory (dopo il
guard `active()`), leggere la variabile e registrare il redirect:

```ts
export default function (pi: any) {
  if (!active()) return
  const headroom = process.env.CLI_COMPANION_HEADROOM_BASE_URL
  if (headroom) {
    try { pi.registerProvider("anthropic", { baseUrl: headroom }) } catch {}
  }
  // ... resto invariato (registerMcpServer, eventi, ecc.)
}
```

Perché funziona: la factory dell'estensione viene eseguita all'avvio di Pi, e
`registerProvider` in quel momento è disponibile per la selezione dei modelli
(punto 2). L'estensione è già installata dall'integrazione Pi in
`<agent dir>/extensions/cli-companion-notify/`; nessun file di configurazione
di Pi (`settings.json`, `models.json`) viene toccato. Fuori da un terminale
gestito (`CLI_COMPANION_SESSION` assente) l'estensione resta inerte, quindi un
`pi` normale dell'utente non cambia.

Scelta del nome della variabile: `CLI_COMPANION_HEADROOM_BASE_URL` è coerente
con `CLI_COMPANION_SESSION` (variabile del core già letta dall'estensione) e
chiarisce che è un valore impostato dall'app, non dall'utente. In alternativa
un nome neutro `HEADROOM_PROXY_URL`; da decidere in fase di implementazione,
tenendo presente che la variabile passa per `WSLENV` su WSL.

### Cosa fare per WSL

Oggi Headroom si applica solo a `TargetKind::Windows` (`launch.rs:81`) e la
proposta mantiene quel limite. Il motivo: `settings.base_url()` è
`http://127.0.0.1:<porta>`, ma dentro WSL2 `127.0.0.1` è la VM Linux, non
l'host Windows dove gira il proxy. Per WSL servirebbe:

- risolvere l'indirizzo dell'host Windows dal lato WSL (IP dell'interfaccia
  `vEthernet (WSL)`, o `host.docker.internal` con networking "mirrored", o il
  nameserver di `/etc/resolv.conf`), oppure
- usare il socket UDS (`headroom proxy --uds`, POSIX-only) esposto in una
  directory condivisa `/mnt/…`, se praticabile.

È lo stesso limite già esistente per Claude Code, quindi Pi non introduce nulla
di nuovo: WSL resta un follow-up comune a entrambi.

---

## Raccomandazione di implementazione

1. **Aggiungere il contratto `ProxyRouting`** (trait in
   `agents/contracts/proxy_routing.rs`, campo in `Capabilities`, variante in
   `Capability`, aggiornamenti a `declared()` e `conformance.rs`).
2. **Rifattorizzare `headroom/launch.rs`** per risolvere l'integrazione via
   `registry::find(agent_id)` e usare `capabilities().proxy_routing` al posto di
   `agent_id == CLAUDE_CODE_ID`; il controllo "variabile già impostata" diventa
   generico per ogni variabile restituita.
3. **Spostare in `claude_code/`** la variabile `ANTHROPIC_BASE_URL` (nuovo
   `claude_code/proxy_routing.rs`).
4. **In `pi_agent/`** implementare `ProxyRouting` (variabile
   `CLI_COMPANION_HEADROOM_BASE_URL`) e far leggere alla `extension.ts` quella
   variabile per chiamare `pi.registerProvider("anthropic", { baseUrl })`.
5. **Ambito iniziale**: solo provider Anthropic su Windows, come oggi per
   Claude Code. L'eventuale supporto OpenAI/altri per Pi è un'estensione
   successiva (l'estensione può registrare `"openai"` con
   `baseUrl + "/v1"`, perché Headroom espone `/v1/chat/completions` e
   `/v1/responses`).
6. **Documentazione**: nella stessa modifica, aggiornare
   `docs/wiki/integrare-un-coding-agent.md` (la sezione Pi dichiara oggi
   `env` vuoto nell'automazione e non parla di Headroom) e `docs/DESIGN.MD`
   (`headroom/` non è più "solo Claude Code"). In questo task di sola analisi
   i due file non sono stati toccati: è l'unico file di questo task
   `docs/analisi/headroom-pi.md`.

### Test da aggiungere/aggiornare

- `headroom/launch.rs` (unit, sulla parte pura): i casi odierni riscritti in
  termini di contratto/closure — disabilitato, non-Windows, agente senza
  contratto → default; contratto `None` → default; variabile già presente →
  warning; proxy irraggiungibile/che parte/pronto.
- `claude_code/proxy_routing.rs` (unit): restituisce `ANTHROPIC_BASE_URL` con
  il `base_url` passato.
- `pi_agent/proxy_routing.rs` (unit): restituisce `CLI_COMPANION_HEADROOM_BASE_URL`.
- `pi_agent/extension.rs` (unit): `index.ts` generato contiene la lettura di
  `CLI_COMPANION_HEADROOM_BASE_URL` e la chiamata `registerProvider("anthropic", …)`.
- `agents/conformance.rs`: la nuova variabile supera `is_valid_env_name` /
  `is_valid_env_value` e `declared()` resta coerente.
- Test architetturali Rust e JS: nessuna nuova violazione attesa (le stringhe
  `ANTHROPIC_BASE_URL`, `CLI_COMPANION_HEADROOM_BASE_URL`, `anthropic` stanno
  dentro le cartelle delle rispettive integrazioni e non sono token di agenti).

### Rischi e note

- **Pi richiede l'estensione installata**: la variabile da sola non instrada
  Pi. Se l'estensione `cli-companion-notify` non è installata (integrazione Pi
  non installata/aggiornata), la sessione parte comunque diretta, senza avviso
  specifico. Da valutare se segnalarlo.
- **Chiave/autenticazione**: il redirect preserva l'autenticazione built-in di
  Pi (`ANTHROPIC_API_KEY`, OAuth, ecc.); non toccare `auth.json`.
- **Versione di Pi**: l'API `registerProvider(name, config)` è stabile nella
  1.x verificata (1.6.0). `version.rs` già limita l'integrazione alla major 1.
- **Path del proxy**: per Anthropic il `baseUrl` va senza `/v1` (il client
  aggiunge `/v1/messages`); per OpenAI andrebbe con `/v1`. L'estensione deve
  conoscere questa convenzione per provider.
- **Headroom in modalità default** (non `--no-optimize`) applica compressione e
  CCR: nel log di prova è comparso un warning per il tool differito
  `__pi_deferred_placeholder__` di Pi, innocuo in passthrough ma da ricontrollare
  con l'ottimizzazione attiva (comportamento di compressione su richieste Pi).
- **WSL**: fuori ambito iniziale (vedi sopra); non introdurre `127.0.0.1` per
  WSL.

---

## Cosa resta da verificare

- Prova end-to-end con Headroom in modalità **ottimizzazione attiva** (non
  passthrough) e una chiave Anthropic reale: verificare che compressione/CCR non
  rompano le richieste di Pi (in particolare il warning sul tool deferito).
- Il provider **OpenAI** di Pi via Headroom (baseUrl `http://127.0.0.1:PORT/v1`):
  qui solo dedotto dalle route di Headroom e dal codice di Pi, non provato.
- La persistenza/sovrascrittura dell'estensione: assicurarsi che un
  aggiornamento dell'estensione (marker/versione) reincluda la logica Headroom.
- Il comportamento con `PI_CODING_AGENT_DIR` personalizzata e su WSL (vedi sopra).
- Eventuale interazione con `--no-extensions` / `-ne`: se l'utente disabilita
  le estensioni, Pi non si instrada (coerente: senza estensione non c'è
  `registerProvider`).

## Distinzione verificato / dedotto

- **Verificato**: Pi ignora `ANTHROPIC_BASE_URL`/`OPENAI_BASE_URL` (codice +
  prova negativa); `registerProvider` cambia il `baseUrl` a runtime (codice +
  prova positiva); Headroom espone route Anthropic e OpenAI (output di
  `headroom proxy`); il round-trip `pi → proxy → api.anthropic.com` (log del
  proxy).
- **Dedotto** (non provato in questo task): il contratto `ProxyRouting` e i
  file da toccare (descritti, non implementati); il routing del provider OpenAI
  di Pi; il comportamento con ottimizzazione attiva; il funzionamento su WSL.
