//! API JSON della fixture e hook di test (`/__state`, `/__reset`, `/__seed`).

use super::http::{Request, Response};
use super::session::{clear_cookie_value, current_user, lock, session_id, set_cookie_value};
use super::state::{SeedAccount, Store};
use super::validate::{
    normalize_email, validate_display_name, validate_email, validate_name, validate_password,
    validate_registration, RegisterInput,
};
use super::DUPLICATE_EMAIL_ERROR;
use serde::Deserialize;
use serde_json::json;
use std::sync::Mutex;

const INVALID_LOGIN: &str = "Invalid email or password.";
const NOT_SIGNED_IN: &str = "Not signed in.";
const INVALID_JSON: &str = "Invalid JSON body.";
const SET_COOKIE: &str = "Set-Cookie";

#[derive(Deserialize)]
struct LoginInput {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct SettingsInput {
    display_name: String,
}

/// `None` se `path` non appartiene alle API (404 a carico del chiamante).
pub fn handle(store: &Mutex<Store>, req: &Request) -> Option<Response> {
    let response = match req.path.as_str() {
        "/api/register" => only(req, "POST", || register(store, req)),
        "/api/login" => only(req, "POST", || login(store, req)),
        "/api/logout" => only(req, "POST", || logout(store, req)),
        "/api/me" => only(req, "GET", || me(store, req)),
        "/api/settings" => only(req, "POST", || settings(store, req)),
        "/__state" => only(req, "GET", || Response::json(200, &lock(store).snapshot())),
        "/__reset" => only(req, "POST", || {
            lock(store).reset();
            Response::empty(204)
        }),
        "/__seed" => only(req, "POST", || seed(store, req)),
        _ => return None,
    };
    Some(response)
}

fn only(req: &Request, method: &str, handler: impl FnOnce() -> Response) -> Response {
    if req.method == method {
        handler()
    } else {
        Response::text(405, "Method not allowed")
    }
}

fn error(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn parse_body<'a, T: Deserialize<'a>>(req: &'a Request) -> Result<T, Response> {
    serde_json::from_slice(&req.body).map_err(|_| error(400, INVALID_JSON))
}

fn register(store: &Mutex<Store>, req: &Request) -> Response {
    let input: RegisterInput = match parse_body(req) {
        Ok(input) => input,
        Err(response) => return response,
    };
    let registration = match validate_registration(&input) {
        Ok(registration) => registration,
        Err(message) => return error(400, message),
    };
    let mut state = lock(store);
    let added = state.add_account(
        &registration.name,
        &registration.email,
        &registration.password,
    );
    if added.is_err() {
        return error(409, DUPLICATE_EMAIL_ERROR);
    }
    let session = state.open_session(&registration.email);
    Response::json(201, &json!({ "ok": true })).with_header(SET_COOKIE, set_cookie_value(&session))
}

fn login(store: &Mutex<Store>, req: &Request) -> Response {
    let input: LoginInput = match parse_body(req) {
        Ok(input) => input,
        Err(response) => return response,
    };
    let email = normalize_email(&input.email);
    let mut state = lock(store);
    if !state.login(&email, &input.password) {
        return error(401, INVALID_LOGIN);
    }
    let session = state.open_session(&email);
    Response::json(200, &json!({ "ok": true })).with_header(SET_COOKIE, set_cookie_value(&session))
}

fn logout(store: &Mutex<Store>, req: &Request) -> Response {
    if let Some(id) = session_id(req) {
        lock(store).close_session(id);
    }
    Response::json(200, &json!({ "ok": true })).with_header(SET_COOKIE, clear_cookie_value())
}

fn me(store: &Mutex<Store>, req: &Request) -> Response {
    match current_user(store, req) {
        Some((name, display_name)) => {
            Response::json(200, &json!({ "name": name, "display_name": display_name }))
        }
        None => error(401, NOT_SIGNED_IN),
    }
}

fn settings(store: &Mutex<Store>, req: &Request) -> Response {
    let Some(id) = session_id(req) else {
        return error(401, NOT_SIGNED_IN);
    };
    let input: SettingsInput = match parse_body(req) {
        Ok(input) => input,
        Err(response) => return response,
    };
    let display_name = match validate_display_name(&input.display_name) {
        Ok(name) => name,
        Err(message) => return error(400, message),
    };
    if lock(store).set_display_name(id, &display_name) {
        Response::json(200, &json!({ "display_name": display_name }))
    } else {
        error(401, NOT_SIGNED_IN)
    }
}

fn seed(store: &Mutex<Store>, req: &Request) -> Response {
    let input: SeedAccount = match parse_body(req) {
        Ok(input) => input,
        Err(response) => return response,
    };
    let valid = validate_name(&input.name).and_then(|name| {
        let email = validate_email(&input.email)?;
        validate_password(&input.password)?;
        Ok((name, email))
    });
    let (name, email) = match valid {
        Ok(fields) => fields,
        Err(message) => return error(400, message),
    };
    match lock(store).add_account(&name, &email, &input.password) {
        Ok(()) => Response::json(201, &json!({ "ok": true })),
        Err(_) => error(409, DUPLICATE_EMAIL_ERROR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::http::read_request;

    fn request(method: &str, path: &str, cookie: Option<&str>, body: &str) -> Request {
        let cookie = cookie.map_or(String::new(), |c| format!("Cookie: session={c}\r\n"));
        let raw = format!(
            "{method} {path} HTTP/1.1\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        read_request(&mut raw.as_bytes()).unwrap()
    }

    fn call(store: &Mutex<Store>, method: &str, path: &str, body: &str) -> Response {
        handle(store, &request(method, path, None, body)).unwrap()
    }

    #[test]
    fn unknown_paths_are_not_api() {
        let store = Mutex::new(Store::default());
        assert!(handle(&store, &request("GET", "/api/none", None, "")).is_none());
    }

    #[test]
    fn wrong_method_is_405() {
        let store = Mutex::new(Store::default());
        assert_eq!(call(&store, "GET", "/api/login", "").status(), 405);
        assert_eq!(call(&store, "POST", "/__state", "").status(), 405);
    }

    #[test]
    fn bad_json_is_400() {
        let store = Mutex::new(Store::default());
        assert_eq!(call(&store, "POST", "/api/login", "{nope").status(), 400);
        assert_eq!(call(&store, "POST", "/__seed", "{}").status(), 400);
    }

    #[test]
    fn settings_and_me_require_a_session() {
        let store = Mutex::new(Store::default());
        assert_eq!(call(&store, "GET", "/api/me", "").status(), 401);
        let body = r#"{"display_name":"X"}"#;
        assert_eq!(call(&store, "POST", "/api/settings", body).status(), 401);
    }

    #[test]
    fn seed_validates_and_detects_duplicates() {
        let store = Mutex::new(Store::default());
        let ok = r#"{"name":"Dana","email":"Dana@Example.test","password":"long-enough"}"#;
        assert_eq!(call(&store, "POST", "/__seed", ok).status(), 201);
        assert_eq!(call(&store, "POST", "/__seed", ok).status(), 409);
        let bad = r#"{"name":"Dana","email":"nope","password":"long-enough"}"#;
        assert_eq!(call(&store, "POST", "/__seed", bad).status(), 400);
        assert_eq!(
            lock(&store).snapshot().accounts[0].email,
            "dana@example.test"
        );
    }
}
