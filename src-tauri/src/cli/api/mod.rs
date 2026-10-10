//! Loopback-only HTTP API for scripts and local automation.
//!
//! The module is split by concern so a reader can jump straight to the topic:
//!
//! | File        | Owns                                                                      |
//! | ----------- | ------------------------------------------------------------------------- |
//! | `mod.rs`    | `LocalApiServer` lifecycle (construction, bind, start, stop) and limits   |
//! | `http.rs`   | Constants, accept loop, connection handler, HTTP framing, auth, responses |
//! | `routes.rs` | Endpoint dispatch plus the JSON/query/text helpers the endpoints need     |
//! | `tests.rs`  | Unit and loopback tests for every file above                              |

use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use crate::item_operations::CopyContext;
use crate::storage::Database;

use http::{serve, ServeContext};

mod http;
mod routes;

#[cfg(test)]
mod tests;

/// A loopback-only HTTP API for scripts and local automation.
///
/// The server is opt-in from the Tauri command layer and never binds a public
/// interface. It owns a separate SQLite connection wrapped in `Arc`, so API
/// requests can safely run alongside the desktop UI.
///
/// Every request must present `Authorization: Bearer <token>` (the token is
/// persisted beside the config at `conf/api.token`), must target
/// `Host: 127.0.0.1:<port>` (or `localhost:<port>`), and must NOT carry an
/// `Origin` header. The last rule is what keeps browser pages 閳?the CSRF and
/// DNS-rebinding threat model 閳?unable to reach the clipboard history even
/// from a malicious website running on the same machine.
pub struct LocalApiServer {
    pub port: u16,
    database: Option<Arc<Database>>,
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    page_size_limit: u32,
    search_page_size_limit: u32,
    token: Option<Arc<String>>,
    copy_context: CopyContext,
    on_change: Arc<dyn Fn() + Send + Sync>,
}

impl LocalApiServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            database: None,
            stop_sender: None,
            handle: None,
            page_size_limit: 500,
            search_page_size_limit: 500,
            token: None,
            copy_context: CopyContext::default(),
            on_change: Arc::new(|| {}),
        }
    }

    pub fn with_limits(mut self, page_size_limit: u32, search_page_size_limit: u32) -> Self {
        self.page_size_limit = page_size_limit;
        self.search_page_size_limit = search_page_size_limit;
        self
    }

    pub fn with_database(port: u16, database: Arc<Database>) -> Self {
        let mut server = Self::new(port);
        server.database = Some(database);
        server
    }

    /// Sets the bearer token clients must present. Required before start.
    pub fn set_copy_context(&mut self, context: CopyContext) {
        self.copy_context = context;
    }
    pub fn set_change_callback(&mut self, callback: Arc<dyn Fn() + Send + Sync>) {
        self.on_change = callback;
    }
    pub fn set_token(&mut self, token: String) {
        self.token = Some(Arc::new(token));
    }

    pub fn start(&mut self) -> Result<u16, String> {
        let database = self
            .database
            .clone()
            .ok_or_else(|| "local API database is not configured".to_owned())?;
        self.start_with_database(database)
    }

    pub fn start_with_database(&mut self, database: Arc<Database>) -> Result<u16, String> {
        if self.handle.is_some() {
            return Err("local API server is already running".to_owned());
        }
        let token = self
            .token
            .clone()
            .ok_or_else(|| "local API token is not configured".to_owned())?;

        let listener = TcpListener::bind(("127.0.0.1", self.port))
            .map_err(|error| format!("failed to bind local API: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure local API listener: {error}"))?;
        self.port = listener
            .local_addr()
            .map_err(|error| format!("failed to inspect local API listener: {error}"))?
            .port();
        self.database = Some(database.clone());

        let (stop_sender, stop_receiver) = mpsc::channel();
        let active_connections = Arc::new(AtomicUsize::new(0));
        let context = ServeContext {
            database,
            page_size_limit: self.page_size_limit,
            search_page_size_limit: self.search_page_size_limit,
            token,
            port: self.port,
            active_connections,
            copy_context: self.copy_context.clone(),
            on_change: self.on_change.clone(),
        };
        let handle = thread::Builder::new()
            .name("clipboard-local-api".to_owned())
            .spawn(move || serve(listener, stop_receiver, context))
            .map_err(|error| format!("failed to start local API: {error}"))?;
        self.stop_sender = Some(stop_sender);
        self.handle = Some(handle);
        Ok(self.port)
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    pub fn set_port(&mut self, port: u16) -> Result<(), String> {
        if self.is_running() {
            return Err("local API server is already running".to_owned());
        }
        self.port = port;
        Ok(())
    }

    pub fn set_limits(&mut self, page_size_limit: u32, search_page_size_limit: u32) {
        self.page_size_limit = page_size_limit;
        self.search_page_size_limit = search_page_size_limit;
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            if handle.thread().id() != thread::current().id() {
                handle
                    .join()
                    .map_err(|_| "local API thread terminated with a panic".to_owned())?;
            }
        }
        Ok(())
    }
}

impl Drop for LocalApiServer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
