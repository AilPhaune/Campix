static mut UPTIME: u64 = 0;

pub fn handler(_ist: u64, _rsp: u64) {
    unsafe {
        UPTIME += 1;
    }
}

pub fn get_uptime_ticks() -> u64 {
    unsafe { UPTIME }
}
