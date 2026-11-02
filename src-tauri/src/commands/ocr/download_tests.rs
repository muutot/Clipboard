use super::*;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const BODY: &[u8] = b"fixture-model";

#[derive(Clone, Copy)]
enum ResponseMode {
    Exact,
    ExactChunked,
    ExactCloseDelimited,
    OversizedDeclared,
    OversizedChunked,
    OversizedCloseDelimited,
}

struct DownloadServer {
    file: ocr::models::PpOcrModelFile,
    requests: Arc<AtomicUsize>,
    release: mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DownloadServer {
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.release.send(());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for DownloadServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn server() -> DownloadServer {
    server_with_response(ResponseMode::Exact)
}

fn server_with_response(mode: ResponseMode) -> DownloadServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url =
        Box::leak(format!("http://{}/fixture", listener.local_addr().unwrap()).into_boxed_str());
    use sha2::{Digest, Sha256};
    let digest = Box::leak(hex::encode(Sha256::digest(BODY)).into_boxed_str());
    let file = ocr::models::PpOcrModelFile {
        filename: "fixture-model.onnx",
        label: "synthetic model",
        size_bytes: BODY.len() as u64,
        sha256: digest,
        url,
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (release, receiver) = mpsc::channel();
    let count = requests.clone();
    let stopping = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut receiver = Some(receiver);
        let mut handlers = Vec::new();
        while !stopping.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let first = count.fetch_add(1, Ordering::SeqCst) == 0;
                    let release = if first { receiver.take() } else { None };
                    handlers.push(std::thread::spawn(move || {
                        let mut request = Vec::new();
                        let mut byte = [0u8; 1];
                        while !request.ends_with(b"\r\n\r\n") {
                            if stream.read(&mut byte).unwrap_or(0) == 0 {
                                return;
                            }
                            request.push(byte[0]);
                            assert!(request.len() < 8192);
                        }
                        if !matches!(mode, ResponseMode::Exact) {
                            let framing = match mode {
                                ResponseMode::OversizedDeclared => {
                                    format!("Content-Length: {}\r\n", BODY.len() + 1)
                                }
                                ResponseMode::ExactChunked | ResponseMode::OversizedChunked => {
                                    "Transfer-Encoding: chunked\r\n".to_owned()
                                }
                                ResponseMode::ExactCloseDelimited
                                | ResponseMode::OversizedCloseDelimited => String::new(),
                                ResponseMode::Exact => unreachable!(),
                            };
                            write!(
                                stream,
                                "HTTP/1.1 200 OK\r\n{framing}Connection: close\r\n\r\n"
                            )
                            .unwrap();
                            let chunked = matches!(
                                mode,
                                ResponseMode::ExactChunked | ResponseMode::OversizedChunked
                            );
                            if chunked {
                                write!(stream, "{:x}\r\n", BODY.len()).unwrap();
                            }
                            let _ = stream.write_all(BODY);
                            if chunked {
                                let _ = stream.write_all(b"\r\n");
                            }
                            let _ = stream.flush();
                            if matches!(
                                mode,
                                ResponseMode::ExactChunked | ResponseMode::ExactCloseDelimited
                            ) {
                                if chunked {
                                    let _ = stream.write_all(b"0\r\n\r\n");
                                }
                                return;
                            }
                            let release = release.unwrap();
                            let _ = release.recv_timeout(Duration::from_secs(5));
                            let overflow: &[u8] = if matches!(mode, ResponseMode::OversizedChunked)
                            {
                                b"1\r\nx\r\n"
                            } else {
                                b"x"
                            };
                            let _ = stream.write_all(overflow);
                            let _ = stream.flush();
                            // Do not end the response until the test has checked
                            // the disk budget and early refusal, rather than EOF.
                            let _ = release.recv_timeout(Duration::from_secs(5));
                            if matches!(mode, ResponseMode::OversizedChunked) {
                                let _ = stream.write_all(b"0\r\n\r\n");
                            }
                            return;
                        }
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            BODY.len()
                        )
                        .unwrap();
                        if let Some(release) = release {
                            stream.write_all(&BODY[..3]).unwrap();
                            stream.flush().unwrap();
                            let _ = release.recv_timeout(Duration::from_secs(5));
                            let _ = stream.write_all(&BODY[3..]);
                        } else {
                            stream.write_all(BODY).unwrap();
                        }
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => panic!("fixture server accept failed: {e}"),
            }
        }
        for handler in handlers {
            handler.join().unwrap();
        }
    });
    DownloadServer {
        file,
        requests,
        release,
        stop,
        thread: Some(thread),
    }
}

fn temporary_directory() -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("clipboard-ocr-download-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&path).unwrap();
    path
}

fn concurrent_install_scenario(shared_directory: bool) {
    let mut server = server();
    let directory = temporary_directory();
    let other_directory = (!shared_directory).then(temporary_directory);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (first_result, second_result, body, digest_matches) = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let (ready, received) = tokio::sync::oneshot::channel();
        let file = server.file;
        let first_dir = directory.clone();
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            let mut ready = Some(ready);
            download_ppocr_models(&first_client, &first_dir, [file], |progress| {
                if progress.current > 0 {
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(());
                    }
                }
            })
            .await
            .map(drop)
        });
        tokio::time::timeout(Duration::from_secs(5), received)
            .await
            .unwrap()
            .unwrap();
        let second = download_ppocr_models(
            &client,
            other_directory.as_ref().unwrap_or(&directory),
            [file],
            |_| {},
        )
        .await
        .map(drop);
        server.release.send(()).unwrap();
        let first = first.await.unwrap();
        let body = fs::read(directory.join(file.filename)).ok();
        let digest_matches = ocr::models::model_digest_matches(&directory, &file);
        (first, second, body, digest_matches)
    });
    server.shutdown();
    let requests = server.requests.load(Ordering::SeqCst);
    fs::remove_dir_all(directory).unwrap();
    if let Some(other) = other_directory {
        fs::remove_dir_all(other).unwrap();
    }
    if shared_directory {
        assert!(
        second_result
            .as_ref()
            .is_err_and(|e| e.contains("already in progress")),
        "second install entered the shared partial file: {second_result:?}; first={first_result:?}; requests={requests}"
    );
        assert_eq!(
            requests, 1,
            "a competing install must not start another request"
        );
    } else {
        assert!(
            second_result.is_ok(),
            "independent directory was blocked: {second_result:?}"
        );
        assert_eq!(requests, 2);
    }
    assert!(
        first_result.is_ok(),
        "first verified download failed: {first_result:?}"
    );
    assert_eq!(body.as_deref(), Some(BODY));
    assert!(digest_matches);
}

#[test]
fn concurrent_model_installs_do_not_replace_an_active_partial_download() {
    concurrent_install_scenario(true);
}

#[test]
fn concurrent_model_installs_in_independent_directories_can_progress() {
    concurrent_install_scenario(false);
}

#[test]
fn failed_model_download_releases_the_directory_lease_for_retry_and_cache_reuse() {
    let mut server = server();
    server.release.send(()).unwrap();
    let directory = temporary_directory();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (failed, retry, cached, bytes) = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let invalid = ocr::models::PpOcrModelFile {
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            ..server.file
        };
        let failed = download_ppocr_models(&client, &directory, [invalid], |_| {})
            .await
            .map(drop);
        let retry = download_ppocr_models(&client, &directory, [server.file], |_| {})
            .await
            .map(drop);
        let cached = download_ppocr_models(&client, &directory, [server.file], |_| {})
            .await
            .map(drop);
        let bytes = fs::read(directory.join(server.file.filename)).ok();
        (failed, retry, cached, bytes)
    });
    server.shutdown();
    let requests = server.requests.load(Ordering::SeqCst);
    fs::remove_dir_all(directory).unwrap();
    assert!(
        failed.as_ref().is_err_and(|e| e.contains("SHA-256")),
        "{failed:?}"
    );
    assert!(
        retry.is_ok(),
        "retry must acquire the released lease: {retry:?}"
    );
    assert!(
        cached.is_ok(),
        "cached install must acquire the released lease: {cached:?}"
    );
    assert_eq!(requests, 2, "cache reuse must not download again");
    assert_eq!(bytes.as_deref(), Some(BODY));
}

#[test]
fn completed_download_keeps_the_lease_until_activation_caller_releases_it() {
    let mut server = server();
    server.release.send(()).unwrap();
    let directory = temporary_directory();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (blocked, retry) = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let installation = download_ppocr_models(&client, &directory, [server.file], |_| {})
            .await
            .unwrap();
        let blocked = download_ppocr_models(&client, &directory, [server.file], |_| {})
            .await
            .map(drop);
        drop(installation);
        let retry = download_ppocr_models(&client, &directory, [server.file], |_| {})
            .await
            .map(drop);
        (blocked, retry)
    });
    server.shutdown();
    let requests = server.requests.load(Ordering::SeqCst);
    fs::remove_dir_all(directory).unwrap();
    assert!(
        blocked
            .as_ref()
            .is_err_and(|e| e.contains("already in progress")),
        "lease was released before activation: {blocked:?}"
    );
    assert!(
        retry.is_ok(),
        "retry failed after caller released the lease: {retry:?}"
    );
    assert_eq!(requests, 1);
}

fn oversized_download_scenario(mode: ResponseMode) {
    let mut server = server_with_response(mode);
    let directory = temporary_directory();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let progress_bytes = Arc::new(AtomicUsize::new(0));
    let progress = progress_bytes.clone();
    let (result, partial_size, activated) = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let file = server.file;
        let download_dir = directory.clone();
        let (ready, received) = tokio::sync::oneshot::channel();
        let mut download = tokio::spawn(async move {
            let mut ready = Some(ready);
            download_ppocr_models(&client, &download_dir, [file], |event| {
                progress.fetch_max(event.current as usize, Ordering::SeqCst);
                if event.current > 0 {
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(());
                    }
                }
            })
            .await
            .map(drop)
        });
        if !matches!(mode, ResponseMode::OversizedDeclared) {
            tokio::time::timeout(Duration::from_secs(5), received)
                .await
                .unwrap()
                .unwrap();
            // The valid prefix must be written before sending the first byte
            // beyond the known asset size. EOF remains held by the fixture.
            assert_eq!(
                fs::metadata(directory.join(format!("{}.part", file.filename)))
                    .unwrap()
                    .len(),
                file.size_bytes
            );
            server.release.send(()).unwrap();
        }
        let result = match tokio::time::timeout(Duration::from_secs(1), &mut download).await {
            Ok(result) => Some(result.unwrap()),
            Err(_) => {
                download.abort();
                let _ = download.await;
                None
            }
        };
        let partial_size = fs::metadata(directory.join(format!("{}.part", file.filename)))
            .ok()
            .map(|metadata| metadata.len());
        let activated = directory.join(file.filename).exists()
            || directory.join(format!("{}.sha256", file.filename)).exists();
        (result, partial_size, activated)
    });
    // Both fixture pauses must be releasable even when the assertion fails.
    let _ = server.release.send(());
    let _ = server.release.send(());
    server.shutdown();
    fs::remove_dir_all(directory).unwrap();
    let written = progress_bytes.load(Ordering::SeqCst);
    assert!(
        result.as_ref().is_some_and(|result| result
            .as_ref()
            .is_err_and(|error| error.contains("size"))),
        "oversized response waited for EOF: result={result:?}, partial={partial_size:?}, progress={written}"
    );
    assert_eq!(partial_size, None, "rejected download left partial bytes");
    assert!(!activated, "oversized response activated a model");
    if matches!(mode, ResponseMode::OversizedDeclared) {
        assert_eq!(
            written, 0,
            "oversized header must be rejected before writing"
        );
    } else {
        assert_eq!(written, BODY.len(), "overflow chunk must not reach disk");
    }
}

#[test]
fn oversized_declared_model_download_is_rejected_before_writing() {
    oversized_download_scenario(ResponseMode::OversizedDeclared);
}

#[test]
fn oversized_chunked_model_download_is_rejected_before_writing_overflow() {
    oversized_download_scenario(ResponseMode::OversizedChunked);
}

#[test]
fn oversized_close_delimited_model_download_is_rejected_before_writing_overflow() {
    oversized_download_scenario(ResponseMode::OversizedCloseDelimited);
}

fn valid_download_without_length_scenario(mode: ResponseMode) {
    let mut server = server_with_response(mode);
    let directory = temporary_directory();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut progress = Vec::new();
    let result = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        download_ppocr_models(&client, &directory, [server.file], |event| {
            progress.push((event.current, event.total));
        })
        .await
        .map(drop)
    });
    let body = fs::read(directory.join(server.file.filename)).ok();
    let digest_matches = ocr::models::model_digest_matches(&directory, &server.file);
    let partial = directory
        .join(format!("{}.part", server.file.filename))
        .exists();
    server.shutdown();
    fs::remove_dir_all(directory).unwrap();
    assert!(
        result.is_ok(),
        "valid unbounded framing was rejected: {result:?}"
    );
    assert_eq!(body.as_deref(), Some(BODY));
    assert!(digest_matches);
    assert!(!partial);
    assert_eq!(
        progress.last(),
        Some(&(BODY.len() as u64, BODY.len() as u64))
    );
    assert!(progress
        .iter()
        .all(|(_, total)| *total == BODY.len() as u64));
}

#[test]
fn valid_chunked_model_download_uses_the_pinned_size_and_digest() {
    valid_download_without_length_scenario(ResponseMode::ExactChunked);
}

#[test]
fn valid_close_delimited_model_download_uses_the_pinned_size_and_digest() {
    valid_download_without_length_scenario(ResponseMode::ExactCloseDelimited);
}
