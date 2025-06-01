use crate::{
    interrupts::{
        self,
        idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
        pic::pic_send_eoi,
    },
    process::scheduler::SCHEDULER,
};

static mut UPTIME: u64 = 0;

pub fn handler(
    _ist: u64,
    _rsp: u64,
    _ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    _ife: Option<&mut InterruptFrameExtra>,
) {
    unsafe {
        UPTIME += 1;

        if ifc.cs & 0b11 != 0 {
            // If interrupted a userland process, switch to another one
            // (don't switch if interrupted a kernel routine, which will decide itself to switch or not)
            interrupts::run_without_interrupts(|| {
                pic_send_eoi(0);
                SCHEDULER.schedule();
            });
        }
    }
}

pub fn get_uptime_ticks() -> u64 {
    unsafe { UPTIME }
}
