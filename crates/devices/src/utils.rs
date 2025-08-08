use polyhal::{debug_console::DebugConsole, PageTable};

use crate::MAIN_UART;

/// Translate virtual address into physical address in the current virtual address space
///
#[inline]
pub fn virt_to_phys(vaddr: usize) -> Option<usize> {
    PageTable::current()
        .translate(vaddr.into())
        .map(|x| x.0.raw())
}

pub fn puts(buffer: &[u8]) {
    // Use the main uart as much as possible.
    let main_uart_inited = MAIN_UART.is_init();
    for i in buffer {
        match main_uart_inited {
            true => MAIN_UART.put(*i),
            false => {
                // 如果DebugConsole也有问题，使用直接UART输出作为最后的回退
                DebugConsole::putchar(*i);
                
                // 备用方案：直接UART输出（如果DebugConsole失败）
                // 在2k1000上，如果上面的方法仍然有问题，可以启用下面的代码
                /*
                unsafe {
                    let uart_base = 0x800000001fe20000 as *mut u8;
                    uart_base.write_volatile(*i);
                }
                */
            }
        }
    }
}

/// Get a character from the uart.
///
/// If the uart device was initialized, then use it.
pub fn get_char() -> Option<u8> {
    match MAIN_UART.try_get() {
        Some(uart) => uart.get(),
        None => DebugConsole::getchar(),
    }
}
