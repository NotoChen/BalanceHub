//! Loopback-only protocol fixtures. No installed configuration or external site is used.
use crate::models::{AuthMode, Provider, ProviderInput, ProviderProxyMode};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Reply(pub u16, pub &'static str);

pub(crate) fn serve(replies: Vec<Reply>) -> (Provider, Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, requests) = mpsc::channel();
    let server = thread::spawn(move || {
        for Reply(status, body) in replies {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing model request");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8(bytes).unwrap());
            write!(stream, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let mut input = ProviderInput::default();
    input.identity.base_url = format!("http://{address}/relay");
    input.auth.mode = AuthMode::Session;
    input.auth.session_cookie = "session=fixture-session".into();
    input.auth.api_user = "42".into();
    input.auth.api_key = "sk-fixture-model-key".into();
    input.automation.auto_shield = false;
    input.proxy.mode = ProviderProxyMode::NoProxy;
    (
        Provider::from_input(input, format!("fixture-{address}")),
        requests,
        server,
    )
}
