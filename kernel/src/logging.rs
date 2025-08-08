use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};
use devices::utils::puts;

static LOGGER_BUSY: AtomicBool = AtomicBool::new(false);

pub struct Logger;

impl Write for Logger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // 简单的输出同步 - 等待之前的输出完成
        while LOGGER_BUSY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        LOGGER_BUSY.store(true, Ordering::Release);
        
        // 改进的直接UART输出，添加状态检查
        unsafe {
            let uart_base = 0x800000001fe20000 as *mut u8;
            let lsr_offset = 5; // Line Status Register offset
            let lsr_addr = uart_base.add(lsr_offset);
            
            for byte in s.bytes() {
                // 等待UART发送缓冲区空闲
                while (lsr_addr.read_volatile() & 0x20) == 0 {
                    // 等待发送缓冲区空闲 (THRE bit)
                }
                
                uart_base.write_volatile(byte);
                
                // 添加小延迟确保字符传输完成
                for _ in 0..50 {
                    core::hint::spin_loop();
                }
            }
        }
        
        LOGGER_BUSY.store(false, Ordering::Release);
        Ok(())
    }
}

// 添加直接UART输出的宏（带同步）
#[macro_export]
macro_rules! direct_print {
    ($($arg:tt)*) => ({
        let msg = format!($($arg)*);
        // 等待Logger空闲
        while $crate::logging::LOGGER_BUSY.load(core::sync::atomic::Ordering::Acquire) {
            core::hint::spin_loop();
        }
        $crate::logging::LOGGER_BUSY.store(true, core::sync::atomic::Ordering::Release);
        
        unsafe {
            let uart_base = 0x800000001fe20000 as *mut u8;
            let lsr_offset = 5;
            let lsr_addr = uart_base.add(lsr_offset);
            
            for byte in msg.bytes() {
                while (lsr_addr.read_volatile() & 0x20) == 0 {}
                uart_base.write_volatile(byte);
                for _ in 0..50 {
                    core::hint::spin_loop();
                }
            }
        }
        
        $crate::logging::LOGGER_BUSY.store(false, core::sync::atomic::Ordering::Release);
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::logging::print(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! println {
    () => ($crate::logging::print(format_args!("\r\n")));
    ($($arg:tt)*) => ({
        $crate::logging::print(format_args!($($arg)*));
        $crate::logging::print(format_args!("\r\n"));
    })
}

#[inline]
pub fn print(args: fmt::Arguments) {
    Logger
        .write_fmt(args)
        .expect("can't write string in logging module.");
}
