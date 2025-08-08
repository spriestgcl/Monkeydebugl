#![no_std]
#![feature(used_with_arg)]

extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use devices::{
    device::{DeviceType, Driver, UartDriver},
    driver_define,
    fdt::Node,
    node_to_interrupts, register_device_irqs, VIRT_ADDR_START,
};
use log::info;
use ns16550a::{
    Break, DMAMode, Divisor, ParityBit, ParitySelect, StickParity, StopBits, Uart, WordLength,
};

pub struct NS16550a {
    _base: usize,
    inner: Uart,
    irqs: Vec<u32>,
}

impl Driver for NS16550a {
    fn get_id(&self) -> &str {
        "ns16550a"
    }

    fn get_device_wrapper(self: Arc<Self>) -> DeviceType {
        DeviceType::UART(self.clone())
    }

    fn try_handle_interrupt(&self, _irq: u32) -> bool {
        info!("handle uart interrupt");
        false
    }

    fn interrupts(&self) -> &[u32] {
        &self.irqs
    }
}

impl UartDriver for NS16550a {
    fn put(&self, c: u8) {
        self.inner.put(c)
    }

    fn get(&self) -> Option<u8> {
        self.inner.get()
    }
}

fn init_driver(node: &Node) -> Arc<dyn Driver> {
    let addr = node.reg().unwrap().next().unwrap().address as usize;

    info!(
        "node interrupts: {:?}",
        node.interrupts().unwrap().flatten().collect::<Vec<u32>>()
    );
    
    // 针对2k1000开发板的地址映射修复
    let uart_addr = if addr == 0x1fe20000 {
        // 2k1000开发板的UART地址是直接映射的
        0x800000001fe20000
    } else {
        // 其他情况使用原有的虚拟地址映射
        VIRT_ADDR_START + addr
    };
    
    // 添加调试输出，确认地址映射
    unsafe {
        let debug_uart_base = 0x800000001fe20000 as *mut u8;
        debug_uart_base.write_volatile(b'A'); // Address mapping
        // 输出计算出的地址的一些位
        debug_uart_base.write_volatile(b'0' + ((uart_addr >> 28) & 0xF) as u8);
        debug_uart_base.write_volatile(b'0' + ((uart_addr >> 24) & 0xF) as u8);
    }
    
    let uart = Arc::new(NS16550a {
        _base: uart_addr,
        inner: Uart::new(uart_addr),
        irqs: node_to_interrupts(node),
    });
    register_device_irqs(uart.clone());
    uart.inner.init(
        WordLength::EIGHT,
        StopBits::ONE,
        ParityBit::DISABLE,
        ParitySelect::EVEN,
        StickParity::DISABLE,
        Break::DISABLE,
        DMAMode::MODE0,
        Divisor::BAUD115200,
    );
    
    // 初始化完成后的测试
    unsafe {
        let debug_uart_base = 0x800000001fe20000 as *mut u8;
        debug_uart_base.write_volatile(b'I'); // Initialized
    }
    
    uart
}

driver_define!("ns16550a", init_driver);
