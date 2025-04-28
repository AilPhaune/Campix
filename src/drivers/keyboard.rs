use core::ops::{Add, AddAssign, Sub, SubAssign};

use alloc::vec::Vec;

use crate::println;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyboardEventKind {
    KeyDown,
    KeyRepeat,
    KeyUp,
}

/// Represents key modifiers, such as Shift, Control, Alt, Windows, NumLock, CapsLock, ScrollLock, and more
#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum KeyModifiers {
    None = 0,
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
}

impl core::fmt::Debug for KeyModifiers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            write!(f, "None")
        } else {
            let mut modifiers = Vec::new();
            if self.contains_all(KeyModifiers::LeftShift) {
                modifiers.push("LeftShift");
            }
            if self.contains_all(KeyModifiers::LeftControl) {
                modifiers.push("LeftControl");
            }
            if self.contains_all(KeyModifiers::LeftAlt) {
                modifiers.push("LeftAlt");
            }
            if self.contains_all(KeyModifiers::RightShift) {
                modifiers.push("RightShift");
            }
            if self.contains_all(KeyModifiers::RightControl) {
                modifiers.push("RightControl");
            }
            if self.contains_all(KeyModifiers::RightAlt) {
                modifiers.push("RightAlt");
            }
            write!(f, "{}", modifiers.join("+"))
        }
    }
}

impl KeyModifiers {
    /// Creates a new KeyModifiers enum from a bitfield
    pub fn from_bits(bits: u16) -> KeyModifiers {
        unsafe { core::mem::transmute(bits) }
    }

    /// Converts the KeyModifiers enum to a bitfield
    pub fn to_bits(&self) -> u16 {
        *self as u16
    }

    /// Checks if no modifiers are pressed
    pub fn is_empty(&self) -> bool {
        *self == KeyModifiers::None
    }

    /// Checks if all modifiers are pressed
    pub fn contains_all(&self, other: KeyModifiers) -> bool {
        self.to_bits() & other.to_bits() == other.to_bits()
    }

    /// Checks if any modifier is pressed
    pub fn contains_any(&self, other: KeyModifiers) -> bool {
        self.to_bits() & other.to_bits() != 0
    }

    /// Checks if any Shift is pressed
    pub fn has_shift(&self) -> bool {
        self.contains_any(KeyModifiers::LeftShift + KeyModifiers::RightShift)
    }

    /// Checks if any Control is pressed
    pub fn has_control(&self) -> bool {
        self.contains_any(KeyModifiers::LeftControl + KeyModifiers::RightControl)
    }

    /// Checks if any Alt is pressed
    pub fn has_alt(&self) -> bool {
        self.contains_any(KeyModifiers::LeftAlt + KeyModifiers::RightAlt)
    }
}

impl Add<KeyModifiers> for KeyModifiers {
    type Output = KeyModifiers;

    // stfu clippy
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn add(self, rhs: KeyModifiers) -> Self::Output {
        KeyModifiers::from_bits(self.to_bits() | rhs.to_bits())
    }
}

impl AddAssign<KeyModifiers> for KeyModifiers {
    fn add_assign(&mut self, rhs: KeyModifiers) {
        *self = *self + rhs;
    }
}

impl Sub<KeyModifiers> for KeyModifiers {
    type Output = KeyModifiers;

    fn sub(self, rhs: KeyModifiers) -> Self::Output {
        KeyModifiers::from_bits(self.to_bits() & !rhs.to_bits())
    }
}

impl SubAssign<KeyModifiers> for KeyModifiers {
    fn sub_assign(&mut self, rhs: KeyModifiers) {
        *self = *self - rhs;
    }
}

/// Represents a key on the Multimedia section of the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiKey {
    Power,
    Sleep,
    Wake,
}

/// Represents a key on the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            Key::LeftControl => KeyModifiers::LeftControl,
            Key::RightControl => KeyModifiers::RightControl,
            Key::LeftShift => KeyModifiers::LeftShift,
            Key::RightShift => KeyModifiers::RightShift,
            Key::LeftAlt => KeyModifiers::LeftAlt,
            Key::RightAlt => KeyModifiers::RightAlt,
            _ => KeyModifiers::None,
        }
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

/// Handles a keyboard event from the keyboard driver
pub fn handle_keyboard_event(event: KeyboardEvent) {
    println!("{:?}", event);
}
