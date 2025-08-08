#![no_std]
#![feature(used_with_arg)]

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate log;

use alloc::sync::Arc;
use alloc::vec::Vec;
use devices::device::{BlkDriver, DeviceType, Driver};
use devices::{driver_define, Mutex};

mod ahci;
mod pci;
mod platform;
mod ahci_types;
mod ahci_core;

pub use ahci::AhciController;
pub use pci::PciConfig;
pub use ahci_types::*;
pub use ahci_core::*;

// 全局 AHCI 控制器存储
static AHCI_CONTROLLERS: Mutex<Vec<Arc<AhciController>>> = Mutex::new(Vec::new());

// 全局 AHCI 设备结构（参考驱动风格）
static mut GLOBAL_AHCI_DEVICE: ahci_device = ahci_device {
    mmio_base: 0,
    flags: 0,
    cap: 0,
    cap2: 0,
    version: 0,
    port_map: 0,
    pio_mask: 0,
    udma_mask: 0,
    n_ports: 0,
    port_map_linkup: 0,
    port: [ahci_ioport {
        port_mmio: 0,
        cmd_slot: core::ptr::null_mut(),
        cmd_slot_dma: 0,
        rx_fis: 0,
        rx_fis_dma: 0,
        cmd_tbl: 0,
        cmd_tbl_dma: 0,
        cmd_tbl_sg: core::ptr::null_mut(),
    }; 32],
    port_idx: 0,
    blk_dev: ahci_blk_dev {
        lba48: true,
        lba: 0,
        blksz: 512,
        queue_depth: 1,
        product: [0; 41],
        serial: [0; 21],
        revision: [0; 9],
    },
};

/// AHCI 驱动实现
pub struct AhciDriver {
    controller: Arc<AhciController>,
    port_id: u8,
    irqs: Vec<u32>,
}

impl Driver for AhciDriver {
    fn get_id(&self) -> &str {
        "ahci"
    }

    fn interrupts(&self) -> &[u32] {
        &self.irqs
    }

    fn get_device_wrapper(self: Arc<Self>) -> DeviceType {
        DeviceType::BLOCK(self.clone())
    }

    fn try_handle_interrupt(&self, _irq: u32) -> bool {
        // 处理 AHCI 中断
        self.controller.handle_interrupt(self.port_id)
    }
}

impl BlkDriver for AhciDriver {
    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) {
        unsafe {
            let blocks_to_read = (buf.len() + 511) / 512; // 向上取整到块数
            let result = ahci_sata_read_common(
                &GLOBAL_AHCI_DEVICE,
                block_id as u64,
                blocks_to_read as u32,
                buf.as_mut_ptr(),
            );
            
            if result == 0 {
                warn!("AHCI read returned 0, operation may have failed");
            }
        }
    }

    fn write_blocks(&self, block_id: usize, buf: &[u8]) {
        unsafe {
            let blocks_to_write = (buf.len() + 511) / 512; // 向上取整到块数
            let result = ahci_sata_write_common(
                &GLOBAL_AHCI_DEVICE,
                block_id as u64,
                blocks_to_write as u32,
                buf.as_ptr() as *mut u8,
            );
            
            if result == 0 {
                warn!("AHCI write returned 0, operation may have failed");
            }
        }
    }

    fn capacity(&self) -> usize {
        unsafe {
            GLOBAL_AHCI_DEVICE.blk_dev.lba as usize
        }
    }
}

// 注册驱动 - 使用参考驱动的初始化方式
driver_define!({
    info!("AHCI driver: Initializing with reference driver style");
    
    // 使用参考驱动的初始化函数
    unsafe {
        let init_result = ahci_init(&mut GLOBAL_AHCI_DEVICE);
        if init_result != 0 {
            error!("AHCI driver: ahci_init failed with code {}", init_result);
            return None;
        }
    }
    
    // 检查是否有设备连接
    unsafe {
        if GLOBAL_AHCI_DEVICE.port_map_linkup & 1 == 0 {
            warn!("AHCI driver: No device connected to port 0");
            return None;
        }
    }
    
    // 创建现代化的 AHCI 控制器包装器（为了兼容现有接口）
    let ahci_virt_addr = unsafe { GLOBAL_AHCI_DEVICE.mmio_base };
    match AhciController::new(ahci_virt_addr as *mut u8) {
        Ok(controller) => {
            let controller = Arc::new(controller);
            let port_id = 0; // 根据资料，板卡上固定只开启最低位的一个端口
            
            let driver = Arc::new(AhciDriver {
                controller: controller.clone(),
                port_id,
                irqs: vec![], // TODO: 配置中断号
            });
            
            // 保存控制器引用
            AHCI_CONTROLLERS.lock().push(controller);
            
            info!("AHCI driver: Successfully initialized with reference driver style");
            Some(driver)
        }
        Err(e) => {
            error!("AHCI driver: Failed to create controller wrapper: {:?}", e);
            None
        }
    }
});