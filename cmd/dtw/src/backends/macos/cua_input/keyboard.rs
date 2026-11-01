// Adapted from trycua/cua b38360443960a6540685a9a5eb5407f10fe3d03b.
// Source: libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs
// Copyright (c) 2025 Cua AI, Inc. MIT. See docs/background-input-port.md.

use core_graphics::{
    event::{CGEvent, CGEventFlags},
    event_source::{CGEventSource, CGEventSourceStateID},
};

pub fn press_key(pid: i32, key: &str, modifiers: &[&str]) -> anyhow::Result<()> {
    // Handle "+" / "plus" → Shift+= (US keyboard layout).
    if key == "+" || key.to_lowercase() == "plus" {
        let flags = modifier_flags(&["shift"]);
        let eq_code = key_name_to_code("=")?;
        post_key(pid, eq_code, true, modifier_flags(modifiers) | flags)?;
        key_gap_sleep();
        post_key(pid, eq_code, false, modifier_flags(modifiers) | flags)?;
        return Ok(());
    }

    let key_code = key_name_to_code(key)?;
    let flags = modifier_flags(modifiers);

    post_key(pid, key_code, true, flags)?;
    key_gap_sleep();
    post_key(pid, key_code, false, flags)?;
    Ok(())
}

fn control_key_for_char(ch: char) -> Option<u16> {
    match ch {
        '\n' | '\r' => key_name_to_code("return").ok(),
        '\t' => key_name_to_code("tab").ok(),
        _ => None,
    }
}

fn char_key_events(source: &CGEventSource, ch: char) -> anyhow::Result<(CGEvent, CGEvent)> {
    let (key_code, payload) = match control_key_for_char(ch) {
        Some(code) => (code, None),
        None => (0, Some(ch.to_string())),
    };
    let make = |down: bool| -> anyhow::Result<CGEvent> {
        let event = CGEvent::new_keyboard_event(source.clone(), key_code, down)
            .map_err(|_| anyhow::anyhow!("CGEvent keyboard event failed"))?;
        if let Some(payload) = &payload {
            event.set_string(payload);
        }
        event.set_flags(CGEventFlags::CGEventFlagNull);
        Ok(event)
    };
    Ok((make(true)?, make(false)?))
}

fn normalize_text(text: &str) -> String {
    // Preserve the existing DTW literal-text contract: CRLF is one Return.
    text.replace("\r\n", "\n")
}

pub fn type_text(pid: i32, text: &str) -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;

    for ch in normalize_text(text).chars() {
        // Event construction has no desktop side effect. Keep local stop/error
        // checks here; post_event_via validates the full target immediately before
        // every KeyDown and the original input owner before each paired KeyUp.
        // A second full AX traversal before allocation only spends call budget.
        super::control::check_completed()?;
        let (down, up) = char_key_events(&source, ch)?;
        post_keyboard_event(pid, &down);
        key_gap_sleep();
        post_keyboard_event(pid, &up);
        key_gap_sleep();
    }
    Ok(())
}

fn modifier_key_code_and_flag(modifier: &str) -> Option<(u16, CGEventFlags)> {
    match modifier.to_lowercase().as_str() {
        "cmd" | "command" | "super" | "meta" | "win" => {
            Some((55, CGEventFlags::CGEventFlagCommand))
        }
        "shift" => Some((56, CGEventFlags::CGEventFlagShift)),
        "option" | "alt" => Some((58, CGEventFlags::CGEventFlagAlternate)),
        "ctrl" | "control" => Some((59, CGEventFlags::CGEventFlagControl)),
        "fn" => Some((63, CGEventFlags::CGEventFlagSecondaryFn)),
        _ => None,
    }
}

pub(super) fn with_pid_modifier_keys<T>(
    pid: i32,
    modifiers: &[&str],
    gesture: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mut active_flags = CGEventFlags::CGEventFlagNull;
    let mut pressed = Vec::new();
    for modifier in modifiers {
        let Some((key_code, flag)) = modifier_key_code_and_flag(modifier) else {
            continue;
        };
        if pressed
            .iter()
            .any(|(pressed_code, _)| *pressed_code == key_code)
        {
            continue;
        }
        active_flags |= flag;
        if let Err(error) = post_key(pid, key_code, true, active_flags) {
            release_pid_modifiers(pid, &pressed, active_flags);
            return Err(error);
        }
        pressed.push((key_code, flag));
        key_gap_sleep();
    }

    let result = gesture();
    release_pid_modifiers(pid, &pressed, active_flags);
    result
}

fn release_pid_modifiers(
    pid: i32,
    pressed: &[(u16, CGEventFlags)],
    mut active_flags: CGEventFlags,
) {
    for &(key_code, flag) in pressed.iter().rev() {
        active_flags.remove(flag);
        let _ = post_key(pid, key_code, false, active_flags);
        key_gap_sleep();
    }
}

fn post_key(pid: i32, key_code: u16, key_down: bool, flags: CGEventFlags) -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("CGEventSource::new failed"))?;
    let event = CGEvent::new_keyboard_event(source, key_code, key_down)
        .map_err(|_| anyhow::anyhow!("CGEvent::new_keyboard_event failed"))?;
    // HIDSystemState can inherit physically held modifiers. Always overwrite
    // the event flags, including the empty case, so an unrelated Shift/Caps
    // state cannot leak into a targeted key press.
    event.set_flags(flags);
    post_keyboard_event(pid, &event);
    super::control::check_completed()
}

fn modifier_flags(modifiers: &[&str]) -> CGEventFlags {
    let mut flags = CGEventFlags::CGEventFlagNull;
    for m in modifiers {
        match m.to_lowercase().as_str() {
            "cmd" | "command" | "super" | "meta" | "win" => {
                flags |= CGEventFlags::CGEventFlagCommand
            }
            "shift" => flags |= CGEventFlags::CGEventFlagShift,
            "option" | "alt" => flags |= CGEventFlags::CGEventFlagAlternate,
            "ctrl" | "control" => flags |= CGEventFlags::CGEventFlagControl,
            "fn" => flags |= CGEventFlags::CGEventFlagSecondaryFn,
            _ => {}
        }
    }
    flags
}

fn canonical_key_name(key: &str) -> String {
    let lower = key.trim().to_lowercase();
    if lower.chars().count() < 2 {
        return lower;
    }
    let compact: String = lower
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect();
    match compact.as_str() {
        "pgdn" | "pgdown" => "pagedown",
        "pgup" => "pageup",
        "arrowleft" | "leftarrow" => "left",
        "arrowright" | "rightarrow" => "right",
        "arrowup" | "uparrow" => "up",
        "arrowdown" | "downarrow" => "down",
        "forwarddelete" | "fwddelete" | "deleteforward" => "forward_delete",
        "bksp" => "backspace",
        "escape" => "escape",
        "kpenter" | "numpadenter" => "enter",
        _ => return compact,
    }
    .to_owned()
}

pub(super) fn key_name_to_code(key: &str) -> anyhow::Result<u16> {
    let code = match canonical_key_name(key).as_str() {
        "return" | "enter" => 36,
        "tab" => 48,
        "space" => 49,
        "delete" | "backspace" => 51,
        "escape" | "esc" => 53,
        "command" | "cmd" | "super" | "meta" | "win" => 55,
        "shift" => 56,
        "capslock" => 57,
        "option" | "alt" => 58,
        "control" | "ctrl" => 59,
        "fn" => 63,
        "home" => 115,
        "pageup" => 116,
        "del" | "forward_delete" => 117,
        "end" => 119,
        "pagedown" => 121,
        "left" | "left_arrow" => 123,
        "right" | "right_arrow" => 124,
        "down" | "down_arrow" => 125,
        "up" | "up_arrow" => 126,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "=" => 24,
        "9" => 25,
        "7" => 26,
        "-" => 27,
        "8" => 28,
        "0" => 29,
        "]" => 30,
        "o" => 31,
        "u" => 32,
        "[" => 33,
        "i" => 34,
        "p" => 35,
        "l" => 37,
        "j" => 38,
        "'" => 39,
        "k" => 40,
        ";" => 41,
        "\\" => 42,
        "," => 43,
        "/" => 44,
        "n" => 45,
        "m" => 46,
        "." => 47,
        "`" => 50,
        _ => anyhow::bail!("{}", unknown_key_message(key)),
    };
    Ok(code)
}

fn unknown_key_message(key: &str) -> String {
    format!("Unknown key name: {key}. Pass one named key per keys array entry, with modifiers first (for example [\"META\",\"A\"]); use typeText for text characters.")
}

fn key_gap_sleep() {
    super::control::sleep(std::time::Duration::from_millis(8));
}
fn post_keyboard_event(pid: i32, event: &CGEvent) {
    super::control::post_event(pid, event, true);
}

pub(super) fn is_modifier(key: &str) -> bool {
    modifier_key_code_and_flag(key).is_some()
}

#[cfg(test)]
mod tests {
    #[test]
    fn crlf_is_one_return_without_dropping_standalone_control_characters() {
        assert_eq!(super::normalize_text("a\r\nb\rc\n😀"), "a\nb\rc\n😀");
    }
    use super::*;
    #[test]
    fn dtw_named_keys_keep_their_native_codes() {
        for (name, code) in [
            ("META", 55),
            ("CTRL", 59),
            ("ALT", 58),
            ("SHIFT", 56),
            ("BACKSPACE", 51),
            ("ENTER", 36),
            ("ESC", 53),
            ("LEFT", 123),
            ("RIGHT", 124),
            ("UP", 126),
            ("DOWN", 125),
            ("HOME", 115),
            ("END", 119),
            ("PAGEUP", 116),
            ("PAGEDOWN", 121),
            ("F1", 122),
            ("F2", 120),
            ("F3", 99),
            ("F4", 118),
            ("F5", 96),
            ("F6", 97),
            ("F7", 98),
            ("F8", 100),
            ("F9", 101),
            ("F10", 109),
            ("F11", 103),
            ("F12", 111),
        ] {
            assert_eq!(key_name_to_code(name).unwrap(), code, "{name}");
        }
    }
    #[test]
    fn newline_and_tab_are_real_controls() {
        assert_eq!(control_key_for_char('\n'), Some(36));
        assert_eq!(control_key_for_char('\r'), Some(36));
        assert_eq!(control_key_for_char('\t'), Some(48));
        assert_eq!(control_key_for_char('中'), None);
    }
}
