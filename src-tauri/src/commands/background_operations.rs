use crate::background_operations::{BackgroundOperations, OperationKind, OperationSnapshot};

#[tauri::command]
pub fn get_background_operation(
    operations: tauri::State<'_, BackgroundOperations>,
    kind: OperationKind,
) -> Option<OperationSnapshot> {
    operations.snapshot(kind)
}

#[tauri::command]
pub fn cancel_background_operation(
    operations: tauri::State<'_, BackgroundOperations>,
    kind: OperationKind,
    id: String,
) -> bool {
    operations.cancel(kind, &id)
}
