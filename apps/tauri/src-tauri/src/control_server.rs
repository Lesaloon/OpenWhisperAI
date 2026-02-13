use crate::ptt::PttHandle;
use std::thread;

const CONTROL_ADDR: &str = "127.0.0.1:1422";

pub fn start(handle: PttHandle) {
    let enabled = std::env::var("OPENWHISPERAI_CONTROL_SERVER")
        .ok()
        .map(|value| value != "0")
        .unwrap_or(true);
    if !enabled {
        return;
    }

    thread::spawn(move || {
        let server = match tiny_http::Server::http(CONTROL_ADDR) {
            Ok(server) => server,
            Err(err) => {
                log::warn!("control server failed to bind {CONTROL_ADDR}: {err}");
                return;
            }
        };
        log::info!("control server listening on http://{CONTROL_ADDR}");

        for request in server.incoming_requests() {
            let url = request.url();
            if url.starts_with("/toggle") {
                let result = handle.manual_toggle();
                let status = if result.is_ok() { 200 } else { 500 };
                let body = match result {
                    Ok(state) => format!("ok {state:?}"),
                    Err(err) => format!("error {err}"),
                };
                let _ = request
                    .respond(tiny_http::Response::from_string(body).with_status_code(status));
                continue;
            }
            if url.starts_with("/ping") {
                let _ = request.respond(tiny_http::Response::from_string("pong"));
                continue;
            }

            let _ = request
                .respond(tiny_http::Response::from_string("not found").with_status_code(404));
        }
    });
}
