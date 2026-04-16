#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(target_arch = "arm")]
use cortex_m::asm;
#[cfg(target_arch = "arm")]
use cortex_m_rt::entry;
#[cfg(target_arch = "arm")]
use panic_halt as _;

#[cfg(target_arch = "arm")]
#[entry]
fn main() -> ! {
    let _ = telemutter_cortexm::roundtrip_status_sid32();
    loop {
        asm::nop();
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    let code = telemutter_cortexm::roundtrip_status_sid32();
    if code != 0 {
        std::process::exit(code as i32);
    }
}
