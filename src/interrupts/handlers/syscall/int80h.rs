use crate::{
    e9::write_char,
    interrupts::idt::{InterruptFrameContext, InterruptFrameExtra, InterruptFrameRegisters},
    println,
    process::memory::{get_address_space, VirtualAddressSpace},
};

pub fn handler(
    ist: u64,
    rsp: u64,
    ifr: &mut InterruptFrameRegisters,
    ifc: &mut InterruptFrameContext,
    ife: Option<&mut InterruptFrameExtra>,
) {
    macro_rules! print_info {
        () => {
            println!("Software interrupt 0x80.");
            println!("ist={:#02x}, rsp={:#016x}", ist, rsp);
            println!("{:#?}", ifr);
            println!("{:#?}", ifc);
            println!("{:#?}", ife);
        };
    }

    if ifr.rax == 1 {
        // print function

        let buffer_begin = ifr.rdi;
        let buffer_len = ifr.rsi;

        // verify bounds
        if buffer_len > 1024 * 1024 {
            print_info!();
            println!("Buffer too large.");
            println!("Software interrupt 0x80 dump complete.");
            return;
        }

        let begin_space = get_address_space(buffer_begin);
        let end_space = get_address_space(buffer_begin + buffer_len);
        if !matches!(begin_space, Some(VirtualAddressSpace::LowerHalf(_)))
            || !matches!(end_space, Some(VirtualAddressSpace::LowerHalf(_)))
        {
            print_info!();
            println!("Buffer has parts outside of lower half.");
            println!("Software interrupt 0x80 dump complete.");
            return;
        }

        let mut ptr = buffer_begin as *mut u8;
        for _ in 0..buffer_len {
            unsafe {
                write_char(*ptr);
                ptr = ptr.offset(1);
            }
        }

        return;
    }

    print_info!();
    println!("Software interrupt 0x80 dump complete.");
}
