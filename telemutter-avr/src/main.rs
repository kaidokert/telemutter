#![cfg_attr(target_arch = "avr", no_std)]
#![cfg_attr(target_arch = "avr", no_main)]

#[cfg(target_arch = "avr")]
#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(target_arch = "avr")]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> ! {
    let _ = telemutter_avr::roundtrip_status_sid8();
    loop {}
}

#[cfg(not(target_arch = "avr"))]
fn main() {
    let code = telemutter_avr::roundtrip_status_sid8();
    if code != 0 {
        std::process::exit(code as i32);
    }
}
