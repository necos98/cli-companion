//! Contratto dell'instradamento su un proxy del provider (per esempio un proxy
//! locale che comprime il contesto): come un agente sceglie l'endpoint delle
//! sue richieste. Il core non sa come ogni agente lo decide: gli passa dove sta
//! il proxy e riceve le variabili d'ambiente da mettere al lancio.
//! Un'integrazione che non lo dichiara parte sempre diretta.

/// Dove sta il proxy e a quale upstream OpenAI inoltra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyTarget {
    /// Base URL del proxy, `http://127.0.0.1:<porta>`, senza `/` finale.
    pub base_url: String,
    /// Upstream OpenAI effettivo del proxy (senza `/v1` e senza `/` finale):
    /// serve a riconoscere i provider da ridirezionare.
    pub openai_upstream: String,
}

/// Instradamento di un agente sul proxy.
pub trait ProxyRouting: Sync {
    /// Variabili d'ambiente da impostare sul processo dell'agente perche' usi
    /// il proxy. `None` = questo agente non e' instradabile (la sessione parte
    /// diretta, senza avvisi).
    fn proxy_env(&self, proxy: &ProxyTarget) -> Option<Vec<(String, String)>>;
}
