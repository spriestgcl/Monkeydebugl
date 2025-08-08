// AHCI 驱动的数据结构定义，参考 ahci/rust/src/libahci.rs

use crate::platform::*;

// ATA相关常量
pub const ATA_ID_PROD_LEN: u32 = 40;
pub const ATA_ID_SERNO_LEN: u32 = 20; 
pub const ATA_ID_FW_REV_LEN: u32 = 8;

// AHCI相关常量
pub const AHCI_MAX_PORTS: u32 = 32;
pub const AHCI_MAX_SG: u32 = 56;
pub const AHCI_MAX_CMDS: u32 = 32;
pub const AHCI_CMD_SZ: u32 = 32;
pub const AHCI_CMD_SLOT_SZ: u32 = AHCI_MAX_CMDS * AHCI_CMD_SZ;
pub const AHCI_RX_FIS_SZ: u32 = 256;
pub const AHCI_CMD_TBL_HDR_SZ: u32 = 128;
pub const AHCI_CMD_TBL_SZ: u32 = AHCI_CMD_TBL_HDR_SZ + (AHCI_MAX_SG * 16);

// SATA标志位
pub const SATA_FLAG_FLUSH_EXT: u32 = 1024;
pub const SATA_FLAG_FLUSH: u32 = 512;
pub const SATA_FLAG_WCACHE: u32 = 256;

// 读写命令类型
pub const READ_CMD: u32 = 0;
pub const WRITE_CMD: u32 = 1;

/// AHCI 命令头结构体
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_cmd_hdr {
    pub opts: u32,
    pub status: u32,
    pub tbl_addr_lo: u32,
    pub tbl_addr_hi: u32,
    pub reserved: [u32; 4],
}

/// AHCI 散列表项
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_sg {
    pub addr_lo: u32,
    pub addr_hi: u32,
    pub reserved: u32,
    pub flags_size: u32,
}

/// AHCI IO端口结构体
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_ioport {
    pub port_mmio: u64,
    pub cmd_slot: *mut ahci_cmd_hdr,
    pub cmd_slot_dma: u64,
    pub rx_fis: u64,
    pub rx_fis_dma: u64,
    pub cmd_tbl: u64,
    pub cmd_tbl_dma: u64,
    pub cmd_tbl_sg: *mut ahci_sg,
}

/// AHCI 块设备结构体 - 代表sata硬盘
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_blk_dev {
    pub lba48: bool,
    pub lba: u64,
    pub blksz: u64,  // 块大小，固定为512
    pub queue_depth: u32,
    pub product: [u8; (ATA_ID_PROD_LEN + 1) as usize],
    pub serial: [u8; (ATA_ID_SERNO_LEN + 1) as usize],
    pub revision: [u8; (ATA_ID_FW_REV_LEN + 1) as usize],
}

/// AHCI 设备结构体 - 代表ahci控制器
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_device {
    pub mmio_base: u64,
    
    pub flags: u32,
    
    pub cap: u32,
    pub cap2: u32,
    pub version: u32,
    pub port_map: u32,
    
    pub pio_mask: u32,
    pub udma_mask: u32,
    
    pub n_ports: u8,  // 端口数量
    pub port_map_linkup: u32,
    pub port: [ahci_ioport; 32],
    pub port_idx: u8,  // 启用的端口索引，根据资料固定为0
    
    pub blk_dev: ahci_blk_dev,  // 由于只有1个端口，直接包含1个块设备
}

impl Default for ahci_blk_dev {
    fn default() -> Self {
        ahci_blk_dev {
            lba48: true,  // 默认支持lba48
            lba: 0,
            blksz: 512,   // 固定块大小512字节
            queue_depth: 1,
            product: [0; (ATA_ID_PROD_LEN + 1) as usize],
            serial: [0; (ATA_ID_SERNO_LEN + 1) as usize],
            revision: [0; (ATA_ID_FW_REV_LEN + 1) as usize],
        }
    }
}

impl Default for ahci_ioport {
    fn default() -> Self {
        ahci_ioport {
            port_mmio: 0,
            cmd_slot: core::ptr::null_mut(),
            cmd_slot_dma: 0,
            rx_fis: 0,
            rx_fis_dma: 0,
            cmd_tbl: 0,
            cmd_tbl_dma: 0,
            cmd_tbl_sg: core::ptr::null_mut(),
        }
    }
}

impl Default for ahci_device {
    fn default() -> Self {
        ahci_device {
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
            port: [ahci_ioport::default(); 32],
            port_idx: 0,  // 根据资料，固定使用端口0
            blk_dev: ahci_blk_dev::default(),
        }
    }
}