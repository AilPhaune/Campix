use core::cmp::Ordering;

use alloc::collections::{btree_map::Entry, BTreeMap};

use crate::{debuggable_bitset_enum, println};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyboardEventKind {
    KeyDown,
    KeyRepeat,
    KeyUp,
}

debuggable_bitset_enum!(
    u16,
    pub enum KeyModifier {
        LeftShift = 1,
        LeftControl = 2,
        LeftAlt = 4,
        Windows = 8,
        NumLock = 16,
        CapsLock = 32,
        ScrollLock = 64,
        RightShift = 128,
        RightControl = 256,
        RightAlt = 512,
    },
    KeyModifiers
);

/// Represents a key on the Multimedia section of the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum MultimediaKey {
    NextTrack,
    Mute,
    Calculator,
    Play,
    Stop,
    VolumeDown,
    VolumeUp,
    WWWSearch,
    WWWFavorites,
    WWWRefresh,
    WWWStop,
    WWWForward,
    WWWBack,
    MyComputer,
    Email,
    MediaSelect,
}

/// Represents a key on the ACPI section of the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum AcpiKey {
    Power,
    Sleep,
    Wake,
}

/// Represents a key on the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum Key {
    Escape,
    Character(char),
    Backspace,
    Tab,
    Enter,
    LeftControl,
    LeftShift,
    RightShift,
    Keypad(char),
    LeftAlt,
    Space,
    CapsLock,
    F(usize),
    NumLock,
    ScrollLock,
    Multimedia(MultimediaKey),
    KeypadEnter,
    RightControl,
    RightAlt,
    Home,
    CursorUp,
    PageUp,
    CursorLeft,
    CursorRight,
    End,
    CursorDown,
    PageDown,
    Insert,
    Delete,
    LeftGui,
    RightGui,
    Apps,
    Acpi(AcpiKey),
}

impl Key {
    /// Returns the modifier(s) that this key is associated with
    pub fn modifiers(&self) -> KeyModifiers {
        match self {
            Key::LeftControl => KeyModifier::LeftControl,
            Key::RightControl => KeyModifier::RightControl,
            Key::LeftShift => KeyModifier::LeftShift,
            Key::RightShift => KeyModifier::RightShift,
            Key::LeftAlt => KeyModifier::LeftAlt,
            Key::RightAlt => KeyModifier::RightAlt,
            _ => return KeyModifiers::empty(),
        }
        .into()
    }

    /// Returns whether or not this key is associated to a printable character
    pub const fn printable(&self) -> bool {
        matches!(
            self,
            Key::Character(_)
                | Key::Keypad(_)
                | Key::Space
                | Key::Tab
                | Key::Enter
                | Key::KeypadEnter
        )
    }

    /// Returns the printable character associated with this key if it exists
    pub const fn printable_char(&self) -> Option<char> {
        match self {
            Key::Character(c) => Some(*c),
            Key::Keypad(c) => Some(*c),
            Key::Space => Some(' '),
            Key::Tab => Some('\t'),
            Key::Enter => Some('\n'),
            Key::KeypadEnter => Some('\n'),
            _ => None,
        }
    }
}

/// Represents a keyboard event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardEvent {
    pub kind: KeyboardEventKind,
    pub modifiers: KeyModifiers,
    pub raw_key: Key,
    pub mapped_key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModifiedKey(Key, KeyModifiers);

impl PartialOrd for ModifiedKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModifiedKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(Ordering::Equal)
            .then(self.1.partial_cmp(&other.1).unwrap_or(Ordering::Equal))
    }
}

/// Maps a keyboard key to another, depending on the layout
pub struct KeyboardLayout {
    mappings: BTreeMap<ModifiedKey, Key>,
}

impl KeyboardLayout {
    pub fn default_en_us() -> KeyboardLayout {
        let mut layout = KeyboardLayout {
            mappings: BTreeMap::new(),
        };
        for letter in "abcdefghijklmnopqrstuvwxyz".chars() {
            layout.set_map(
                Key::Character(letter),
                KeyModifier::LeftShift.into(),
                Some(Key::Character(letter.to_ascii_uppercase())),
            );
            layout.set_map(
                Key::Character(letter),
                KeyModifier::RightShift.into(),
                Some(Key::Character(letter.to_ascii_uppercase())),
            );
            layout.set_map(
                Key::Character(letter),
                *KeyModifiers::empty()
                    .set(KeyModifier::LeftShift)
                    .set(KeyModifier::RightShift),
                Some(Key::Character(letter.to_ascii_uppercase())),
            );
        }
        layout
    }

    pub fn map(&self, key: Key, modifiers: KeyModifiers) -> Key {
        self.mappings
            .get(&ModifiedKey(key, modifiers))
            .copied()
            .unwrap_or(key)
    }

    pub fn set_map(&mut self, key: Key, modifiers: KeyModifiers, mapped_key: Option<Key>) {
        match self.mappings.entry(ModifiedKey(key, modifiers)) {
            Entry::Occupied(mut entry) => match mapped_key {
                Some(key) => *entry.get_mut() = key,
                None => {
                    entry.remove();
                }
            },
            Entry::Vacant(entry) => {
                if let Some(mapped_key) = mapped_key {
                    entry.insert(mapped_key);
                }
            }
        }
    }
}

/// Handles a keyboard event from the keyboard driver
pub fn handle_keyboard_event(event: KeyboardEvent) {
    println!("{:?}", event);
}
