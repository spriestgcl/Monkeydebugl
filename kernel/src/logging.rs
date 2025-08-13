use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};
use devices::utils::puts;
use log::{Level, LevelFilter, Log, Metadata, Record};

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

// -------- Integrate with log crate (fallback logger) --------

struct KernelLog;

impl Log for KernelLog {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mp = record.module_path().unwrap_or("");
        let ln = record.line().unwrap_or(0);
        // 使用 main.rs 中同样的 UART 输出实现，避免任何差异
        let prefix = match record.level() {
            Level::Error => "[ERROR]",
            Level::Warn => "[WARN ]",
            Level::Info => "[INFO ]",
            Level::Debug => "[DEBUG]",
            Level::Trace => "[TRACE]",
        };
        // 直接使用本地 println 宏，底层已指向 0x800000001fe20000 UART
        println!("{} <{}:{}> {}", prefix, mp, ln, record.args());
    }

    fn flush(&self) {}
}

static KERNEL_LOGGER: KernelLog = KernelLog;

/// Initialize a simple UART-backed logger as a fallback.
/// Safe to call even if another logger is already set.
pub fn init_logger() {
    // Compute desired level from compile-time env
    let level = match option_env!("LOG") {
        Some("error") => LevelFilter::Error,
        Some("warn") => LevelFilter::Warn,
        Some("info") => LevelFilter::Info,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };

    // Try to set our logger; ignore error if a logger was already set.
    let _ = log::set_logger(&KERNEL_LOGGER);
    log::set_max_level(level);
}
