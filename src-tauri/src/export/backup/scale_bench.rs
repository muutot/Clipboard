//! Opt-in process-memory measurement, with generated data only and no timing gate.
use super::*;
use crate::performance::MemoryMonitor;
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Write one row at a time so fixture generation does not retain the workload.
fn fixture(path: &Path, count: usize) -> u64 {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    zip.start_file(
        "records.jsonl",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    for index in 0..count {
        let mut item = super::tests::item(&format!("bench-{index:06}"), ClipboardKind::Text, None);
        item.text_content = Some(format!("{index:06} {}", "x".repeat(2048)));
        item.size_bytes = item.text_content.as_ref().unwrap().len() as u64;
        item.preview_path = None;
        item.icon_path = None;
        let mut row = serde_json::to_vec(&item).unwrap();
        row.push(b'\n');
        hash.update(&row);
        bytes += row.len() as u64;
        zip.write_all(&row).unwrap();
    }
    let manifest = Manifest {
        version: 1,
        item_count: count,
        entries: vec![Entry {
            name: "records.jsonl".into(),
            bytes,
            sha256: hex::encode(hash.finalize()),
        }],
    };
    zip.start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap();
    bytes
}

#[test]
#[ignore = "synthetic memory benchmark; set CLIPBOARD_BACKUP_BENCH_ITEMS and run with --ignored --nocapture --test-threads=1"]
fn preview_large_backup() {
    let count: usize = std::env::var("CLIPBOARD_BACKUP_BENCH_ITEMS")
        .expect("set CLIPBOARD_BACKUP_BENCH_ITEMS to 1000, 10000 or 100000")
        .parse()
        .unwrap();
    assert!([1000, 10000, 100000].contains(&count));
    let root = Scratch::at(&std::env::temp_dir()).unwrap();
    let archive = root.0.join("synthetic.clipbackup");
    let jsonl_bytes = fixture(&archive, count);
    let database = Database::open(root.0.join("target.sqlite3")).unwrap();
    let baseline = MemoryMonitor::new().current_usage_bytes();
    let (stop, stopped) = mpsc::channel::<()>();
    let sampler = thread::spawn(move || {
        let monitor = MemoryMonitor::new();
        loop {
            monitor.record_snapshot();
            match stopped.recv_timeout(Duration::from_millis(20)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => return monitor.snapshot().peak_bytes,
            }
        }
    });
    let started = Instant::now();
    let outcome = preview(&archive, &database);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(stop);
    let peak = sampler.join().unwrap();
    let result = outcome.unwrap();
    assert_eq!(result.item_count, count);
    assert_eq!(result.duplicate_count, 0);
    assert_eq!(database.item_count().unwrap(), 0);
    eprintln!(
        "[backup-bench] {}",
        serde_json::json!({
            "records": count, "jsonlBytes": jsonl_bytes, "elapsedMs": elapsed_ms,
            "baselineRssBytes": baseline, "peakRssBytes": peak,
            "rssIncreaseBytes": peak.saturating_sub(baseline),
        })
    );
}
