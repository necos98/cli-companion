//! Test end-to-end della fixture via HTTP reale su una porta effimera.

use super::state::StateSnapshot;
use super::{FixtureServer, DISPLAY_NAME_SAVED, DUPLICATE_EMAIL_ERROR};
use serde_json::{json, Value};
use std::time::Duration;

struct Reply {
    status: u16,
    set_cookie: Option<String>,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    /// Valore `session=...` da rimandare come Cookie.
    fn session_cookie(&self) -> String {
        let header = self.set_cookie.clone().unwrap();
        header.split(';').next().unwrap().to_string()
    }
}

struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    fn new(server: &FixtureServer) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        Client {
            agent,
            base: server.base_url().to_string(),
        }
    }

    fn get(&self, path: &str, cookie: Option<&str>) -> Reply {
        let mut request = self.agent.get(format!("{}{path}", self.base));
        if let Some(cookie) = cookie {
            request = request.header("Cookie", cookie);
        }
        reply(request.call().unwrap())
    }

    fn post(&self, path: &str, body: &Value, cookie: Option<&str>) -> Reply {
        let mut request = self.agent.post(format!("{}{path}", self.base));
        if let Some(cookie) = cookie {
            request = request.header("Cookie", cookie);
        }
        reply(request.send_json(body).unwrap())
    }

    fn reset(&self) -> Reply {
        reply(
            self.agent
                .post(format!("{}/__reset", self.base))
                .send_empty()
                .unwrap(),
        )
    }

    fn state(&self) -> StateSnapshot {
        serde_json::from_str(&self.get("/__state", None).body).unwrap()
    }
}

fn reply(mut response: ureq::http::Response<ureq::Body>) -> Reply {
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().to_string());
    Reply {
        status: response.status().as_u16(),
        set_cookie,
        body: response.body_mut().read_to_string().unwrap(),
    }
}

fn registration(email: &str) -> Value {
    json!({
        "name": "Dana <Rivera>",
        "email": email,
        "password": "Spike-12345-pw",
        "confirm_password": "Spike-12345-pw",
        "accept_terms": true,
    })
}

#[test]
fn register_duplicate_and_welcome() {
    let server = FixtureServer::start(0).unwrap();
    let client = Client::new(&server);

    let created = client.post("/api/register", &registration("  Dana@Example.test "), None);
    assert_eq!(created.status, 201);
    let cookie = created.session_cookie();
    let welcome = client.get("/welcome", Some(&cookie));
    assert!(welcome
        .body
        .contains("<h1>Welcome, Dana &lt;Rivera&gt;</h1>"));

    let duplicate = client.post("/api/register", &registration("dana@example.test"), None);
    assert_eq!(duplicate.status, 409);
    assert_eq!(duplicate.json()["error"], DUPLICATE_EMAIL_ERROR);

    let state = client.state();
    assert_eq!(state.accounts.len(), 1);
    assert_eq!(state.accounts[0].email, "dana@example.test");
    assert_eq!(state.accounts[0].display_name, "Dana <Rivera>");
    assert!(state.logins.is_empty(), "la registrazione non e' un login");
    assert_eq!(state.active_sessions, 1);

    let mut invalid = registration("x@example.test");
    invalid["accept_terms"] = json!(false);
    assert_eq!(client.post("/api/register", &invalid, None).status, 400);
}

#[test]
fn login_settings_and_logout() {
    let server = FixtureServer::start(0).unwrap();
    let client = Client::new(&server);
    let seed = json!({"name": "Dana", "email": "dana@example.test", "password": "Spike-12345-pw"});
    assert_eq!(client.post("/__seed", &seed, None).status, 201);

    let wrong = json!({"email": "dana@example.test", "password": "sbagliata"});
    let denied = client.post("/api/login", &wrong, None);
    assert_eq!(denied.status, 401);
    assert_eq!(denied.json()["error"], "Invalid email or password.");

    let right = json!({"email": " DANA@example.test", "password": "Spike-12345-pw"});
    let cookie = client.post("/api/login", &right, None).session_cookie();
    assert_eq!(client.state().logins, vec!["dana@example.test"]);
    assert_eq!(client.get("/app", Some(&cookie)).status, 200);
    assert!(client
        .get("/app", Some(&cookie))
        .body
        .contains("Signed in as Dana"));

    let me = client.get("/api/me", Some(&cookie));
    assert_eq!(me.json()["display_name"], "Dana");
    let saved = client.post(
        "/api/settings",
        &json!({"display_name": " Morgan Lee "}),
        Some(&cookie),
    );
    assert_eq!(saved.status, 200);
    assert_eq!(saved.json()["display_name"], "Morgan Lee");
    assert_eq!(client.state().accounts[0].display_name, "Morgan Lee");
    let too_long = json!({"display_name": "x".repeat(61)});
    assert_eq!(
        client
            .post("/api/settings", &too_long, Some(&cookie))
            .status,
        400
    );

    let settings_page = client.get("/app/settings", Some(&cookie));
    assert!(settings_page.body.contains(DISPLAY_NAME_SAVED));

    client.post("/api/logout", &json!({}), Some(&cookie));
    assert_eq!(client.state().active_sessions, 0);
    assert_eq!(client.get("/api/me", Some(&cookie)).status, 401);
    let redirect = client.get("/app", Some(&cookie));
    assert_eq!(redirect.status, 303);
}

#[test]
fn reset_clears_everything_and_seed_conflicts() {
    let server = FixtureServer::start(0).unwrap();
    let client = Client::new(&server);
    let seed = json!({"name": "Dana", "email": "dana@example.test", "password": "Spike-12345-pw"});
    assert_eq!(client.post("/__seed", &seed, None).status, 201);
    assert_eq!(client.post("/__seed", &seed, None).status, 409);
    let bad = json!({"name": "", "email": "dana@example.test", "password": "Spike-12345-pw"});
    assert_eq!(client.post("/__seed", &bad, None).status, 400);

    let login = json!({"email": "dana@example.test", "password": "Spike-12345-pw"});
    client.post("/api/login", &login, None);
    assert_eq!(client.reset().status, 204);
    let state = client.state();
    assert!(state.accounts.is_empty());
    assert!(state.logins.is_empty());
    assert_eq!(state.active_sessions, 0);
}

#[test]
fn pages_and_assets_are_served() {
    let server = FixtureServer::start(0).unwrap();
    let client = Client::new(&server);
    for path in [
        "/",
        "/register",
        "/login",
        "/pricing",
        "/info/terms",
        "/assets/style.css",
    ] {
        assert_eq!(client.get(path, None).status, 200, "{path}");
    }
    assert_eq!(client.get("/welcome", None).status, 303);
    assert_eq!(client.get("/app/settings", None).status, 303);
    assert_eq!(client.get("/missing", None).status, 404);
    assert_eq!(client.get("/assets/nothing.js", None).status, 404);
}
