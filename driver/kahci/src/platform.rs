use core::arch::asm;
extern crate alloc;

// 平台相关函数实现，参考 ahci/rust/src/platform.rs

// 日志总开关
const KAHCI_PLATFORM_LOG: bool = false;

// 静态对齐缓冲区作为分配回退
#[repr(align(1024))]
struct Align1024([u8; 1024]);
#[repr(align(512))]
struct Align512([u8; 512]);
#[repr(align(256))]
struct Align256([u8; 256]);
#[repr(align(128))]
struct AlignCT([u8; 256 * 32]); // 32 个 256B 的命令表

static mut CL_STATIC: Align1024 = Align1024([0; 1024]);
static mut ID_STATIC: Align512 = Align512([0; 512]);
static mut FIS_STATIC: Align256 = Align256([0; 256]);
static mut CT_STATIC: AlignCT = AlignCT([0; 256 * 32]);
static mut CL_USED: bool = false;
// 512B缓冲允许重复使用，去除ID_USED标志
// static mut ID_USED: bool = false;
static mut FIS_USED: bool = false;
static mut CT_USED: bool = false;

/// 等待指定毫秒数
pub fn ahci_mdelay(ms: u32) {
    // TODO: 实现真正的延迟函数，这里暂时用空循环代替
    for _ in 0..(ms * 1000) {
        core::hint::spin_loop();
    }
}

/// 同步dcache中所有cached和uncached访存请求
pub fn ahci_sync_dcache() {
    unsafe {
        // LoongArch64 的数据缓存屏障指令
        asm!("dbar 0");
    }
}

/// 分配按align字节对齐的内存
pub fn ahci_malloc_align(size: u64, _align: u32) -> u64 {
    // 使用devices crate的内存分配器
    use devices::{frame_alloc, frame_alloc_much};
    
    let pages = (size + 4095) / 4096; // 向上取整到页数
    
    // 添加调试输出
    extern "C" {
        fn debug_uart_string(s: *const u8);
    }
    if KAHCI_PLATFORM_LOG {
        let debug_msg = alloc::format!("[KAHCI] Allocating {} pages for size {} bytes\n\0", pages, size);
        unsafe { debug_uart_string(debug_msg.as_ptr()); }
    }
    
    // 先尝试批量分配
    if let Some(frames) = frame_alloc_much(pages as usize) {
        if let Some(first_frame) = frames.first() {
            let addr = first_frame.raw() as u64;
            if KAHCI_PLATFORM_LOG {
                let debug_msg = alloc::format!("[KAHCI] Allocated memory at 0x{:x} (batch)\n\0", addr);
                unsafe { debug_uart_string(debug_msg.as_ptr()); }
            }
            return addr;
        }
    }
    
    // 如果批量分配失败，尝试单个页面分配
    if pages == 1 {
        if let Some(frame) = frame_alloc() {
            let addr = frame.raw() as u64;
            if KAHCI_PLATFORM_LOG {
                let debug_msg = alloc::format!("[KAHCI] Allocated memory at 0x{:x} (single)\n\0", addr);
                unsafe { debug_uart_string(debug_msg.as_ptr()); }
            }
            return addr;
        }
    }

    // 回退：使用静态对齐缓冲区（仅适用于单端口单实例）
    unsafe {
        if size <= 1024 && !CL_USED {
            CL_USED = true;
            let p = CL_STATIC.0.as_mut_ptr();
            core::ptr::write_bytes(p, 0, 1024);
            let addr = p as usize as u64;
            if KAHCI_PLATFORM_LOG {
                let msg = alloc::format!("[KAHCI] Fallback using static CL buffer @ 0x{:x}\n\0", addr);
                debug_uart_string(msg.as_ptr());
            }
            return addr;
        }
        if size <= 256 && !FIS_USED {
            FIS_USED = true;
            let p = FIS_STATIC.0.as_mut_ptr();
            core::ptr::write_bytes(p, 0, 256);
            let addr = p as usize as u64;
            if KAHCI_PLATFORM_LOG {
                let msg = alloc::format!("[KAHCI] Fallback using static FIS buffer @ 0x{:x}\n\0", addr);
                debug_uart_string(msg.as_ptr());
            }
            return addr;
        }
        if size <= 512 {
            let p = ID_STATIC.0.as_mut_ptr();
            core::ptr::write_bytes(p, 0, 512);
            let addr = p as usize as u64;
            if KAHCI_PLATFORM_LOG {
                let msg = alloc::format!("[KAHCI] Fallback using static 512B buffer @ 0x{:x}\n\0", addr);
                debug_uart_string(msg.as_ptr());
            }
            return addr;
        }
        if size <= (256 * 32) as u64 && !CT_USED {
            CT_USED = true;
            let p = CT_STATIC.0.as_mut_ptr();
            core::ptr::write_bytes(p, 0, 256 * 32);
            let addr = p as usize as u64;
            if KAHCI_PLATFORM_LOG {
                let msg = alloc::format!("[KAHCI] Fallback using static CT buffer @ 0x{:x}\n\0", addr);
                debug_uart_string(msg.as_ptr());
            }
            return addr;
        }
    }
    
    // 分配失败
    if KAHCI_PLATFORM_LOG {
        let debug_msg = alloc::format!("[KAHCI] Memory allocation failed for {} pages\n\0", pages);
        unsafe { debug_uart_string(debug_msg.as_ptr()); }
    }
    0
}

/// 物理地址转换为uncached虚拟地址
/// 根据LoongArch64规范，uncached地址通过设置高位实现
pub fn ahci_phys_to_uncached(pa: u64) -> u64 {
    // LoongArch64 uncached地址映射：物理地址 | 0x8000_0000_0000_0000
    pa | 0x8000_0000_0000_0000u64
}

/// cached虚拟地址转换为物理地址
/// ahci dma可以接受64位的物理地址
pub fn ahci_virt_to_phys(va: u64) -> u64 {
    // 先检查 0x9000...（cached 映射），再检查 0x8000...（uncached 映射）
    if va >= 0x9000_0000_0000_0000u64 {
        return va - 0x9000_0000_0000_0000u64;
    }
    if va >= 0x8000_0000_0000_0000u64 {
        return va - 0x8000_0000_0000_0000u64;
    }
    va
}

/// 内存设置函数
pub fn ahci_memset(ptr: *mut u8, value: u8, count: u64) {
    unsafe {
        core::ptr::write_bytes(ptr, value, count as usize);
    }
}

/// 内存拷贝函数
pub fn ahci_memcpy(dest: *mut u8, src: *const u8, count: u64) {
    unsafe {
        core::ptr::copy_nonoverlapping(src, dest, count as usize);
    }
}

/// printf 函数实现 - 使用log crate替代
pub fn ahci_printf(_format: &str, args: core::fmt::Arguments) {
    // 使用 log crate 的 info! 宏来替代 printf
    log::info!("{}", args);
}

// 为了兼容C风格的printf调用，提供一个简化版本
#[no_mangle]
pub extern "C" fn printf_simple(msg: *const u8) -> i32 {
    if !msg.is_null() {
        let mut len = 0;
        unsafe {
            while *msg.add(len) != 0 {
                len += 1;
            }
            if let Ok(s) = core::str::from_utf8(core::slice::from_raw_parts(msg, len)) {
                log::info!("{}", s);
            }
        }
    }
    0
}