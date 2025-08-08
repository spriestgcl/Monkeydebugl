// AHCI 类型定义，参考 ahci/c/ahci_platform.h

/// AHCI 端口结构体
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_ioport {
    pub port_mmio: u64,        // 端口寄存器基地址
    pub cmd_slot: *mut u8,     // 命令列表基地址
    pub cmd_slot_dma: u64,     // 命令列表DMA地址
    pub rx_fis_dma: u64,       // 接收FIS DMA地址
    pub cmd_tbl_dma: u64,      // 命令表DMA地址
    pub cmd_tbl: u64,          // 命令表基地址
}

/// AHCI 块设备结构体
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_blk_dev {
    pub lba48: bool,           // 是否支持LBA48
    pub lba: u64,              // 逻辑块地址数量
    pub blksz: u32,            // 块大小
    pub lba_offset: u64,       // 分区起始LBA偏移（用于MBR/GPT）
}

/// AHCI 设备结构体
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ahci_device {
    pub mmio_base: u64,        // AHCI控制器寄存器基地址
    pub cap: u32,              // HOST_CAP 寄存器
    pub cap2: u32,             // HOST_CAP2 寄存器
    pub version: u32,          // HOST_VERSION 寄存器
    pub port_map: u32,         // HOST_PORTS_IMPL 寄存器
    pub port_map_linkup: u32,  // 端口连接状态
    pub n_ports: u32,          // 端口数量
    pub port_idx: u32,         // 当前使用的端口索引
    pub port: [ahci_ioport; 32], // 端口数组
    pub blk_dev: ahci_blk_dev, // 块设备信息
}

// AHCI 寄存器偏移定义
pub const AHCI_GHC: u64 = 0x04;      // Global Host Control
pub const AHCI_CAP: u64 = 0x00;      // Host Capabilities
pub const AHCI_CAP2: u64 = 0x24;     // Host Capabilities Extended
pub const AHCI_PI: u64 = 0x0C;       // Ports Implemented
pub const AHCI_VS: u64 = 0x10;       // AHCI Version

// 端口寄存器偏移
pub const PORT_SSTS: u64 = 0x28;     // Port Serial ATA Status
pub const PORT_CMD: u64 = 0x18;      // Port Command
pub const PORT_CLB: u64 = 0x00;      // Port Command List Base
pub const PORT_CLBU: u64 = 0x04;     // Port Command List Base Upper
pub const PORT_FB: u64 = 0x08;       // Port FIS Base
pub const PORT_FBU: u64 = 0x0C;      // Port FIS Base Upper
pub const PORT_IS: u64 = 0x10;       // Port Interrupt Status
pub const PORT_IE: u64 = 0x14;       // Port Interrupt Enable
pub const PORT_CMD_ISSUE: u64 = 0x38; // Port Command Issue
pub const PORT_TFD: u64 = 0x20;      // Port Task File Data

// AHCI 命令和状态位
pub const HOST_AHCI_EN: u32 = 1 << 31;  // AHCI Enable
pub const HOST_IRQ_EN: u32 = 1 << 1;    // Interrupt Enable
pub const PORT_CMD_ST: u32 = 1 << 0;    // Start
pub const PORT_CMD_FRE: u32 = 1 << 4;   // FIS Receive Enable
pub const PORT_CMD_FR: u32 = 1 << 14;   // FIS Receive Running
pub const PORT_CMD_CR: u32 = 1 << 15;   // Command Running

// SATA FIS 类型
pub const SATA_FIS_TYPE_REGISTER_H2D: u8 = 0x27; // Register FIS - Host to Device

// ATA 命令
pub const ATA_CMD_IDENTIFY_DEVICE: u8 = 0xEC;
pub const ATA_CMD_READ_DMA: u8 = 0xC8;
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA: u8 = 0xCA;
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
pub const ATA_CMD_FLUSH_CACHE: u8 = 0xE7;
pub const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA;

// ATA 设备/头部寄存器位
pub const ATA_LBA: u8 = 0x40;        // LBA 模式

// 设备识别数据偏移
pub const ATA_ID_PIO_MODES: usize = 63;
pub const ATA_ID_UDMA_MODES: usize = 88;
pub const ATA_ID_WORDS: usize = 256;

// 最大传输大小
pub const AHCI_MAX_BYTES_PER_TRANS: u32 = 65536 * 512; // 32MB

// 命令表大小
pub const AHCI_CMD_SZ: u32 = 32;     // 命令列表条目大小
pub const AHCI_CMD_TBL_SZ: u32 = 256; // 命令表大小

// SATA 状态位
pub const SATA_DET_MASK: u32 = 0xF;  // 设备检测掩码
pub const SATA_DET_PRESENT: u32 = 0x3; // 设备存在
pub const SATA_IPM_MASK: u32 = 0xF00; // 接口电源管理掩码
pub const SATA_IPM_ACTIVE: u32 = 0x100; // 活动状态