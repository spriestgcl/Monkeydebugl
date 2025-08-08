use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, Ordering},
};

use polyhal::{
    common::get_cpu_num,
    ctor::{ph_init_iter, CtorType},
    println,
};

// Define multi-architecture modules and pub use them.
cfg_if::cfg_if! {
    if #[cfg(target_arch = "loongarch64")] {
        mod loongarch64;
    } else if #[cfg(target_arch = "aarch64")] {
        mod aarch64;
    } else if #[cfg(target_arch = "riscv64")] {
        mod riscv64;
    } else if #[cfg(target_arch = "x86_64")] {
        mod x86_64;
    } else {
        compile_error!("unsupported architecture!");
    }
}

/// Clear the bss section
pub(crate) fn clear_bss() {
    extern "C" {
        fn _sbss();
        fn _ebss();
    }
    unsafe {
        core::slice::from_raw_parts_mut(
            _sbss as usize as *mut u128,
            (_ebss as usize - _sbss as usize) / size_of::<u128>(),
        )
        .fill(0);
    }
}

fn call_real_main(hartid: usize) {
    // 添加调试输出函数
    unsafe fn debug_uart_char(c: u8) {
        let uart_base = 0x800000001fe20000 as *mut u8;
        uart_base.write_volatile(c);
    }

    static IS_BOOT: AtomicBool = AtomicBool::new(true);
    static INIT_DONE: AtomicBool = AtomicBool::new(false);

    // 调试输出: 进入call_real_main
    unsafe {
        debug_uart_char(b'X');
    } // 'X' - eXecute call_real_main

    extern "Rust" {
        fn _secondary_start();
        pub(crate) fn _main_for_arch(hartid: usize);
        pub(crate) fn _secondary_for_arch(hartid: usize);
    }

    if IS_BOOT.swap(false, Ordering::SeqCst) {
        // 调试输出: 主核启动
        unsafe {
            debug_uart_char(b'A');
        } // 'A' - mAin core boot

        const SP_SIZE: usize = 0x40_0000;

        // (0..get_cpu_num()).for_each(|x| unsafe {
        //     if x == hartid {
        //         return;
        //     }
        //     // 调试输出: 启动其他核心
        //     unsafe {
        //         debug_uart_char(b'a');
        //     } // 'a' - boot Another core

        //     let stack_top = polyhal::mem::alloc(SP_SIZE).add(SP_SIZE);
        //     println!("Boot Core: {}   {:#p}", x, stack_top);
        //     polyhal::multicore::boot_core(x, _secondary_start as usize, stack_top as usize);
        // });

        // 调试输出: 多核启动完成
        unsafe {
            debug_uart_char(b'B');
        } // 'B' - Boot cores done

        polyhal::println!();

        // Run Kernel's Contructors Before Droping Into Kernel.
        ph_init_iter(CtorType::KernelService).for_each(|x| {
            unsafe {
                debug_uart_char(b'k');
            } // 'k' - kernel service constructor
            (x.func)();
        });

        // 调试输出: 内核服务构造函数完成
        unsafe {
            debug_uart_char(b'K');
        } // 'K' - Kernel service done

        ph_init_iter(CtorType::Normal).for_each(|x| {
            unsafe {
                debug_uart_char(b'n');
            } // 'n' - normal constructor
            (x.func)();
        });

        // 调试输出: 普通构造函数完成
        unsafe {
            debug_uart_char(b'N');
        } // 'N' - Normal constructors done

        INIT_DONE.store(true, Ordering::SeqCst);

        // 调试输出: 准备调用主函数
        unsafe {
            debug_uart_char(b'G');
        } // 'G' - Go to main function

        // Declare the _main_for_arch exists.
        unsafe {
            _main_for_arch(hartid);
        }

        // 调试输出: 主函数返回（不应该发生）
        unsafe {
            debug_uart_char(b'Z');
        } // 'Z' - main function returned
    } else {
        // 调试输出: 从核等待
        unsafe {
            debug_uart_char(b'W');
        } // 'W' - Wait for init

        while !INIT_DONE.load(Ordering::SeqCst) {
            spin_loop();
        }
        unsafe {
            _secondary_for_arch(hartid);
        }
    }
    loop {
        spin_loop();
    }
}
