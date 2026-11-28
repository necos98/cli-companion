//! Contratto della delega dell'esplorazione: un agente che sa far partire un
//! sotto-agente economico e di sola lettura puo' lasciargli i giri larghi nel
//! repository, tenendo nel proprio contesto solo le conclusioni. La politica
//! (a quali task e con quali parole) sta nel core; qui solo la meccanica.
//! Un'integrazione che non lo dichiara non riceve il paragrafo.

/// Come un agente avvia il suo esploratore.
pub trait ExplorationDelegation: Sync {
    /// Come il worker avvia l'esploratore, come frase che completa
    /// "To delegate, ...": per esempio "call the Agent tool with
    /// subagent_type `cli-companion-notify:explorer`". Una sola riga, senza `\r`.
    fn explorer_invocation(&self) -> String;
}
