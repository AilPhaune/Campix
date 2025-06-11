use crate::drivers::keyboard::KeyboardEvent;

#[derive(Debug)]
pub enum UiEvent {
    KeyboardEvent(KeyboardEvent),
}
