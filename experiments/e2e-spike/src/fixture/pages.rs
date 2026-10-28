//! Pagine HTML della fixture (asset incorporati: nessun percorso su disco).

use super::http::{Request, Response};
use super::layout::{app_page, placeholder_title, public_page};
use super::session::current_user;
use super::state::Store;
use super::validate::escape_html;
use super::DISPLAY_NAME_SAVED;
use std::sync::Mutex;

const CSS: &str = include_str!("assets/style.css");
const COMMON_JS: &str = include_str!("assets/common.js");
const REGISTER_JS: &str = include_str!("assets/register.js");
const LOGIN_JS: &str = include_str!("assets/login.js");
const SETTINGS_JS: &str = include_str!("assets/settings.js");
const HOME_HTML: &str = include_str!("assets/home.html");
const REGISTER_HTML: &str = include_str!("assets/register.html");
const LOGIN_HTML: &str = include_str!("assets/login.html");
const SETTINGS_HTML: &str = include_str!("assets/settings.html");

const JS_TYPE: &str = "text/javascript; charset=utf-8";
const ASSET_PREFIX: &str = "/assets/";
const SAVED_PLACEHOLDER: &str = "{{DISPLAY_NAME_SAVED}}";

/// `None` se `path` non e' una pagina (nemmeno segnaposto).
pub fn handle(store: &Mutex<Store>, req: &Request) -> Option<Response> {
    let response = render(store, req)?;
    if req.method == "GET" {
        Some(response)
    } else {
        Some(Response::text(405, "Method not allowed"))
    }
}

fn render(store: &Mutex<Store>, req: &Request) -> Option<Response> {
    let path = req.path.as_str();
    if let Some(name) = path.strip_prefix(ASSET_PREFIX) {
        return asset(name);
    }
    let response = match path {
        "/" => Response::html(200, public_page("Spike Demo", HOME_HTML, &[])),
        "/register" => Response::html(
            200,
            public_page("Register", REGISTER_HTML, &["common.js", "register.js"]),
        ),
        "/login" => Response::html(
            200,
            public_page("Sign in", LOGIN_HTML, &["common.js", "login.js"]),
        ),
        "/welcome" => welcome(store, req),
        "/app" => dashboard(store, req),
        "/app/settings" => settings(store, req),
        other => placeholder(other)?,
    };
    Some(response)
}

fn asset(name: &str) -> Option<Response> {
    let (content_type, body) = match name {
        "style.css" => ("text/css; charset=utf-8", CSS),
        "common.js" => (JS_TYPE, COMMON_JS),
        "register.js" => (JS_TYPE, REGISTER_JS),
        "login.js" => (JS_TYPE, LOGIN_JS),
        "settings.js" => (JS_TYPE, SETTINGS_JS),
        _ => return None,
    };
    Some(Response::asset(content_type, body))
}

fn placeholder(path: &str) -> Option<Response> {
    let title = placeholder_title(path)?;
    let main = format!(
        "<h1>{}</h1>\n<p>This page is a placeholder.</p>",
        escape_html(title)
    );
    Some(Response::html(200, public_page(title, &main, &[])))
}

fn welcome(store: &Mutex<Store>, req: &Request) -> Response {
    let Some((name, _)) = current_user(store, req) else {
        return Response::redirect("/login");
    };
    let main = format!("<h1>Welcome, {}</h1>", escape_html(&name));
    Response::html(200, public_page("Welcome", &main, &["common.js"]))
}

fn dashboard(store: &Mutex<Store>, req: &Request) -> Response {
    let Some((_, display_name)) = current_user(store, req) else {
        return Response::redirect("/login");
    };
    let main = "<h1>Dashboard</h1>\n<p>This is a placeholder dashboard.</p>";
    Response::html(
        200,
        app_page("Dashboard", &display_name, main, &["common.js"]),
    )
}

fn settings(store: &Mutex<Store>, req: &Request) -> Response {
    let Some((_, display_name)) = current_user(store, req) else {
        return Response::redirect("/login");
    };
    let main = SETTINGS_HTML.replace(SAVED_PLACEHOLDER, &escape_html(DISPLAY_NAME_SAVED));
    Response::html(
        200,
        app_page(
            "Settings",
            &display_name,
            &main,
            &["common.js", "settings.js"],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::http::read_request;
    use crate::fixture::validate::{
        EMAIL_INVALID, NAME_REQUIRED, PASSWORD_MISMATCH, PASSWORD_SHORT, TERMS_REQUIRED,
    };

    #[test]
    fn client_validation_messages_match_the_server() {
        for message in [
            NAME_REQUIRED,
            EMAIL_INVALID,
            PASSWORD_SHORT,
            PASSWORD_MISMATCH,
            TERMS_REQUIRED,
        ] {
            assert!(REGISTER_JS.contains(message), "{message}");
        }
    }

    #[test]
    fn register_form_has_the_documented_labels() {
        for label in [
            "Full name",
            "Email",
            "Password",
            "Confirm password",
            "I accept the terms and conditions",
            "Create account",
        ] {
            assert!(REGISTER_HTML.contains(label), "{label}");
        }
    }

    #[test]
    fn protected_pages_redirect_without_session() {
        let store = Mutex::new(Store::default());
        for path in ["/welcome", "/app", "/app/settings"] {
            let raw = format!("GET {path} HTTP/1.1\r\n\r\n");
            let req = read_request(&mut raw.as_bytes()).unwrap();
            assert_eq!(handle(&store, &req).unwrap().status(), 303, "{path}");
        }
    }

    #[test]
    fn unknown_paths_and_wrong_methods() {
        let store = Mutex::new(Store::default());
        let get = |raw: &str| read_request(&mut raw.as_bytes()).unwrap();
        assert!(handle(&store, &get("GET /nope HTTP/1.1\r\n\r\n")).is_none());
        assert!(handle(&store, &get("GET /assets/../x HTTP/1.1\r\n\r\n")).is_none());
        assert_eq!(
            handle(&store, &get("POST /login HTTP/1.1\r\n\r\n"))
                .unwrap()
                .status(),
            405
        );
        assert_eq!(
            handle(&store, &get("GET /pricing HTTP/1.1\r\n\r\n"))
                .unwrap()
                .status(),
            200
        );
    }
}
