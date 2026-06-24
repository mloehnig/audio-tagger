use std::io::{Read, Write};
use std::net::TcpListener;
use onetagger_shared::{PORT, WEBSERVER_CALLBACKS};

/// Page shown in the browser after the Spotify redirect is captured.
const CALLBACK_HTML: &str = "<html><head><script>window.close();</script></head>\
<body><h1>Spotify authorized successfully, you can close this window.</h1></body></html>";

/// Spawn a minimal one-shot HTTP server on `127.0.0.1:PORT` to capture the Spotify OAuth
/// redirect (the `/spotify?code=...` callback) and store it in `WEBSERVER_CALLBACKS`, which
/// `Spotify::auth_server` polls. Replaces the full `onetagger-ui` web server for CLI auth.
pub fn spawn_callback_server() {
    std::thread::spawn(|| {
        let listener = match TcpListener::bind(("127.0.0.1", PORT as u16)) {
            Ok(l) => l,
            Err(e) => {
                error!("Failed binding Spotify callback server on 127.0.0.1:{PORT}: {e}");
                return;
            }
        };
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };

            // We only need the request line: `GET /spotify?code=...&state=... HTTP/1.1`
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let target = request.lines().next().and_then(|line| line.split_whitespace().nth(1));

            // Store the callback path so Spotify::auth_server can parse the code out of it
            // (mirrors what onetagger-ui's /spotify route did with request.uri()).
            if let Some(target) = target {
                if target.starts_with("/spotify") {
                    debug!("Spotify callback: {target}");
                    WEBSERVER_CALLBACKS.lock().unwrap().insert("spotify".to_string(), target.to_string());
                }
            }

            // Respond and finish
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                CALLBACK_HTML.len(),
                CALLBACK_HTML
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();

            // Done once the spotify callback has been captured
            if WEBSERVER_CALLBACKS.lock().unwrap().contains_key("spotify") {
                break;
            }
        }
    });
}
