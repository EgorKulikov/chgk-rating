//! A tiny local HTTP server for driving the clients without the network.
//! Records every request it receives; answers from a caller-supplied
//! function of (method, path-and-query, body).

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// What to answer: status, content type, extra headers, body.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(mut self, name: &str, value: &str) -> Reply {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub fn json(body: &str) -> Reply {
    Reply {
        status: 200,
        content_type: "application/json",
        headers: vec![],
        body: body.into(),
    }
}

pub fn html(body: &str) -> Reply {
    Reply {
        status: 200,
        content_type: "text/html; charset=utf-8",
        headers: vec![],
        body: body.into(),
    }
}

pub fn status(code: u16, body: &str) -> Reply {
    Reply {
        status: code,
        content_type: "text/html; charset=utf-8",
        headers: vec![],
        body: body.into(),
    }
}

/// A 302 to `location`.
pub fn redirect(location: &str) -> Reply {
    status(302, "").header("location", location)
}

pub type Handler = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

pub struct Server {
    pub addr: SocketAddr,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// Start a server on an ephemeral port; runs until the test's runtime
/// shuts down.
pub async fn serve<F>(handler: F) -> Server
where
    F: Fn(&Seen) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
    let handler: Handler = Arc::new(handler);
    let seen_bg = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let seen = seen_bg.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let seen = seen.clone();
                    let handler = handler.clone();
                    async move {
                        let method = req.method().to_string();
                        let path = req
                            .uri()
                            .path_and_query()
                            .map(|p| p.to_string())
                            .unwrap_or_default();
                        let headers = req
                            .headers()
                            .iter()
                            .map(|(k, v)| {
                                (
                                    k.to_string(),
                                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                                )
                            })
                            .collect();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let body = String::from_utf8_lossy(&body).into_owned();
                        let s = Seen {
                            method,
                            path,
                            headers,
                            body,
                        };
                        // Record first, so a panicking handler still leaves a trace.
                        seen.lock().unwrap().push(s.clone());
                        let reply = handler(&s);
                        let mut builder = Response::builder()
                            .status(StatusCode::from_u16(reply.status).unwrap())
                            .header("content-type", reply.content_type);
                        for (k, v) in &reply.headers {
                            builder = builder.header(k.as_str(), v.as_str());
                        }
                        let resp = builder.body(Full::new(Bytes::from(reply.body))).unwrap();
                        Ok::<_, hyper::Error>(resp)
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    Server { addr, seen }
}
