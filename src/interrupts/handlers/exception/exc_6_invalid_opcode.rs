use crate::{
    interrupts::idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    println,
};

pub fn handler(
    _interrupt_num: u64,
    rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    ife: Option<&mut InterruptFrameExtra>,
) {
    println!("Invalid opcode exception.");

    println!("rsp = {:#016x}", rsp);
    println!("{:#?}", ifr);
    println!("{:#?}", ifc);
    println!("{:#?}", ife);

    panic!("Invalid opcode exception dump complete.");
}
