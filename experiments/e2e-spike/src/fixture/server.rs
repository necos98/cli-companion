//! Listener TCP della fixture: un thread per connessione, solo 127.0.0.1.

use super::http::{read_request, Request, Response};
use super::state::Store;
use super::{api, pages};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const HOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

pub struct FixtureServer {
    stop: Arc<AtomicBool>,
    addr: SocketAddr,
    base_url: String,
    accept_thread: Option<JoinHandle<()>>,
}

impl FixtureServer {
    /// Porta 0 = porta effimera scelta dal sistema.
    pub fn start(port: u16) -> Result<Self, String> {
        let listener = bind(port)?;
        let addr = local_addr(&listener)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let accept_thread = thread::Builder::new()
            .name("fixture-accept".into())
            .spawn(move || accept_loop(&listener, &flag))
            .map_err(|e| format!("avvio del thread della fixture fallito: {e}"))?;
        Ok(FixtureServer {
            stop,
            addr,
            base_url: format!("http://{addr}"),
            accept_thread: Some(accept_thread),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Sblocca la accept() in corso; se fallisce il thread muore con il processo.
        let _ = TcpStream::connect_timeout(&self.addr, IO_TIMEOUT);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Ascolta finche' il processo vive (modalita' `serve-fixture`).
pub fn serve_forever(port: u16) -> Result<(), String> {
    let listener = bind(port)?;
    let addr = local_addr(&listener)?;
    println!("Fixture in ascolto su http://{addr} (Ctrl+C per uscire)");
    accept_loop(&listener, &AtomicBool::new(false));
    Ok(())
}

fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind((HOST, port)).map_err(|e| format!("bind di {HOST}:{port} fallito: {e}"))
}

fn local_addr(listener: &TcpListener) -> Result<SocketAddr, String> {
    listener
        .local_addr()
        .map_err(|e| format!("indirizzo locale della fixture non disponibile: {e}"))
}

fn accept_loop(listener: &TcpListener, stop: &AtomicBool) {
    let store = Arc::new(Mutex::new(Store::default()));
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let store = Arc::clone(&store);
        // Se il thread non parte la connessione viene chiusa: il client riprova.
        let _ = thread::Builder::new()
            .name("fixture-conn".into())
            .spawn(move || handle_connection(stream, &store));
    }
}

fn handle_connection(mut stream: TcpStream, store: &Mutex<Store>) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let response = match read_request(&mut stream) {
        Ok(request) => dispatch(store, &request),
        Err(e) => e.to_response(),
    };
    // Il client puo' aver chiuso la connessione: non c'e' nulla da fare.
    let _ = response.write_to(&mut stream);
}

fn dispatch(store: &Mutex<Store>, req: &Request) -> Response {
    api::handle(store, req)
        .or_else(|| pages::handle(store, req))
        .unwrap_or_else(|| Response::text(404, "Not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn raw_exchange(server: &FixtureServer, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(server.addr).unwrap();
        stream.write_all(request).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_404_400_and_413_over_tcp() {
        let server = FixtureServer::start(0).unwrap();
        assert!(raw_exchange(&server, b"GET /nope HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(raw_exchange(&server, b"BAD\r\n\r\n").starts_with("HTTP/1.1 400"));
        let big = b"POST /api/login HTTP/1.1\r\nContent-Length: 999999\r\n\r\n";
        assert!(raw_exchange(&server, big).starts_with("HTTP/1.1 413"));
        server.stop();
    }

    #[test]
    fn stop_releases_the_port() {
        let server = FixtureServer::start(0).unwrap();
        let addr = server.addr;
        assert!(server.base_url().starts_with("http://127.0.0.1:"));
        server.stop();
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
    }

    #[test]
    fn stalled_client_is_dropped_after_timeout() {
        let server = FixtureServer::start(0).unwrap();
        let mut stream = TcpStream::connect(server.addr).unwrap();
        stream.write_all(b"GET / HTT").unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 400"), "{out}");
    }
}
