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
