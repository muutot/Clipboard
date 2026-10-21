use std::sync::Mutex;

use super::collector::{
    collect_ocr_memory, collect_processes, directory_size, summarize_process_group,
};

#[cfg(target_os = "windows")]
use super::collector::collect_system_memory;

use super::types::MemoryProcess;

use crate::config::ConfigStore;
use crate::storage::StoragePaths;

#[test]
fn group_sums_available_process_metrics_without_panicking_on_missing_values() {
    let processes = vec![
        MemoryProcess {
            pid: 1,
            parent_pid: None,
            name: "main".to_owned(),
            role: Some("main".to_owned()),
            working_set_bytes: Some(10),
            private_bytes: Some(20),
            private_working_set_bytes: Some(8),
            virtual_bytes: Some(30),
        },
        MemoryProcess {
            pid: 2,
            parent_pid: Some(1),
            name: "child".to_owned(),
            role: Some("child".to_owned()),
            working_set_bytes: None,
            private_bytes: Some(4),
            private_working_set_bytes: None,
            virtual_bytes: Some(5),
        },
    ];
    let group = summarize_process_group(processes);
    assert_eq!(group.private_working_set_bytes, Some(8));
    assert_eq!(group.working_set_bytes, 10);
    assert_eq!(group.private_bytes, 24);
    assert_eq!(group.virtual_bytes, 35);
    assert_eq!(group.processes.len(), 2);
}

#[test]
fn group_private_working_set_is_unknown_when_every_process_omits_it() {
    let processes = vec![MemoryProcess {
        pid: 1,
        parent_pid: None,
        name: "main".to_owned(),
        role: Some("main".to_owned()),
        working_set_bytes: Some(10),
        private_bytes: None,
        private_working_set_bytes: None,
        virtual_bytes: Some(30),
    }];

    let group = summarize_process_group(processes);
    assert_eq!(group.private_working_set_bytes, None);
    assert_eq!(group.working_set_bytes, 10);
}

#[test]
fn process_group_serializes_private_working_set_as_camel_case() {
    let group = summarize_process_group(vec![MemoryProcess {
        pid: 1,
        parent_pid: None,
        name: "main".to_owned(),
        role: Some("main".to_owned()),
        working_set_bytes: Some(10),
        private_bytes: Some(20),
        private_working_set_bytes: Some(8),
        virtual_bytes: Some(30),
    }]);

    let value = serde_json::to_value(group).unwrap();
    assert_eq!(value["privateWorkingSetBytes"], 8);
    assert_eq!(value["workingSetBytes"], 10);
}

#[test]
fn memory_process_serializes_camel_case_and_preserves_missing_values() {
    let process = MemoryProcess {
        pid: 7,
        parent_pid: Some(3),
        name: "worker".to_owned(),
        role: Some("child".to_owned()),
        working_set_bytes: Some(10),
        private_bytes: None,
        private_working_set_bytes: Some(8),
        virtual_bytes: None,
    };
    let value = serde_json::to_value(process).unwrap();
    assert_eq!(value["parentPid"], 3);
    assert_eq!(value["privateWorkingSetBytes"], 8);
    assert!(value["privateBytes"].is_null());
}

#[test]
fn directory_size_ignores_missing_roots() {
    let path =
        std::env::temp_dir().join(format!("clipboard-memory-missing-{}", std::process::id()));
    assert_eq!(directory_size(&path), (0, 0));
}

#[test]
fn ocr_diagnostics_report_a_missing_model_as_not_installed() {
    let root = std::env::temp_dir().join(format!(
        "clipboard-memory-ocr-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary project root");
    let config = ConfigStore::load(&root).expect("config store");
    let paths = StoragePaths::initialize(root.join("storage")).expect("storage paths");

    let diagnostics = collect_ocr_memory(&paths, &Mutex::new(config));

    // The configured PP-OCR variant has no files on disk, so the diagnostics
    // report an uninstalled model instead of a ready engine.
    assert_eq!(diagnostics.engine, "ppocr");
    assert_eq!(diagnostics.model_variant, "small");
    assert!(!diagnostics.model_installed);
    assert!(diagnostics.installed_variants.is_empty());
    assert_eq!(diagnostics.model_bytes, 0);
    assert_eq!(diagnostics.model_file_count, 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn process_probe_always_includes_the_current_process() {
    let current_pid = std::process::id();
    let processes = collect_processes(current_pid);
    let _current = processes
        .iter()
        .find(|process| process.pid == current_pid)
        .expect("current process should be present in diagnostics");

    #[cfg(target_os = "windows")]
    {
        let current = _current;
        assert!(current.working_set_bytes.is_some_and(|bytes| bytes > 0));
        assert!(current.private_bytes.is_some_and(|bytes| bytes > 0));
        assert!(current
            .private_working_set_bytes
            .is_some_and(|bytes| bytes > 0));
        assert!(current.virtual_bytes.is_some_and(|bytes| bytes > 0));
        assert!(current.private_working_set_bytes <= current.working_set_bytes);
        let system = collect_system_memory();
        assert!(system.total_bytes.is_some_and(|bytes| bytes > 0));
        assert!(system.available_bytes <= system.total_bytes);
    }
}
