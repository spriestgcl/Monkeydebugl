#![no_std]
#![allow(dead_code, unused_assignments, unused_mut)]
#![feature(used_with_arg)]

extern crate alloc;

mod ahci_core;
mod ahci_types;
mod platform;

use crate::ahci_core::*;
use crate::ahci_types::*;
use log::warn;
use devices::driver_define;
use alloc::sync::Arc;
use core::ptr::{addr_of, addr_of_mut};

// 移除panic_handler，因为kernel已经有了
// #[panic_handler]
// fn panic(_info: &PanicInfo) -> ! {
//     loop {}
// }

// 全局AHCI设备实例
static mut GLOBAL_AHCI_DEVICE: ahci_device = ahci_device {
    mmio_base: 0,
    cap: 0,
    cap2: 0,
    version: 0,
    port_map: 0,
    port_map_linkup: 0,
    n_ports: 0,
    port_idx: 0,
    port: [ahci_ioport {
        port_mmio: 0,
        cmd_slot: core::ptr::null_mut(),
        cmd_slot_dma: 0,
        rx_fis_dma: 0,
        cmd_tbl_dma: 0,
        cmd_tbl: 0,
    }; 32],
    blk_dev: ahci_blk_dev {
        lba48: false,
        lba: 0,
        blksz: 512,
        lba_offset: 0,
    },
};

/// AHCI驱动结构体
pub struct AhciDriver;

impl AhciDriver {
    pub fn new() -> Self {
        Self
    }
}

impl devices::device::Driver for AhciDriver {
    fn get_id(&self) -> &str {
        "kahci"
    }

    fn get_device_wrapper(self: Arc<Self>) -> devices::device::DeviceType {
        devices::device::DeviceType::BLOCK(Arc::new(BlockDeviceWrapper(self)))
    }
}

struct BlockDeviceWrapper(Arc<AhciDriver>);

impl devices::device::Driver for BlockDeviceWrapper {
    fn get_id(&self) -> &str {
        "kahci-block"
    }

    fn get_device_wrapper(self: Arc<Self>) -> devices::device::DeviceType {
        devices::device::DeviceType::None
    }
}

impl devices::device::BlkDriver for BlockDeviceWrapper {
    fn read_blocks(&self, block_id: usize, buf: &mut [u8]) {
        unsafe {
            let blknr = block_id as u64;
            let blkcnt = (buf.len() / 512) as u32;
            if blkcnt == 0 { return; }
            let _ = ahci_sata_read_common(
                &*addr_of!(GLOBAL_AHCI_DEVICE),
                blknr,
                blkcnt,
                buf.as_mut_ptr(),
            );
        }
    }

    fn write_blocks(&self, block_id: usize, buf: &[u8]) {
        unsafe {
            let blknr = block_id as u64;
            let blkcnt = (buf.len() / 512) as u32;
            if blkcnt == 0 { return; }
            let _ = ahci_sata_write_common(
                &*addr_of!(GLOBAL_AHCI_DEVICE),
                blknr,
                blkcnt,
                buf.as_ptr() as *mut u8,
            );
        }
    }

    fn capacity(&self) -> usize {
        unsafe {
            let lba_count = (*addr_of!(GLOBAL_AHCI_DEVICE)).blk_dev.lba as usize;
            let block_size = (*addr_of!(GLOBAL_AHCI_DEVICE)).blk_dev.blksz as usize;
            lba_count * block_size
        }
    }
}

// 使用driver_define!宏注册驱动
driver_define!({
    // 初始化 AHCI 控制器，只有成功且容量非零时才注册驱动
    let ok = unsafe {
        let ret = ahci_init(&mut *addr_of_mut!(GLOBAL_AHCI_DEVICE));
        if ret != 0 {
            warn!("AHCI: initialization failed, skip registering driver");
            false
        } else if (*addr_of!(GLOBAL_AHCI_DEVICE)).blk_dev.lba == 0 {
            warn!("AHCI: device capacity is zero, skip registering driver");
            false
        } else {
            true
        }
    };

    if ok { Some(Arc::new(AhciDriver::new())) } else { None }
});
