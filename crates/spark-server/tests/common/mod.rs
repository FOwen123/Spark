#![allow(dead_code)]

use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct TestServer {
    pub port: u16,
    pub token: String,
    pub dir: tempfile::TempDir,
}

impl TestServer {
    pub async fn start() -> TestServer {
        let dir = tempfile::tempdir().unwrap();
        let config = spark_server::Config::load(dir.path().join("config"), dir.path().join("files")).unwrap();
        let token = config.token.clone();
        let listener = spark_server::bind(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(spark_server::serve_on(listener, None, config));
        TestServer { port, token, dir }
    }

    pub fn files_dir(&self) -> PathBuf {
        self.dir.path().join("files")
    }

    /// A raw HTTP/1.1 request; returns (status, body).
    pub async fn http(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            req.push_str(&format!("Host: 127.0.0.1:{}\r\n", self.port));
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(body);
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        (status, body)
    }
}
