// AHCI 核心实现，参考 ahci/rust/src/drv_ahci.rs
use crate::ahci_types::*;
use crate::platform::*;
use core::ptr::{read_volatile, write_volatile};
use alloc::vec::Vec;

// AHCI寄存器读写函数
fn ahci_readl(addr: u64) -> u32 {
    unsafe { read_volatile(addr as *mut u32) }
}

fn ahci_writel(data: u32, addr: u64) {
    unsafe { write_volatile(addr as *mut u32, data) }
}

// 查找第一个设置位的位置
fn ahci_ffs32(val: u32) -> u32 {
    let mut bit: u32 = 1;
    let mut i: u32 = val;

    if i == 0 {
        return 0;
    }

    while i & 1 == 0 {
        i >>= 1;
        bit += 1;
    }

    bit
}

/// 输出AHCI控制器信息
fn ahci_print_info(ahci_dev: &ahci_device) {
    let vers: u32 = ahci_dev.version;
    let cap: u32 = ahci_dev.cap;
    let _cap2: u32 = ahci_dev.cap2;
    let impl_0: u32 = ahci_dev.port_map;
    let speed: u32 = (ahci_dev.cap >> 20) & 0xf;

    let speed_s = match speed {
        1 => "1.5",
        2 => "3",
        3 => "6",
        _ => "?",
    };

    info!("AHCI vers {:02x}{:02x}.{:02x}{:02x}, {} slots, {} ports, {} Gbps, {:#x} impl, SATA mode",
        vers >> 24 & 0xff,
        vers >> 16 & 0xff,
        vers >> 8 & 0xff,
        vers & 0xff,
        (cap >> 8 & 0x1f) + 1,
        (cap & 0x1f) + 1,
        speed_s,
        impl_0
    );

    // 输出能力标志
    let mut flags = Vec::new();
    if cap & (1 << 31) != 0 { flags.push("64bit"); }
    if cap & (1 << 30) != 0 { flags.push("ncq"); }
    if cap & (1 << 29) != 0 { flags.push("sntf"); }
    if cap & (1 << 28) != 0 { flags.push("ilck"); }
    if cap & (1 << 27) != 0 { flags.push("stag"); }
    if cap & (1 << 26) != 0 { flags.push("pm"); }
    
    if !flags.is_empty() {
        info!("flags: {}", flags.join(" "));
    }
}

/// AHCI初始化函数
/// 按照ahci控制器->端口->sata硬盘的顺序初始化
pub fn ahci_init(ahci_dev: &mut ahci_device) -> i32 {
    // 根据资料，AHCI控制器寄存器物理基地址是0x400e0000
    ahci_dev.mmio_base = ahci_phys_to_uncached(0x400e0000);
    
    info!("AHCI: Initializing controller at physical address {:#x}, virtual address {:#x}", 
          0x400e0000u64, ahci_dev.mmio_base);

    // 1. 初始化AHCI主机控制器
    let ret = ahci_host_init(ahci_dev);
    if ret != 0 {
        error!("AHCI: Host initialization failed");
        return -1;
    }

    // 2. 扫描端口
    let ret = ahci_port_scan(ahci_dev);
    if ret != 0 {
        error!("AHCI: Port scan failed");
        return -1;
    }

    // 3. 打印控制器信息
    ahci_print_info(ahci_dev);

    // 4. 扫描SATA设备
    ahci_sata_scan(ahci_dev);

    info!("AHCI: Initialization completed successfully");
    0
}

/// AHCI主机控制器初始化
fn ahci_host_init(ahci_dev: &mut ahci_device) -> i32 {
    let mmio_base = ahci_dev.mmio_base;
    
    // 读取能力寄存器
    ahci_dev.cap = ahci_readl(mmio_base + 0x00);  // HOST_CAP
    ahci_dev.cap2 = ahci_readl(mmio_base + 0x24); // HOST_CAP2
    ahci_dev.version = ahci_readl(mmio_base + 0x10); // HOST_VERSION
    ahci_dev.port_map = ahci_readl(mmio_base + 0x0C); // HOST_PORTS_IMPL
    
    info!("AHCI: CAP={:#x}, CAP2={:#x}, VERSION={:#x}, PORT_MAP={:#x}",
          ahci_dev.cap, ahci_dev.cap2, ahci_dev.version, ahci_dev.port_map);

    // 启用AHCI模式
    let ghc = ahci_readl(mmio_base + 0x04); // HOST_CTL
    ahci_writel(ghc | (1 << 31), mmio_base + 0x04); // HOST_AHCI_EN
    
    // 启用中断（虽然不使用）
    ahci_writel(ghc | (1 << 31) | (1 << 1), mmio_base + 0x04); // HOST_AHCI_EN | HOST_IRQ_EN

    0
}

/// 扫描AHCI端口
fn ahci_port_scan(ahci_dev: &mut ahci_device) -> i32 {
    let port_map = ahci_dev.port_map;
    
    // 根据资料，板卡上固定只开启最低位的一个端口
    if port_map & 1 == 0 {
        error!("AHCI: Port 0 not implemented in port map");
        return -1;
    }

    info!("AHCI: Port 0 is implemented, initializing...");
    
    // 设置端口索引
    ahci_dev.port_idx = 0;
    ahci_dev.n_ports = 1;
    
    // 初始化端口0
    let port_mmio = ahci_dev.mmio_base + 0x100; // 端口0的寄存器基地址
    ahci_dev.port[0].port_mmio = port_mmio;
    
    // 检查端口状态
    let ssts = ahci_readl(port_mmio + 0x28); // PORT_SCR_STAT
    info!("AHCI: Port 0 SSTS = {:#x}", ssts);
    
    if (ssts & 0xF) != 0x3 {
        warn!("AHCI: No device detected on port 0 (SSTS={:#x})", ssts);
        return -1;
    }

    info!("AHCI: Device detected on port 0");
    ahci_dev.port_map_linkup = 1;

    0
}

/// 扫描SATA设备
fn ahci_sata_scan(ahci_dev: &mut ahci_device) {
    if ahci_dev.port_map_linkup & 1 == 0 {
        warn!("AHCI: No device linked up on port 0");
        return;
    }

    info!("AHCI: Scanning SATA device on port 0...");
    
    // 初始化块设备结构
    ahci_dev.blk_dev = ahci_blk_dev::default();
    ahci_dev.blk_dev.lba48 = true; // 默认支持LBA48
    ahci_dev.blk_dev.blksz = 512;  // 固定块大小512字节
    
    // TODO: 实际的设备识别和容量检测
    // 这里需要发送IDENTIFY命令获取设备信息
    ahci_dev.blk_dev.lba = 1024 * 1024; // 临时设置为1M块 = 512MB
    
    info!("AHCI: SATA device initialized - LBA48: {}, Block size: {}, Capacity: {} blocks", 
          ahci_dev.blk_dev.lba48, ahci_dev.blk_dev.blksz, ahci_dev.blk_dev.lba);
}

/// AHCI SATA读取函数
/// blknr: 读取开始块偏移
/// blkcnt: 读取块数量  
/// buffer: 读取数据的缓冲区
pub fn ahci_sata_read_common(
    ahci_dev: &ahci_device,
    blknr: u64,
    blkcnt: u32,
    buffer: *mut u8,
) -> u64 {
    let pdev = &ahci_dev.blk_dev;
    
    info!("AHCI: Reading {} blocks from block {}", blkcnt, blknr);
    
    if pdev.lba48 {
        // TODO: 实现LBA48读取
        // 这里需要构建ATA命令并通过AHCI执行
        warn!("AHCI: LBA48 read not fully implemented yet");
        0
    } else {
        // TODO: 实现LBA28读取
        warn!("AHCI: LBA28 read not implemented");
        0
    }
}

/// AHCI SATA写入函数
/// blknr: 写入开始块偏移
/// blkcnt: 写入块数量
/// buffer: 写入数据的缓冲区
pub fn ahci_sata_write_common(
    ahci_dev: &ahci_device,
    blknr: u64,
    blkcnt: u32,
    buffer: *mut u8,
) -> u64 {
    let pdev = &ahci_dev.blk_dev;
    let flags = ahci_dev.flags;
    
    info!("AHCI: Writing {} blocks to block {}", blkcnt, blknr);
    
    if pdev.lba48 {
        // TODO: 实现LBA48写入
        // 这里需要构建ATA命令并通过AHCI执行
        warn!("AHCI: LBA48 write not fully implemented yet");
        
        // 如果需要，执行缓存刷新
        if flags & SATA_FLAG_WCACHE != 0 && flags & SATA_FLAG_FLUSH_EXT != 0 {
            // TODO: 实现缓存刷新
            info!("AHCI: Cache flush requested");
        }
        0
    } else {
        // TODO: 实现LBA28写入
        warn!("AHCI: LBA28 write not implemented");
        0
    }
}