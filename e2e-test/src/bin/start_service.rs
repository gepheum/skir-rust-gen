// Starts a Skir service on http://127.0.0.1:8787/myapi
//
// Run with:
//   cargo run --bin start_service
//
// Use `call_service` to send requests, or open the URL in a browser to get
// the Skir Studio UI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use e2e_test::skir_client::service::{RawResponse, ServiceBuilder, ServiceError};
use e2e_test::skirout::base::service::{
    AddUserRequest, AddUserResponse, GetUserRequest, GetUserResponse, User, add_user_method,
    get_user_method,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

// =============================================================================
// In-memory user store
// =============================================================================

struct UserStore {
    id_to_user: HashMap<i32, User>,
}

impl UserStore {
    fn new() -> Self {
        Self {
            id_to_user: HashMap::new(),
        }
    }
}

// =============================================================================
// main
// =============================================================================

#[tokio::main]
async fn main() {
    let store = Arc::new(Mutex::new(UserStore::new()));

    let store_for_get = store.clone();
    let store_for_add = store.clone();

    let service = Arc::new(
        ServiceBuilder::<()>::new()
            .add_method(get_user_method(), move |req: GetUserRequest, _meta: ()| {
                let store = store_for_get.clone();
                async move {
                    let store = store.lock().unwrap();
                    Ok(GetUserResponse {
                        user: store.id_to_user.get(&req.user_id).cloned(),
                        _unrecognized: None,
                    })
                }
            })
            .expect("duplicate method number")
            .add_method(add_user_method(), move |req: AddUserRequest, _meta: ()| {
                let store = store_for_add.clone();
                async move {
                    if req.user.name.is_empty() {
                        return Err(ServiceError::bad_request("user name must not be empty"));
                    }
                    let mut store = store.lock().unwrap();
                    let user_id = req.user.user_id;
                    store.id_to_user.insert(user_id, req.user);
                    Ok(AddUserResponse {
                        _unrecognized: None,
                    })
                }
            })
            .expect("duplicate method number")
            .set_can_send_unknown_error_message(true)
            .build(),
    );

    let addr = "127.0.0.1:8787";
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    println!("Skir service listening on http://{addr}/myapi");
    println!("Open http://{addr}/myapi in a browser to try Skir Studio.");
    println!("Press Ctrl+C to stop.");

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let service = service.clone();
                tokio::spawn(async move {
                    handle_connection(stream, service).await;
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

// =============================================================================
// Minimal HTTP/1.1 handler
// =============================================================================

async fn handle_connection(
    mut stream: TcpStream,
    service: Arc<e2e_test::skir_client::Service<()>>,
) {
    let (read_half, mut write_half) = stream.split();
    let mut reader = BufReader::new(read_half);

    // ── Request line ──────────────────────────────────────────────────────────
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.is_err() {
        return;
    }
    let parts: Vec<&str> = request_line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let raw_path = parts[1];

    // ── Headers ───────────────────────────────────────────────────────────────
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() {
            break;
        }
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().ok();
        }
    }

    // ── Route check: only serve /myapi ────────────────────────────────────────
    let path = raw_path.split('?').next().unwrap_or("/");
    if path != "/myapi" {
        write_response(
            &mut write_half,
            404,
            "text/plain; charset=utf-8",
            b"Not Found",
        )
        .await;
        return;
    }

    // ── Build Skir body ───────────────────────────────────────────────────────
    let body: String = if method == "GET" {
        let query = raw_path.find('?').map(|i| &raw_path[i + 1..]).unwrap_or("");
        percent_decode(query)
    } else if method == "POST" {
        let len = content_length.unwrap_or(0);
        if len == 0 {
            String::new()
        } else {
            let mut buf = vec![0u8; len];
            if reader.read_exact(&mut buf).await.is_err() {
                write_response(
                    &mut write_half,
                    400,
                    "text/plain; charset=utf-8",
                    b"Bad Request",
                )
                .await;
                return;
            }
            String::from_utf8(buf).unwrap_or_default()
        }
    } else {
        write_response(
            &mut write_half,
            405,
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
        )
        .await;
        return;
    };

    // ── Dispatch ──────────────────────────────────────────────────────────────
    let RawResponse {
        data,
        status_code,
        content_type,
    } = service.handle_request(&body, ()).await;
    write_response(&mut write_half, status_code, content_type, data.as_bytes()).await;
}

async fn write_response(
    w: &mut (impl AsyncWriteExt + Unpin),
    status: u16,
    content_type: &str,
    body: &[u8],
) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );
    let _ = w.write_all(header.as_bytes()).await;
    let _ = w.write_all(body).await;
}

/// Decodes a percent-encoded URL query string.
fn percent_decode(s: &str) -> String {
    let sb = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(sb.len());
    let mut i = 0;
    while i < sb.len() {
        if sb[i] == b'%' && i + 2 < sb.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(sb[i + 1]), hex_nibble(sb[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        } else if sb[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(sb[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
