use super::collector::{collect_processes, collect_system_memory, directory_size, summarize_process_group};
use super::types::MemoryProcess;

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
    assert_eq!(group.working_set_bytes, 10);
    assert_eq!(group.private_bytes, 24);
    assert_eq!(group.virtual_bytes, 35);
    assert_eq!(group.processes.len(), 2);
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
