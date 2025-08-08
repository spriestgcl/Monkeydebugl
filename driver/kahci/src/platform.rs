use core::arch::asm;

// 平台相关函数实现，参考 ahci/rust/src/platform.rs

/// 等待指定毫秒数
pub fn ahci_mdelay(ms: u32) {
    // TODO: 实现真正的延迟函数，这里暂时用空循环代替
    for _ in 0..(ms * 1000) {
        unsafe {
            core::hint::spin_loop();
        }
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
pub fn ahci_malloc_align(size: u64, align: u32) -> u64 {
    // TODO: 实现真正的对齐内存分配
    // 这里需要与操作系统的内存分配器集成
    use alloc::alloc::{alloc, Layout};
    
    if let Ok(layout) = Layout::from_size_align(size as usize, align as usize) {
        unsafe {
            let ptr = alloc(layout);
            if !ptr.is_null() {
                return ptr as u64;
            }
        }
    }
    0
}

/// 物理地址转换为uncached虚拟地址
/// 根据资料，在LoongArch64上，uncached地址通过设置高位实现
pub fn ahci_phys_to_uncached(pa: u64) -> u64 {
    // LoongArch64 uncached地址映射：物理地址 | 0x8000_0000_0000_0000
    pa | 0x8000_0000_0000_0000u64
}

/// cached虚拟地址转换为物理地址
/// ahci dma可以接受64位的物理地址
pub fn ahci_virt_to_phys(va: u64) -> u64 {
    // 简单的虚拟地址到物理地址转换
    // 去掉uncached标志位
    if va & 0x8000_0000_0000_0000u64 != 0 {
        va & !0x8000_0000_0000_0000u64
    } else {
        // TODO: 实现真正的虚拟地址到物理地址转换
        // 这里需要与操作系统的内存管理器集成
        va
    }
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
pub fn ahci_printf(format: &str, args: core::fmt::Arguments) {
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