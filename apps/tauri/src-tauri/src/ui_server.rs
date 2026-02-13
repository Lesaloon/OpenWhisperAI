use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::thread;

pub fn maybe_start() {
    let enabled = std::env::var("OPENWHISPERAI_UI_SERVER")
        .ok()
        .map(|value| value != "0")
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let ui_dir = std::env::var("OPENWHISPERAI_UI_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("tauri-app")
                .join("public")
        });

    thread::spawn(move || serve(ui_dir));
}

fn serve(root: PathBuf) {
    let addr = "127.0.0.1:1421";
    let server = match tiny_http::Server::http(addr) {
        Ok(server) => server,
        Err(err) => {
            eprintln!("ui server failed to bind {addr}: {err}");
            return;
        }
    };
    println!("ui server listening on http://{addr}");

    for request in server.incoming_requests() {
        let url = request.url().split('?').next().unwrap_or("/");
        let path = sanitize_path(url);
        let path = if path.as_os_str().is_empty() {
            PathBuf::from("index.html")
        } else {
            path
        };

        let full_path = root.join(&path);
        let response = match read_file(&full_path) {
            Ok((body, content_type)) => tiny_http::Response::from_data(body).with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type).unwrap(),
            ),
            Err(_) => tiny_http::Response::from_string("Not found").with_status_code(404),
        };
        let _ = request.respond(response);
    }
}

fn sanitize_path(url: &str) -> PathBuf {
    let raw = url.trim_start_matches('/');
    let mut safe = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(part) => safe.push(part),
            _ => {}
        }
    }
    safe
}

fn read_file(path: &Path) -> Result<(Vec<u8>, &'static str), std::io::Error> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let content_type = match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    Ok((buffer, content_type))
}
