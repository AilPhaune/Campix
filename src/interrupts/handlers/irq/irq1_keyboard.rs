use alloc::vec::Vec;

use crate::{
    drivers::keyboard::{
        handle_keyboard_event, AcpiKey, Key, KeyModifiers, KeyboardEvent, KeyboardEventKind,
        KeyboardLayout, MultimediaKey,
    },
    interrupts::idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    io::inb,
};

fn read_keyboard_layout_en_us() -> Option<(Key, KeyboardEventKind)> {
    let scancode = inb(0x60);

    if scancode == 0xE0 {
        let scancode = inb(0x60);
        Some((
            match scancode & !0x80 {
                0x19 => Key::Multimedia(MultimediaKey::NextTrack),
                0x1C => Key::KeypadEnter,
                0x1D => Key::RightControl,
                0x20 => Key::Multimedia(MultimediaKey::Mute),
                0x21 => Key::Multimedia(MultimediaKey::Calculator),
                0x22 => Key::Multimedia(MultimediaKey::Play),
                0x24 => Key::Multimedia(MultimediaKey::Stop),
                0x2E => Key::Multimedia(MultimediaKey::VolumeDown),
                0x30 => Key::Multimedia(MultimediaKey::VolumeUp),
                0x35 => Key::Keypad('/'),
                0x38 => Key::RightAlt,
                0x47 => Key::Home,
                0x48 => Key::CursorUp,
                0x49 => Key::PageUp,
                0x4B => Key::CursorLeft,
                0x4D => Key::CursorRight,
                0x4F => Key::End,
                0x50 => Key::CursorDown,
                0x51 => Key::PageDown,
                0x52 => Key::Insert,
                0x53 => Key::Delete,
                0x5B => Key::LeftGui,
                0x5C => Key::RightGui,
                0x5D => Key::Apps,
                0x5E => Key::Acpi(AcpiKey::Power),
                0x5F => Key::Acpi(AcpiKey::Sleep),
                0x63 => Key::Acpi(AcpiKey::Wake),
                0x65 => Key::Multimedia(MultimediaKey::WWWSearch),
                0x66 => Key::Multimedia(MultimediaKey::WWWFavorites),
                0x67 => Key::Multimedia(MultimediaKey::WWWRefresh),
                0x68 => Key::Multimedia(MultimediaKey::WWWStop),
                0x69 => Key::Multimedia(MultimediaKey::WWWForward),
                0x6A => Key::Multimedia(MultimediaKey::WWWBack),
                0x6B => Key::Multimedia(MultimediaKey::MyComputer),
                0x6C => Key::Multimedia(MultimediaKey::Email),
                0x6D => Key::Multimedia(MultimediaKey::MediaSelect),
                _ => return None,
            },
            if scancode & 0x80 != 0 {
                KeyboardEventKind::KeyUp
            } else {
                KeyboardEventKind::KeyDown
            },
        ))
    } else {
        Some((
            match scancode & !0x80 {
                0x01 => Key::Escape,
                0x02 => Key::Character('1'),
                0x03 => Key::Character('2'),
                0x04 => Key::Character('3'),
                0x05 => Key::Character('4'),
                0x06 => Key::Character('5'),
                0x07 => Key::Character('6'),
                0x08 => Key::Character('7'),
                0x09 => Key::Character('8'),
                0x0A => Key::Character('9'),
                0x0B => Key::Character('0'),
                0x0C => Key::Character('-'),
                0x0D => Key::Character('='),
                0x0E => Key::Backspace,
                0x0F => Key::Tab,
                0x10 => Key::Character('q'),
                0x11 => Key::Character('w'),
                0x12 => Key::Character('e'),
                0x13 => Key::Character('r'),
                0x14 => Key::Character('t'),
                0x15 => Key::Character('y'),
                0x16 => Key::Character('u'),
                0x17 => Key::Character('i'),
                0x18 => Key::Character('o'),
                0x19 => Key::Character('p'),
                0x1A => Key::Character('['),
                0x1B => Key::Character(']'),
                0x1C => Key::Enter,
                0x1D => Key::LeftControl,
                0x1E => Key::Character('a'),
                0x1F => Key::Character('s'),
                0x20 => Key::Character('d'),
                0x21 => Key::Character('f'),
                0x22 => Key::Character('g'),
                0x23 => Key::Character('h'),
                0x24 => Key::Character('j'),
                0x25 => Key::Character('k'),
                0x26 => Key::Character('l'),
                0x27 => Key::Character(';'),
                0x28 => Key::Character('\''),
                0x29 => Key::Character('`'),
                0x2A => Key::LeftShift,
                0x2B => Key::Character('\\'),
                0x2C => Key::Character('z'),
                0x2D => Key::Character('x'),
                0x2E => Key::Character('c'),
                0x2F => Key::Character('v'),
                0x30 => Key::Character('b'),
                0x31 => Key::Character('n'),
                0x32 => Key::Character('m'),
                0x33 => Key::Character(','),
                0x34 => Key::Character('.'),
                0x35 => Key::Character('/'),
                0x36 => Key::RightShift,
                0x37 => Key::Keypad('*'),
                0x38 => Key::LeftAlt,
                0x39 => Key::Space,
                0x3A => Key::CapsLock,
                0x3B => Key::F(1),
                0x3C => Key::F(2),
                0x3D => Key::F(3),
                0x3E => Key::F(4),
                0x3F => Key::F(5),
                0x40 => Key::F(6),
                0x41 => Key::F(7),
                0x42 => Key::F(8),
                0x43 => Key::F(9),
                0x44 => Key::F(10),
                0x45 => Key::NumLock,
                0x46 => Key::ScrollLock,
                0x47 => Key::Keypad('7'),
                0x48 => Key::Keypad('8'),
                0x49 => Key::Keypad('9'),
                0x4A => Key::Keypad('-'),
                0x4B => Key::Keypad('4'),
                0x4C => Key::Keypad('5'),
                0x4D => Key::Keypad('6'),
                0x4E => Key::Keypad('+'),
                0x4F => Key::Keypad('1'),
                0x50 => Key::Keypad('2'),
                0x51 => Key::Keypad('3'),
                0x52 => Key::Keypad('0'),
                0x53 => Key::Keypad('.'),

                0x57 => Key::F(11),
                0x58 => Key::F(12),

                _ => return None,
            },
            if scancode & 0x80 != 0 {
                KeyboardEventKind::KeyUp
            } else {
                KeyboardEventKind::KeyDown
            },
        ))
    }
}

static mut KEYBOARD_LAYOUT: Option<KeyboardLayout> = None;
static mut DOWN_KEYS: Option<Vec<Key>> = None;
static mut MODIFIERS: KeyModifiers = KeyModifiers::empty();

#[allow(static_mut_refs)]
pub fn handler(
    _ist: u64,
    _rsp: u64,
    _ifr: &mut InterruptFrameRegisters,
    _ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
) {
    let layout = unsafe {
        if KEYBOARD_LAYOUT.is_none() {
            KEYBOARD_LAYOUT = Some(KeyboardLayout::default_en_us());
        }
        KEYBOARD_LAYOUT.as_ref().unwrap()
    };
    let key = read_keyboard_layout_en_us();

    let Some(down_keys) = (unsafe {
        if DOWN_KEYS.is_none() {
            DOWN_KEYS = Some(Vec::new());
        }
        &mut DOWN_KEYS
    }) else {
        return;
    };

    if let Some((key, kind)) = key {
        let mut was_down = true;

        // Handle state
        match kind {
            KeyboardEventKind::KeyDown => {
                // Add key to list
                if !down_keys.contains(&key) {
                    was_down = false;
                    down_keys.push(key);
                }

                // Update modifiers
                unsafe {
                    MODIFIERS |= key.modifiers();
                }
            }
            KeyboardEventKind::KeyUp => {
                // Remove key from list
                if let Some(index) = down_keys.iter().position(|k| *k == key) {
                    down_keys.remove(index);
                }

                // Update modifiers
                unsafe {
                    MODIFIERS &= !key.modifiers();
                }
            }
            _ => {}
        }

        let mapped_key = layout.map(key, unsafe { MODIFIERS });

        // Make event
        let event = KeyboardEvent {
            raw_key: key,
            mapped_key,
            kind: if was_down && kind == KeyboardEventKind::KeyDown {
                KeyboardEventKind::KeyRepeat
            } else {
                kind
            },
            modifiers: unsafe { MODIFIERS },
        };

        handle_keyboard_event(event);
    }
}
