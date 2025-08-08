use core::ptr::{read_volatile, write_volatile};

/// PCI 配置空间访问
pub struct PciConfig {
    base: u64,
    bus: u8,
    device: u8,
    function: u8,
}

impl PciConfig {
    pub fn new(config_base: u64, bus: u8, device: u8, function: u8) -> Self {
        Self {
            base: config_base,
            bus,
            device,
            function,
        }
    }
    
    fn config_addr(&self, offset: u8) -> *mut u32 {
        // LoongArch64 PCI配置空间地址计算
        // 尝试不同的地址映射方式
        let addr = if self.bus == 0 {
            // 对于总线0，使用直接的设备地址
            self.base + ((self.device as u64) << 8) + (offset as u64)
        } else {
            // 对于其他总线，使用标准PCI地址格式
            self.base + 
               ((self.bus as u64) << 20) +
               ((self.device as u64) << 15) + 
               ((self.function as u64) << 12) +
               (offset as u64)
        };
        
        // 添加调试输出
        unsafe {
            let uart_base = 0x800000001fe20000 as *mut u8;
            let lsr_offset = 5;
            let lsr_addr = uart_base.add(lsr_offset);
            let msg = format!("PCI: Reading config at {:#x} (bus={}, dev={}, func={}, offset={:#x})\r\n", 
                             addr, self.bus, self.device, self.function, offset);
            for byte in msg.bytes() {
                while (lsr_addr.read_volatile() & 0x20) == 0 {
                    core::hint::spin_loop();
                }
                uart_base.write_volatile(byte);
                for _ in 0..50 {
                    core::hint::spin_loop();
                }
            }
        }
        
        addr as *mut u32
    }
    
    pub fn read32(&self, offset: u8) -> u32 {
        let addr = self.config_addr(offset);
        let value = unsafe { read_volatile(addr) };
        
        // 添加调试输出
        unsafe {
            let uart_base = 0x800000001fe20000 as *mut u8;
            let lsr_offset = 5;
            let lsr_addr = uart_base.add(lsr_offset);
            let msg = format!("PCI: Read {:#x} from {:#x}\r\n", value, addr as usize);
            for byte in msg.bytes() {
                while (lsr_addr.read_volatile() & 0x20) == 0 {
                    core::hint::spin_loop();
                }
                uart_base.write_volatile(byte);
                for _ in 0..50 {
                    core::hint::spin_loop();
                }
            }
        }
        
        value
    }
    
    pub fn write32(&self, offset: u8, value: u32) {
        unsafe { write_volatile(self.config_addr(offset), value) }
    }
    
    pub fn vendor_id(&self) -> u16 {
        (self.read32(0) & 0xFFFF) as u16
    }
    
    pub fn device_id(&self) -> u16 {
        (self.read32(0) >> 16) as u16
    }
    
    pub fn class_code(&self) -> u8 {
        (self.read32(0x8) >> 24) as u8
    }
    
    pub fn subclass(&self) -> u8 {
        (self.read32(0x8) >> 16) as u8
    }
    
    pub fn interface(&self) -> u8 {
        (self.read32(0x8) >> 8) as u8
    }
    
    pub fn bar(&self, index: u8) -> u32 {
        if index > 5 {
            return 0;
        }
        self.read32(0x10 + index * 4)
    }
    
    pub fn set_bar(&self, index: u8, value: u32) {
        if index <= 5 {
            self.write32(0x10 + index * 4, value);
        }
    }
    
    pub fn command(&self) -> u16 {
        (self.read32(0x4) & 0xFFFF) as u16
    }
    
    pub fn set_command(&self, value: u16) {
        let status = self.read32(0x4) & 0xFFFF0000;
        self.write32(0x4, status | (value as u32));
    }
    
    pub fn enable_bus_master(&self) {
        let cmd = self.command();
        self.set_command(cmd | 0x4); // Bus Master Enable
    }
    
    pub fn enable_memory_space(&self) {
        let cmd = self.command();
        self.set_command(cmd | 0x2); // Memory Space Enable  
    }
}