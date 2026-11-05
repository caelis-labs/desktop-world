// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Cua AI, Inc.

//! Find the menu item a key combination stands for.
//!
//! Background key events reach a window's views but never its app's menu key
//! equivalents (see `input::keyboard::hotkey_no_auth`). A background
//! `cmd+s` to TextEdit was reported as "Pressed" and saved nothing. When a
//! combo is a menu item's key equivalent, `hotkey` acts on that menu item
//! through accessibility instead, or refuses when macOS keeps the item
//! disabled in the background.

// Adapted from trycua/cua 96b18ccd495b7a314592be982dd0d1b713049e40,
// menu_equivalent.rs and invoke_menu.rs. DTW supplies bounded AX reads,
// exact-window admission, one dispatch and the existing cleanup owner.
use super::{ax::Element, control};

/// `kAXMenuItemModifier*` bits of `AXMenuItemCmdModifiers`.
const MOD_SHIFT: i64 = 1 << 0;
const MOD_OPTION: i64 = 1 << 1;
const MOD_CONTROL: i64 = 1 << 2;
const MOD_NO_COMMAND: i64 = 1 << 3;

/// Upper bound on menu items read per lookup, so a huge dynamic menu cannot
/// stall the call.
const MAX_ITEMS: usize = 2000;

/// A hotkey as a menu key equivalent compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Combo {
    shift: bool,
    option: bool,
    control: bool,
    /// The uppercase character of the base key.
    character: String,
}

impl Combo {
    /// A combo with Command and one printable base key; `None` for anything
    /// else (no Command, a named key such as `return`), which keeps today's
    /// key-event route.
    pub(super) fn parse(modifiers: &[String], key: &str) -> Option<Self> {
        let mut cmd = false;
        let mut combo = Combo {
            shift: false,
            option: false,
            control: false,
            character: String::new(),
        };
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" | "meta" | "win" => cmd = true,
                "shift" => combo.shift = true,
                "option" | "alt" => combo.option = true,
                "ctrl" | "control" => combo.control = true,
                _ => return None,
            }
        }
        let mut chars = key.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return None;
        };
        if !cmd || ch.is_whitespace() || ch.is_control() {
            return None;
        }
        combo.character = ch.to_uppercase().collect();
        Some(combo)
    }

    /// Whether a menu item with this `AXMenuItemCmdChar` and
    /// `AXMenuItemCmdModifiers` fires on this combo.
    fn matches(&self, cmd_char: &str, modifiers: i64) -> bool {
        modifiers & MOD_NO_COMMAND == 0
            && cmd_char.to_uppercase() == self.character
            && (modifiers & MOD_SHIFT != 0) == self.shift
            && (modifiers & MOD_OPTION != 0) == self.option
            && (modifiers & MOD_CONTROL != 0) == self.control
    }
}

pub(super) struct MenuMatch {
    item: Element,
    pub enabled: bool,
}
pub(super) enum Lookup {
    Match(MenuMatch),
    NoMatch,
    Unavailable(&'static str),
}

struct Search {
    budget: usize,
    incomplete: bool,
}
impl Search {
    // CUA's finder skips unavailable attributes in unrelated menu branches.
    // Retain that behavior while keeping cancellation/deadlines terminal and
    // distinguishing an incomplete no-match from a proven absence.
    fn read<T>(&mut self, value: anyhow::Result<T>) -> anyhow::Result<Option<T>> {
        match value {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                control::check_local()?;
                self.incomplete = true;
                Ok(None)
            }
        }
    }
}
fn semantic_items(parent: &Element, search: &mut Search) -> anyhow::Result<Vec<Element>> {
    let mut items = Vec::new();
    for child in search.read(parent.children())?.unwrap_or_default() {
        if search.read(child.string("AXRole"))?.flatten().as_deref() == Some("AXMenu") {
            items.extend(search.read(child.children())?.unwrap_or_default());
        } else {
            items.push(child);
        }
    }
    Ok(items)
}

fn search(
    parent: &Element,
    combo: &Combo,
    found: &mut Option<MenuMatch>,
    state: &mut Search,
    depth: usize,
) -> anyhow::Result<()> {
    for item in semantic_items(parent, state)? {
        control::check_local()?;
        if found.as_ref().is_some_and(|m| m.enabled) {
            break;
        }
        anyhow::ensure!(state.budget > 0, "AX menu search limit exceeded");
        state.budget -= 1;
        let character = state
            .read(item.string("AXMenuItemCmdChar"))?
            .flatten()
            .unwrap_or_default();
        if !character.is_empty() {
            if character.to_uppercase() != combo.character {
                continue;
            }
            let Some(modifiers) = state.read(item.number("AXMenuItemCmdModifiers"))?.flatten()
            else {
                state.incomplete = true;
                continue;
            };
            if combo.matches(&character, modifiers) {
                let Some(enabled) = state.read(item.boolean("AXEnabled"))?.flatten() else {
                    state.incomplete = true;
                    continue;
                };
                if found.is_none() || enabled {
                    *found = Some(MenuMatch { item, enabled });
                }
            }
        } else if depth < 2 {
            // The local adapter retains the matching AX object, so no menu
            // path/title reads are needed merely to locate a shortcut.
            search(&item, combo, found, state, depth + 1)?;
        }
    }
    Ok(())
}

// CUA-1767: a document menu must address both the requested key and main
// window. A sibling/unknown context must never be silently switched here.
fn menu_window_matches(requested: u32, focused: Option<u32>, main: Option<u32>) -> bool {
    focused == Some(requested) && main == Some(requested)
}
fn bound(app: &Element, window: u32) -> anyhow::Result<bool> {
    Ok(menu_window_matches(
        window,
        app.element("AXFocusedWindow")?.and_then(|w| w.window_id()),
        app.element("AXMainWindow")?.and_then(|w| w.window_id()),
    ))
}

pub(super) fn find(pid: i32, window: u32, combo: &Combo) -> anyhow::Result<Lookup> {
    let app = Element::application(pid)?;
    let Some(bar) = app.element("AXMenuBar")? else {
        return Ok(Lookup::Unavailable("native_menu_unavailable"));
    };
    let mut found = None;
    let mut state = Search {
        budget: MAX_ITEMS,
        incomplete: false,
    };
    for (index, top) in semantic_items(&bar, &mut state)?.into_iter().enumerate() {
        let title = state
            .read(top.string("AXTitle"))?
            .flatten()
            .unwrap_or_default();
        if index > 0 || title != "Apple" {
            search(&top, combo, &mut found, &mut state, 0)?;
        }
        if found.as_ref().is_some_and(|m| m.enabled) {
            break;
        }
    }
    let Some(found) = found else {
        return Ok(if state.incomplete {
            Lookup::Unavailable("native_menu_unavailable")
        } else {
            Lookup::NoMatch
        });
    };
    if !found.enabled {
        // Real TextEdit control: CUA's no-raise preparation still left these
        // document menus disabled. Choose the first foreground payload here,
        // before any preparation, rather than silently posting or scripting
        // selected commands through a separate application-specific route.
        return Ok(Lookup::Unavailable("native_menu_disabled"));
    }
    if !bound(&app, window)? {
        return Ok(Lookup::Unavailable("native_menu_window_unbound"));
    }
    Ok(Lookup::Match(found))
}

impl MenuMatch {
    pub fn invoke(&self, pid: i32, window: u32, combo: &Combo) -> anyhow::Result<()> {
        let actions = self.item.actions()?;
        let action = ["AXPress", "AXPick", "AXConfirm"]
            .into_iter()
            .find(|name| actions.iter().any(|candidate| candidate == name))
            .ok_or_else(|| anyhow::anyhow!("AX menu item has no supported native action"))?;
        control::native_action(
            || {
                let app = Element::application(pid)?;
                anyhow::ensure!(
                    bound(&app, window)?,
                    "Requested document menu context changed"
                );
                let character = self.item.string("AXMenuItemCmdChar")?.unwrap_or_default();
                let modifiers = self.item.number("AXMenuItemCmdModifiers")?;
                anyhow::ensure!(
                    modifiers.is_some_and(|m| combo.matches(&character, m))
                        && self.item.boolean("AXEnabled")? == Some(true),
                    "Menu shortcut or enablement changed before dispatch"
                );
                Ok(())
            },
            || self.item.perform(action),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dynamic_shortcuts_match_exact_modifiers_without_key_whitelists() {
        let combo = |modifiers: &[&str], key| {
            Combo::parse(
                &modifiers.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
                key,
            )
        };
        for key in ["s", "o", "t", ",", "p", "f", "z"] {
            let plain = combo(&["META"], key).unwrap();
            assert!(plain.matches(key, 0));
            assert!(!plain.matches(key, MOD_SHIFT));
            assert!(!plain.matches(key, MOD_NO_COMMAND));
            assert!(combo(&["CMD", "SHIFT"], key)
                .unwrap()
                .matches(key, MOD_SHIFT));
        }
        for (mods, key) in [
            (&["CTRL"][..], "a"),
            (&["CMD"], "RETURN"),
            (&["ALT"], "LEFT"),
        ] {
            assert!(combo(mods, key).is_none());
        }
    }
    #[test]
    fn background_menus_require_the_exact_key_and_main_window() {
        assert!(menu_window_matches(7, Some(7), Some(7)));
        for (focused, main) in [
            (None, Some(7)),
            (Some(7), None),
            (Some(8), Some(7)),
            (Some(7), Some(8)),
        ] {
            assert!(!menu_window_matches(7, focused, main));
        }
    }
}
