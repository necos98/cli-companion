//! Parsing minimale di richieste HTTP/1.1 e scrittura delle risposte.
//! Solo `Connection: close`, nessun chunked, nessun keep-alive.

use serde::Serialize;
use std::io::{Read, Write};

pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
const READ_CHUNK: usize = 4096;
const HEADER_END: &[u8] = b"\r\n\r\n";
const MAX_METHOD_CHARS: usize = 8;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Header con nome in minuscolo.
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RequestError {
    Malformed(&'static str),
    BodyTooLarge,
    Io(String),
}

impl RequestError {
    pub fn to_response(&self) -> Response {
        match self {
            RequestError::BodyTooLarge => Response::text(413, "Payload too large"),
            RequestError::Malformed(reason) => Response::text(400, reason),
            RequestError::Io(reason) => Response::text(400, reason),
        }
    }
}

pub fn read_request<R: Read>(reader: &mut R) -> Result<Request, RequestError> {
    let mut buf = Vec::new();
    let head_end = read_until_head_end(reader, &mut buf)?;
    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_| RequestError::Malformed("Request head is not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let (method, path) = parse_request_line(lines.next().unwrap_or_default())?;
    let headers = lines.map(parse_header).collect::<Result<Vec<_>, _>>()?;
    let length = content_length(&headers)?;
    let mut body = buf.split_off(head_end + HEADER_END.len());
    body.truncate(length);
    read_body(reader, &mut body, length)?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

/// Legge fino a `\r\n\r\n` (tetto `MAX_HEADER_BYTES`); ritorna la sua posizione.
fn read_until_head_end<R: Read>(reader: &mut R, buf: &mut Vec<u8>) -> Result<usize, RequestError> {
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        if let Some(pos) = buf.windows(HEADER_END.len()).position(|w| w == HEADER_END) {
            return if pos > MAX_HEADER_BYTES {
                Err(RequestError::Malformed("Headers too large"))
            } else {
                Ok(pos)
            };
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(RequestError::Malformed("Headers too large"));
        }
        let n = reader
            .read(&mut chunk)
            .map_err(|e| RequestError::Io(format!("Read failed: {e}")))?;
        if n == 0 {
            return Err(RequestError::Malformed(
                "Connection closed before headers end",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn parse_request_line(line: &str) -> Result<(String, String), RequestError> {
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(RequestError::Malformed("Bad request line"));
    };
    let method_ok = !method.is_empty()
        && method.len() <= MAX_METHOD_CHARS
        && method.bytes().all(|b| b.is_ascii_uppercase());
    if !method_ok || !version.starts_with("HTTP/1.") {
        return Err(RequestError::Malformed("Bad request line"));
    }
    if !target.starts_with('/') || target.starts_with("//") {
        return Err(RequestError::Malformed("Bad request target"));
    }
    let path = target.split(['?', '#']).next().unwrap_or(target);
    Ok((method.to_string(), path.to_string()))
}

fn parse_header(line: &str) -> Result<(String, String), RequestError> {
    let (name, value) = line
        .split_once(':')
        .ok_or(RequestError::Malformed("Bad header line"))?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(RequestError::Malformed("Bad header name"));
    }
    Ok((name.to_ascii_lowercase(), value.trim().to_string()))
}

fn content_length(headers: &[(String, String)]) -> Result<usize, RequestError> {
    if headers.iter().any(|(n, _)| n == "transfer-encoding") {
        return Err(RequestError::Malformed("Transfer-Encoding not supported"));
    }
    let Some((_, value)) = headers.iter().find(|(n, _)| n == "content-length") else {
        return Ok(0);
    };
    let length: usize = value
        .parse()
        .map_err(|_| RequestError::Malformed("Bad Content-Length"))?;
    if length > MAX_BODY_BYTES {
        return Err(RequestError::BodyTooLarge);
    }
    Ok(length)
}

fn read_body<R: Read>(
    reader: &mut R,
    body: &mut Vec<u8>,
    length: usize,
) -> Result<(), RequestError> {
    let mut chunk = [0u8; READ_CHUNK];
    while body.len() < length {
        let want = (length - body.len()).min(READ_CHUNK);
        let n = reader
            .read(&mut chunk[..want])
            .map_err(|e| RequestError::Io(format!("Read failed: {e}")))?;
        if n == 0 {
            return Err(RequestError::Malformed("Body shorter than Content-Length"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

pub struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Response {
            status,
            content_type,
            body,
            headers: Vec::new(),
        }
    }

    pub fn html(status: u16, body: String) -> Self {
        Self::new(status, "text/html; charset=utf-8", body.into_bytes())
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self::new(
            status,
            "text/plain; charset=utf-8",
            body.as_bytes().to_vec(),
        )
    }

    pub fn asset(content_type: &'static str, body: &str) -> Self {
        Self::new(200, content_type, body.as_bytes().to_vec())
    }

    pub fn json<T: Serialize>(status: u16, value: &T) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        Self::new(status, "application/json", body)
    }

    pub fn empty(status: u16) -> Self {
        Self::new(status, "text/plain; charset=utf-8", Vec::new())
    }

    pub fn redirect(location: &str) -> Self {
        Self::empty(303).with_header("Location", location.to_string())
    }

    pub fn with_header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    #[cfg(test)]
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
        );
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        writer.write_all(head.as_bytes())?;
        writer.write_all(&self.body)?;
        writer.flush()
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &[u8]) -> Result<Request, RequestError> {
        read_request(&mut &raw[..])
    }

    #[test]
    fn parses_get_with_query_and_headers() {
        let req =
            parse(b"GET /app?x=1 HTTP/1.1\r\nHost: a\r\nCookie: a=1; session=abc\r\n\r\n").unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/app");
        assert_eq!(req.header("host"), Some("a"));
        assert_eq!(req.cookie("session"), Some("abc"));
        assert_eq!(req.cookie("missing"), None);
        assert!(req.body.is_empty());
    }

    #[test]
    fn parses_post_body_split_across_reads() {
        struct Trickle<'a>(&'a [u8]);
        impl Read for Trickle<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.len().min(buf.len()).min(5);
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let raw = b"POST /api/login HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"a\":\"bcd\"}";
        let req = read_request(&mut Trickle(raw)).unwrap();
        assert_eq!(req.body, b"{\"a\":\"bcd\"}");
    }

    #[test]
    fn rejects_malformed_requests() {
        for raw in [
            &b"GARBAGE\r\n\r\n"[..],
            b"get / HTTP/1.1\r\n\r\n",
            b"GET / HTTP/2.0\r\n\r\n",
            b"GET //evil HTTP/1.1\r\n\r\n",
            b"GET nopath HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1\r\nBad Header: x\r\n\r\n",
            b"GET / HTTP/1.1\r\nNoColon\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: abc\r\n\r\n",
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort",
            b"GET / HTTP/1.1\r\n",
        ] {
            assert!(
                matches!(parse(raw), Err(RequestError::Malformed(_))),
                "{}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn enforces_header_and_body_limits() {
        let big_header = format!(
            "GET / HTTP/1.1\r\nX: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_BYTES)
        );
        assert!(matches!(
            parse(big_header.as_bytes()),
            Err(RequestError::Malformed(_))
        ));
        let big_body = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert_eq!(
            parse(big_body.as_bytes()).unwrap_err(),
            RequestError::BodyTooLarge
        );
        assert_eq!(RequestError::BodyTooLarge.to_response().status(), 413);
    }

    #[test]
    fn response_has_length_and_close() {
        let mut out = Vec::new();
        Response::redirect("/login").write_to(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 303 See Other\r\n"));
        assert!(text.contains("Content-Length: 0\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.contains("Location: /login\r\n"));
    }
}
