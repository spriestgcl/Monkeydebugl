use loongArch64::register::euen;
use polyhal::{
    ctor::{ph_init_iter, CtorType},
    hart_id,
    mem::{init_dtb_once, parse_system_info},
    PhysAddr,
};

#[no_mangle]
pub static mut BOOT_DTB_PTR: usize = 0;

macro_rules! init_dwm {
    () => {
        "
        ori         $t0, $zero, 0x1     # CSR_DMW1_PLV0
        lu52i.d     $t0, $t0, -2048     # UC, PLV0, 0x8000 xxxx xxxx xxxx
        csrwr       $t0, 0x180          # LOONGARCH_CSR_DMWIN0
        ori         $t0, $zero, 0x11    # CSR_DMW1_MAT | CSR_DMW1_PLV0
        lu52i.d     $t0, $t0, -1792     # CA, PLV0, 0x9000 xxxx xxxx xxxx
        csrwr       $t0, 0x181          # LOONGARCH_CSR_DMWIN1
        "
    };
}

/// The earliest entry point for the primary CPU.
///
/// We can't use bl to jump to higher address, so we use jirl to jump to higher address.
#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start() -> ! {
    core::arch::asm!(
        init_dwm!(),
        "# Enable PG
        li.w        $t0, 0xb0       # PLV=0, IE=0, PG=1
        csrwr       $t0, 0x0        # LOONGARCH_CSR_CRMD
        li.w        $t0, 0x00       # PLV=0, PIE=0, PWE=0
        csrwr       $t0, 0x1        # LOONGARCH_CSR_PRMD
        li.w        $t0, 0x00       # FPE=0, SXE=0, ASXE=0, BTE=0
        csrwr       $t0, 0x2        # LOONGARCH_CSR_EUEN

        la.global   $sp, bstack_top
        # If a1 (dtb) is zero, copy a0 into a1 to tolerate bootloaders that pass dtb via a0
        bne         $a1, $zero, 1f
        or          $a1, $a0, $zero
1:
        csrrd       $a0, 0x20           # cpuid (a0)
        # Save a1 (dtb pointer) into a global so we can recover if caller didn't pass it
        la.global   $t1, {boot_dtb}
        st.d        $a1, $t1, 0
        la.global   $t0, {entry}
        jirl        $zero,$t0,0
        ",
        entry = sym rust_tmp_main,
        boot_dtb = sym BOOT_DTB_PTR,
        options(noreturn),
    )
}

/// The earliest entry point for the primary CPU.
///
/// We can't use bl to jump to higher address, so we use jirl to jump to higher address.
#[naked]
#[no_mangle]
unsafe extern "C" fn _secondary_start() -> ! {
    core::arch::asm!(
        init_dwm!(),
        "# Load Stack Pointer From Message Buffer
        li.w         $t0, {MBUF1}
        iocsrrd.d    $sp, $t0

        csrrd        $a0, 0x20                  # cpuid
        la.global    $t0, {entry}

        jirl         $zero, $t0, 0
        ",
        options(noreturn),
        MBUF1 = const loongArch64::consts::LOONGARCH_CSR_MAIL_BUF1,
        entry = sym _rust_secondary_main,
    )
}

/// Rust temporary entry point
///
/// This function will be called after assembly boot stage.
// U-Boot bootm 传递: a0=hart_id, a1=dtb_ptr
pub extern "C" fn rust_tmp_main(hart_id: usize, dt_raw: usize) {
    unsafe fn debug_uart_char(c: u8) {
        let uart_base = 0x800000001fe20000 as *mut u8;
        uart_base.write_volatile(c);
    }
    unsafe fn debug_uart_hex(val: usize) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for i in (0..16).rev() {
            let nibble = ((val >> (i * 4)) & 0xf) as u8;
            debug_uart_char(HEX[nibble as usize]);
        }
    }
    unsafe {
        debug_uart_char(b'h');
    }

    super::clear_bss();
    unsafe {
        debug_uart_char(b'k');
    }
    unsafe {
        debug_uart_char(b'G');
    }
    // 打印传入的 DT 指针
    unsafe {
        debug_uart_char(b'D');
        debug_uart_char(b'T');
        debug_uart_char(b':');
        debug_uart_hex(dt_raw);
    }
    // 若 a1 未传（为 0），回退到启动时保存的全局 BOOT_DTB_PTR
    let dt_final = if dt_raw != 0 { dt_raw } else { unsafe { BOOT_DTB_PTR } };
    unsafe {
        debug_uart_char(b'/');
        debug_uart_hex(dt_final);
    }
    let dt = PhysAddr::new(dt_final);
    match init_dtb_once(dt) {
        Ok(_) => unsafe { debug_uart_char(b'i'); },
        Err(_) => unsafe { debug_uart_char(b'E'); },
    }
    unsafe {
        debug_uart_char(b'c');
    }
    // Initialize CPU Configuration.
    init_cpu();
    unsafe {
        debug_uart_char(b'i');
    }
    ph_init_iter(CtorType::Cpu).for_each(|x| (x.func)());
    unsafe {
        debug_uart_char(b's');
    }

    parse_system_info();
    unsafe {
        debug_uart_char(b'h');
    }
    ph_init_iter(CtorType::Platform).for_each(|x| (x.func)());
    unsafe {
        debug_uart_char(b'b');
    }
    ph_init_iter(CtorType::HALDriver).for_each(|x| (x.func)());

    super::call_real_main(hart_id);
}

/// Initialize CPU Configuration.
fn init_cpu() {
    // Enable floating point
    euen::set_fpe(true);

    // Initialzie Timer
    // timer::init_timer();
}

/// The entry point for the second core.
pub(crate) extern "C" fn _rust_secondary_main() {
    // Initialize CPU Configuration.
    init_cpu();

    super::call_real_main(hart_id());
}
