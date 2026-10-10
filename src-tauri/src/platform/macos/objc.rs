//! Hand-rolled Objective-C message-send helpers.
//!
//! Live code: used by this adapter, `platform/quick_paste.rs` and
//! `platform/ui/window.rs` for NSPasteboard, NSWorkspace and NSWindow calls.

#![allow(non_camel_case_types, dead_code, clashing_extern_declarations)]

#[repr(C)]
pub struct Object([u8; 0]);
pub type Sel = *mut Object;
pub type Id = *mut Object;

pub const YES: i8 = 1;
pub const NO: i8 = 0;

extern "C" {
    pub fn objc_getClass(name: *const i8) -> Id;
    pub fn sel_registerName(name: *const i8) -> Sel;
    #[link_name = "objc_msgSend"]
    pub fn msgSend(receiver: Id, sel: Sel) -> Id;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_isize(receiver: Id, sel: Sel) -> isize;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_ptr(receiver: Id, sel: Sel) -> *const i8;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_id_id(receiver: Id, sel: Sel, arg: Id) -> Id;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_id_Int(receiver: Id, sel: Sel, arg: isize) -> Id;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_i32_Id(receiver: Id, sel: Sel, arg: i32) -> Id;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_bool_usize(receiver: Id, sel: Sel, arg: usize) -> i8;
    #[link_name = "objc_msgSend"]
    pub fn msgSend_void_f64(receiver: Id, sel: Sel, arg: f64);
    pub fn objc_autoreleasePoolPush() -> Id;
    pub fn objc_autoreleasePoolPop(pool: Id);
}

pub fn nsstring_from_str(s: &str) -> Id {
    let cls = unsafe { objc_getClass(c"NSString".as_ptr()) };
    let sel = unsafe { sel_registerName(c"stringWithUTF8String:".as_ptr()) };
    let cstr = std::ffi::CString::new(s).unwrap_or_default();
    unsafe { msgSend_id_id(cls, sel, cstr.as_ptr() as Id) }
}

pub fn nsstring_to_str(s: Id) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let sel = unsafe { sel_registerName(c"UTF8String".as_ptr()) };
    let ptr = unsafe { msgSend_ptr(s, sel) };
    if ptr.is_null() {
        None
    } else {
        unsafe { Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()) }
    }
}

pub fn get_nspasteboard() -> Id {
    let cls = unsafe { objc_getClass(c"NSPasteboard".as_ptr()) };
    let sel = unsafe { sel_registerName(c"generalPasteboard".as_ptr()) };
    unsafe { msgSend(cls, sel) }
}

pub fn pasteboard_change_count(pb: Id) -> isize {
    let sel = unsafe { sel_registerName(c"changeCount".as_ptr()) };
    unsafe { msgSend_isize(pb, sel) }
}

pub fn pasteboard_string_for_type(pb: Id, type_name: &str) -> Option<String> {
    let type_ns = nsstring_from_str(type_name);
    let sel = unsafe { sel_registerName(c"stringForType:".as_ptr()) };
    let result = unsafe { msgSend_id_id(pb, sel, type_ns) };
    nsstring_to_str(result)
}

/// `propertyListForType:` result (an `NSArray` for file lists, nil when
/// the pasteboard has no such type).
pub fn pasteboard_property_list_for_type(pb: Id, type_name: &str) -> Id {
    let type_ns = nsstring_from_str(type_name);
    let sel = unsafe { sel_registerName(c"propertyListForType:".as_ptr()) };
    unsafe { msgSend_id_id(pb, sel, type_ns) }
}

pub fn array_count(array: Id) -> isize {
    let sel = unsafe { sel_registerName(c"count".as_ptr()) };
    unsafe { msgSend_isize(array, sel) }
}

pub fn array_object_at_index(array: Id, index: isize) -> Id {
    let sel = unsafe { sel_registerName(c"objectAtIndex:".as_ptr()) };
    unsafe { msgSend_id_Int(array, sel, index) }
}

pub fn get_nsworkspace() -> Id {
    let cls = unsafe { objc_getClass(c"NSWorkspace".as_ptr()) };
    let sel = unsafe { sel_registerName(c"sharedWorkspace".as_ptr()) };
    unsafe { msgSend(cls, sel) }
}

pub fn workspace_frontmost_app(ws: Id) -> Id {
    let sel = unsafe { sel_registerName(c"frontmostApplication".as_ptr()) };
    unsafe { msgSend(ws, sel) }
}

pub fn running_app_name(app: Id) -> Option<String> {
    let sel = unsafe { sel_registerName(c"localizedName".as_ptr()) };
    let result = unsafe { msgSend(app, sel) };
    nsstring_to_str(result)
}

pub fn running_app_exe_path(app: Id) -> Option<String> {
    let sel = unsafe { sel_registerName(c"executableURL".as_ptr()) };
    let url = unsafe { msgSend(app, sel) };
    if url.is_null() {
        return None;
    }
    let sel_path = unsafe { sel_registerName(c"path".as_ptr()) };
    let path_str = unsafe { msgSend(url, sel_path) };
    nsstring_to_str(path_str)
}
