#![no_std]
#![no_main]

pub mod e9;
pub mod io;

#[no_mangle]
pub extern "C" fn _start() {
    unsafe {
        kmain();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        _handle_panic(info);
        core::arch::asm!("cli", "hlt");
    }
    loop {}
}

#[no_mangle]
unsafe fn _handle_panic(info: &core::panic::PanicInfo) {
    match info.message().as_str() {
        Some(s) => {
            printf!("Panic: {}\r\n", s);
        }
        None => printf!("Panic with no message !\r\n"),
    }
    match info.location() {
        Some(loc) => {
            printf!("Location: {}\r\n", loc);
        }
        None => printf!("Location unknown !\r\n"),
    }
}

unsafe fn kmain() {
    let message = "Welcome to CampiOS !";
    let video_buffer = 0xb8000 as *mut u8;

    for i in 0..(80 * 25) {
        video_buffer.offset(i * 2 + 1).write_volatile(0x0f);
        video_buffer
            .offset(i * 2)
            .write_volatile(match message.chars().nth(i as usize) {
                None => 0,
                Some(c) => c as u8,
            });
    }

    io::outb(0x3d4, 0x0f);
    io::outb(0x3d5, 80);
    io::outb(0x3d4, 0x0e);
    io::outb(0x3d5, 0);

    #[allow(clippy::empty_loop)]
    loop {}
}
