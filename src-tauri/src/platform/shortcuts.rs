use std::collections::HashMap;

use crate::keyboard::ShortcutBinding;

pub struct GlobalShortcutManager {
    pub shortcuts: HashMap<String, Vec<ShortcutBinding>>,
    registered_ids: Vec<i32>,
}

impl Default for GlobalShortcutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalShortcutManager {
    pub fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
            registered_ids: Vec::new(),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn register_platform_hotkeys(&mut self, hwnd: isize) -> Result<(), String> {
        use crate::platform::windows_clipboard;

        self.unregister_platform_hotkeys(hwnd)?;

        let mut next_id: i32 = 1;
        for shortcuts in self.shortcuts.values() {
            for binding in shortcuts {
                if let crate::keyboard::ShortcutBinding::Chord { modifiers, key } = binding {
                    let mut mod_flags: u32 = 0;
                    for m in modifiers {
                        match m {
                            crate::keyboard::Modifier::Alt => {
                                mod_flags |= windows_clipboard::MOD_ALT
                            }
                            crate::keyboard::Modifier::Control => {
                                mod_flags |= windows_clipboard::MOD_CONTROL
                            }
                            crate::keyboard::Modifier::Shift => {
                                mod_flags |= windows_clipboard::MOD_SHIFT
                            }
                            crate::keyboard::Modifier::Meta => {
                                mod_flags |= windows_clipboard::MOD_WIN
                            }
                        }
                    }
                    let vk = match key.to_uppercase().as_str() {
                        "V" => windows_clipboard::VK_V,
                        "SPACE" => 0x20,
                        other if other.len() == 1 => {
                            let c = other.chars().next().unwrap();
                            if c.is_ascii_alphabetic() {
                                c as u8 as u32
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                    windows_clipboard::register_global_hotkey(hwnd, next_id, mod_flags, vk)?;
                    self.registered_ids.push(next_id);
                    next_id += 1;
                }
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn register_platform_hotkeys(&mut self, _hwnd: isize) -> Result<(), String> {
        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub fn unregister_platform_hotkeys(&mut self, hwnd: isize) -> Result<(), String> {
        use crate::platform::windows_clipboard;

        for id in &self.registered_ids {
            let _ = windows_clipboard::unregister_global_hotkey(hwnd, *id);
        }
        self.registered_ids.clear();
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn unregister_platform_hotkeys(&mut self, _hwnd: isize) -> Result<(), String> {
        self.registered_ids.clear();
        Ok(())
    }

    pub fn register(&mut self, action: &str, shortcuts: &[ShortcutBinding]) -> Result<(), String> {
        self.shortcuts.insert(action.to_owned(), shortcuts.to_vec());
        println!(
            "registered {} shortcut(s) for action: {}",
            shortcuts.len(),
            action
        );
        Ok(())
    }

    pub fn unregister_all(&mut self) -> Result<(), String> {
        self.shortcuts.clear();
        println!("all global shortcuts unregistered");
        Ok(())
    }
}
