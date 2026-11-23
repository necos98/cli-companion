//! Raggiungibilita' del proxy Headroom (`GET /health`), con cache breve: il
//! controllo lancia `curl` e si ripete a ogni apertura di un terminale.

use crate::net::run_curl;
use crate::ttl_cache::TtlCache;
use std::sync::OnceLock;
use std::time::Duration;

const CACHE_TTL: Duration = Duration::from_secs(3);
const TIMEOUT_SECS: &str = "1";

fn cache() -> &'static TtlCache<u16, bool> {
    static CACHE: OnceLock<TtlCache<u16, bool>> = OnceLock::new();
    CACHE.get_or_init(|| TtlCache::new(CACHE_TTL))
}

/// Il proxy sulla porta `port` risponde? Bloccante (processo `curl`).
pub fn is_reachable(port: u16) -> bool {
    if let Some(known) = cache().get(&port) {
        return known;
    }
    let reachable = probe(port);
    cache().put(port, reachable);
    reachable
}

/// Come `is_reachable` ma senza leggere la cache (la aggiorna): serve ad
/// attendere l'avvio del proxy.
pub fn probe_fresh(port: u16) -> bool {
    let reachable = probe(port);
    cache().put(port, reachable);
    reachable
}

fn probe(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    let args = ["-s", "-f", "--max-time", TIMEOUT_SECS, url.as_str()].map(String::from);
    run_curl(&args).is_ok()
}

/// Dimentica gli esiti noti (le impostazioni sono cambiate).
pub fn invalidate() {
    cache().clear();
}
