use crate::interrupts::idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters};

static mut UPTIME: u64 = 0;

pub fn handler(
    _ist: u64,
    _rsp: u64,
    _ifr: &mut InterruptFrameRegisters,
    _ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
) {
    unsafe {
        UPTIME += 1;
    }
}

pub fn get_uptime_ticks() -> u64 {
    unsafe { UPTIME }
}
