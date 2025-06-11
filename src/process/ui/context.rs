use alloc::collections::VecDeque;

use crate::process::ui::events::UiEvent;

#[derive(Debug)]
pub struct WindowContext {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Default, Debug)]
pub struct TerminalContext {}

#[derive(Default, Debug)]
pub struct UiContext {
    pub pid: u32,
    pub tid: u32,

    pub graphical_window: Option<WindowContext>,
    pub terminal_context: TerminalContext,

    pub events: VecDeque<UiEvent>,
}

impl UiContext {
    pub fn pid_tid(pid: u32, tid: u32) -> Self {
        Self {
            pid,
            tid,
            ..Default::default()
        }
    }
}
