//! Synchronous facade over cancellable async HTTP. Dropping the selected
//! request/body future aborts I/O; no detached per-request blocking thread.
use crate::cancellation::{self, CancellationToken, CANCELLED};
use bytes::{Buf, Bytes};
use std::{
    io::{self, Read},
    sync::OnceLock,
};

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("create sync HTTP runtime")
    })
}
#[derive(Debug)]
pub(crate) enum SendError {
    Cancelled,
    Http(reqwest::Error),
}
impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str(CANCELLED),
            Self::Http(e) => e.fmt(f),
        }
    }
}
impl SendError {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Http(e) if e.is_timeout() || e.is_connect() || e.is_request())
    }
}
#[derive(Debug)]
pub(crate) struct Response {
    inner: reqwest::Response,
    token: CancellationToken,
    pending: Bytes,
}
impl Response {
    pub fn status(&self) -> reqwest::StatusCode {
        self.inner.status()
    }
    pub fn headers(&self) -> &reqwest::header::HeaderMap {
        self.inner.headers()
    }
    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }
}
impl Read for Response {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.token.check().map_err(io::Error::other)?;
        while self.pending.is_empty() {
            let next = runtime().block_on(async {
                tokio::select! { biased;
                    _ = self.token.0.cancelled() => Err(io::Error::other(CANCELLED)),
                    result = self.inner.chunk() => result.map_err(io::Error::other),
                }
            })?;
            match next {
                Some(bytes) => self.pending = bytes,
                None => return Ok(0),
            }
        }
        let count = output.len().min(self.pending.len());
        output[..count].copy_from_slice(&self.pending[..count]);
        self.pending.advance(count);
        Ok(count)
    }
}
pub(crate) fn send(builder: reqwest::RequestBuilder) -> Result<Response, SendError> {
    let token = cancellation::current();
    let inner = runtime().block_on(async {
        tokio::select! { biased;
            _ = token.0.cancelled() => Err(SendError::Cancelled),
            response = builder.send() => response.map_err(SendError::Http),
        }
    })?;
    Ok(Response {
        inner,
        token,
        pending: Bytes::new(),
    })
}
/// File input stays bounded at 64 KiB per poll. Local filesystem calls cannot
/// be preempted, but cancellation drops network waits and the owned source.
pub(crate) fn stream_body(reader: impl Read + Send + 'static) -> reqwest::Body {
    let token = cancellation::current();
    let stream =
        futures_util::stream::try_unfold((reader, token), |(mut reader, token)| async move {
            token.check().map_err(io::Error::other)?;
            let mut buffer = vec![0; 64 * 1024];
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                Ok::<_, io::Error>(None)
            } else {
                buffer.truncate(count);
                Ok(Some((Bytes::from(buffer), (reader, token))))
            }
        });
    reqwest::Body::wrap_stream(stream)
}
