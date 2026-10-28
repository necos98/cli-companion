//! Wrapper di `agent-browser`: un processo per comando, argomenti sempre come argv.
//! Generico: nessuna logica dell'esperimento.

mod envelope;
pub mod session;
pub mod snapshot;

pub use envelope::BrowserError;

use crate::proc::output_with_timeout;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const OPEN_TIMEOUT: Duration = Duration::from_secs(60);
const SETTLED_ARGS: [&str; 3] = ["wait", "--load", "networkidle"];

pub struct AgentBrowser {
    exe: PathBuf,
    session: String,
    headed: bool,
}

impl AgentBrowser {
    pub fn new(exe: PathBuf, session: String, headed: bool) -> Self {
        AgentBrowser {
            exe,
            session,
            headed,
        }
    }

    pub fn open(&self, url: &str) -> Result<(), BrowserError> {
        self.run(&["open", url], OPEN_TIMEOUT).map(drop)
    }

    pub fn click(&self, element: &str) -> Result<(), BrowserError> {
        self.run(&["click", &at_ref(element)], COMMAND_TIMEOUT)
            .map(drop)
    }

    pub fn fill(&self, element: &str, text: &str) -> Result<(), BrowserError> {
        self.run(&["fill", &at_ref(element), text], COMMAND_TIMEOUT)
            .map(drop)
    }

    pub fn select(&self, element: &str, value: &str) -> Result<(), BrowserError> {
        self.run(&["select", &at_ref(element), value], COMMAND_TIMEOUT)
            .map(drop)
    }

    pub fn press(&self, key: &str) -> Result<(), BrowserError> {
        self.run(&["press", key], COMMAND_TIMEOUT).map(drop)
    }

    /// Attende che la rete sia quieta. L'errore e' non fatale: lo decide il chiamante.
    pub fn wait_settled(&self) -> Result<(), BrowserError> {
        self.run(&SETTLED_ARGS, COMMAND_TIMEOUT).map(drop)
    }

    pub fn url(&self) -> Result<String, BrowserError> {
        self.string_field(&["get", "url"], "url")
    }

    pub fn title(&self) -> Result<String, BrowserError> {
        self.string_field(&["get", "title"], "title")
    }

    pub fn page_text(&self) -> Result<String, BrowserError> {
        self.string_field(&["get", "text", "body"], "text")
    }

    /// `data` grezzo di `snapshot -i -c [--delta]`.
    pub fn snapshot(&self, delta: bool) -> Result<Value, BrowserError> {
        let mut args = vec!["snapshot", "-i", "-c"];
        if delta {
            args.push("--delta");
        }
        self.run(&args, COMMAND_TIMEOUT)
    }

    /// Chiude il browser della sessione (mai `close --all`).
    pub fn close(&self) -> Result<(), BrowserError> {
        self.run(&["close"], COMMAND_TIMEOUT).map(drop)
    }

    fn string_field(&self, args: &[&str], field: &str) -> Result<String, BrowserError> {
        let data = self.run(args, COMMAND_TIMEOUT)?;
        data.get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| BrowserError::BadOutput(format!("campo `{field}` assente")))
    }

    fn run(&self, args: &[&str], timeout: Duration) -> Result<Value, BrowserError> {
        let mut command = Command::new(&self.exe);
        command.args(["--session", &self.session]);
        if self.headed {
            command.arg("--headed");
        }
        command.arg("--json").args(args);
        let started = Instant::now();
        let output = output_with_timeout(&mut command, timeout).map_err(|reason| {
            if started.elapsed() >= timeout {
                BrowserError::Timeout
            } else {
                BrowserError::Spawn(reason)
            }
        })?;
        envelope::parse_output(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
    }
}

fn at_ref(element: &str) -> String {
    format!("@{element}")
}
