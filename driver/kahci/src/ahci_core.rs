// AHCI 核心实现，参考 ahci/c/drv_ahci.c，支持真实硬件操作
extern crate alloc;

use crate::ahci_types::*;
use crate::platform::*;
use core::ptr::{read_volatile, write_volatile};
use alloc::vec::Vec;

// 调试开关
const KAHCI_LOG: bool = false;

// 调试输出函数
extern "C" {
    fn debug_uart_string(s: *const u8);
}

fn ahci_debug_print(msg: &str) {
    if !KAHCI_LOG { return; }
    let debug_msg = alloc::format!("[KAHCI] {}\n\0", msg);
    unsafe { debug_uart_string(debug_msg.as_ptr()); }
}

// AHCI寄存器读写函数
fn ahci_readl(addr: u64) -> u32 {
    unsafe { read_volatile(addr as *mut u32) }
}

fn ahci_writel(data: u32, addr: u64) {
    unsafe { write_volatile(addr as *mut u32, data) }
}

// 微秒级延迟
fn ahci_platform_delay_us(us: u32) {
    for _ in 0..(us * 100) {
        unsafe { core::arch::asm!("nop") };
    }
}

/// AHCI初始化函数
pub fn ahci_init(ahci_dev: &mut ahci_device) -> i32 {
    // AHCI控制器寄存器物理基地址是0x400e0000
    ahci_dev.mmio_base = ahci_phys_to_uncached(0x400e0000);
    
    ahci_debug_print(&alloc::format!("AHCI: Initializing controller at physical 0x400e0000, virtual 0x{:x}", 
          ahci_dev.mmio_base));

    // 1. 初始化AHCI主机控制器
    let ret = ahci_host_init(ahci_dev);
    if ret != 0 {
        ahci_debug_print("AHCI: Host initialization failed");
        return -1;
    }

    // 2. 扫描端口
    let ret = ahci_port_scan(ahci_dev);
    if ret != 0 {
        ahci_debug_print("AHCI: Port scan failed");
        return -1;
    }

    // 3. 初始化端口
    let ret = ahci_port_start(ahci_dev, 0);
    if ret != 0 {
        ahci_debug_print("AHCI: Port start failed");
        return -1;
    }

    // 4. 识别设备
    let ret = ahci_device_identify(ahci_dev);
    if ret != 0 {
        ahci_debug_print("AHCI: Device identify failed");
        return -1;
    }

    // 5. 打印控制器信息
    ahci_print_info(ahci_dev);

    ahci_debug_print("AHCI: Initialization completed successfully");
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
    
    ahci_debug_print(&alloc::format!("AHCI: CAP=0x{:x}, CAP2=0x{:x}, VERSION=0x{:x}, PORT_MAP=0x{:x}",
          ahci_dev.cap, ahci_dev.cap2, ahci_dev.version, ahci_dev.port_map));

    // 启用AHCI模式
    let mut ghc = ahci_readl(mmio_base + 0x04); // HOST_CTL
    ghc |= 1 << 31; // HOST_AHCI_EN
    ahci_writel(ghc, mmio_base + 0x04);
    
    // 启用全局中断（虽然不使用）
    ghc |= 1 << 1; // HOST_IRQ_EN
    ahci_writel(ghc, mmio_base + 0x04);
    
    0
}

/// 扫描AHCI端口
fn ahci_port_scan(ahci_dev: &mut ahci_device) -> i32 {
    let port_map = ahci_dev.port_map;
    
    // 根据资料，板卡上固定只开启最低位的一个端口
    if port_map & 1 == 0 {
        ahci_debug_print("AHCI: Port 0 not implemented in port map");
        return -1;
    }

    ahci_debug_print("AHCI: Port 0 is implemented, checking status...");
    
    // 设置端口信息
    ahci_dev.port_idx = 0;
    ahci_dev.n_ports = 1;
    
    // 初始化端口0寄存器地址
    let port_mmio = ahci_dev.mmio_base + 0x100; // 端口0的寄存器基地址
    ahci_dev.port[0].port_mmio = port_mmio;
    
    // 检查端口状态
    let ssts = ahci_readl(port_mmio + 0x28); // PORT_SCR_STAT
    ahci_debug_print(&alloc::format!("AHCI: Port 0 SSTS = 0x{:x}", ssts));
    
    // 检查设备检测状态 (位0-3)
    let det = ssts & 0xF;
    if det != 0x3 {
        ahci_debug_print(&alloc::format!("AHCI: No device detected on port 0 (DET=0x{:x})", det));
        return -1;
    }

    // 检查接口电源管理状态 (位8-11)
    let ipm = (ssts >> 8) & 0xF;
    if ipm != 0x1 {
        ahci_debug_print(&alloc::format!("AHCI: Device not in active state (IPM=0x{:x})", ipm));
    }

    ahci_debug_print("AHCI: Device detected and active on port 0");
    ahci_dev.port_map_linkup = 1;

    0
}

/// 启动端口
fn ahci_port_start(ahci_dev: &mut ahci_device, port: usize) -> i32 {
    let port_mmio = ahci_dev.port[port].port_mmio;
    
    ahci_debug_print(&alloc::format!("AHCI: Starting port {}", port));
    
    // 分配命令列表 (1KB对齐, 每个端口1KB)
    let cl_size = 1024u64;
    let cl_addr = ahci_malloc_align(cl_size, 1024);
    if cl_addr == 0 {
        ahci_debug_print("AHCI: Failed to allocate command list");
        return -1;
    }
    
    // 分配接收FIS (256字节对齐)
    let fis_size = 256u64;
    let fis_addr = ahci_malloc_align(fis_size, 256);
    if fis_addr == 0 {
        ahci_debug_print("AHCI: Failed to allocate RX FIS");
        return -1;
    }
    
    // 分配命令表 (128字节对齐, 32个命令表)
    let ct_size = 256u64; // 每个命令表256字节
    let ct_addr = ahci_malloc_align(ct_size * 32, 128);
    if ct_addr == 0 {
        ahci_debug_print("AHCI: Failed to allocate command tables");
        return -1;
    }
    
    // 清零分配的内存
    ahci_memset(cl_addr as *mut u8, 0, cl_size);
    ahci_memset(fis_addr as *mut u8, 0, fis_size);
    ahci_memset(ct_addr as *mut u8, 0, ct_size * 32);
    
    // 计算物理地址用于DMA
    let cl_phys = ahci_virt_to_phys(cl_addr);
    let fis_phys = ahci_virt_to_phys(fis_addr);
    let ct_phys = ahci_virt_to_phys(ct_addr);
    
    // 设置端口DMA地址
    ahci_dev.port[port].cmd_slot = cl_addr as *mut u8;
    ahci_dev.port[port].cmd_slot_dma = cl_phys;
    ahci_dev.port[port].rx_fis_dma = fis_phys;
    ahci_dev.port[port].cmd_tbl_dma = ct_phys;
    ahci_dev.port[port].cmd_tbl = ct_addr;
    
    ahci_debug_print(&alloc::format!("AHCI: Allocated DMA buffers - CL:0x{:x}(phys:0x{:x}) FIS:0x{:x}(phys:0x{:x}) CT:0x{:x}(phys:0x{:x})",
                     cl_addr, cl_phys, fis_addr, fis_phys, ct_addr, ct_phys));
    
    // 停止端口
    let mut cmd = ahci_readl(port_mmio + 0x18); // PORT_CMD
    cmd &= !(1 << 0); // 清除ST位
    cmd &= !(1 << 4); // 清除FRE位
    ahci_writel(cmd, port_mmio + 0x18);
    
    // 等待端口停止
    let mut timeout = 1000;
    while (ahci_readl(port_mmio + 0x18) & ((1 << 15) | (1 << 14))) != 0 && timeout > 0 {
        ahci_platform_delay_us(1000);
        timeout -= 1;
    }
    
    if timeout == 0 {
        ahci_debug_print("AHCI: Timeout waiting for port to stop");
        return -1;
    }
    
    // 设置命令列表和FIS基址（使用物理地址）
    ahci_writel(cl_phys as u32, port_mmio + 0x00); // PORT_CLB
    ahci_writel((cl_phys >> 32) as u32, port_mmio + 0x04); // PORT_CLBU
    ahci_writel(fis_phys as u32, port_mmio + 0x08); // PORT_FB
    ahci_writel((fis_phys >> 32) as u32, port_mmio + 0x0C); // PORT_FBU
    
    // 清除中断状态
    ahci_writel(0xFFFFFFFF, port_mmio + 0x10); // PORT_IS
    
    // 启用FIS接收
    cmd = ahci_readl(port_mmio + 0x18);
    cmd |= 1 << 4; // FRE
    ahci_writel(cmd, port_mmio + 0x18);
    
    // 启动端口
    cmd |= 1 << 0; // ST
    ahci_writel(cmd, port_mmio + 0x18);
    
    ahci_debug_print(&alloc::format!("AHCI: Port {} started successfully", port));
    0
}

/// 识别设备
fn ahci_device_identify(ahci_dev: &mut ahci_device) -> i32 {
    ahci_debug_print("AHCI: Identifying device...");
    
    // 分配识别数据缓冲区
    let id_size = 512u64;
    let id_addr = ahci_malloc_align(id_size, 4);
    if id_addr == 0 {
        ahci_debug_print("AHCI: Failed to allocate identify buffer");
        return -1;
    }
    
    // 构建IDENTIFY DEVICE命令
    let mut cfis = [0u8; 20];
    cfis[0] = 0x27; // FIS_TYPE_REGISTER_H2D
    cfis[1] = 0x80; // Command bit set
    cfis[2] = 0xEC; // ATA_CMD_IDENTIFY_DEVICE
    
    // 执行命令
    let result = ahci_exec_ata_cmd(
        ahci_dev,
        0, // port 0
        &cfis,
        id_addr as *mut u8,
        512,
        false, // 读取操作
    );
    
    if result == 0 {
        ahci_debug_print("AHCI: IDENTIFY DEVICE command failed");
        return -1;
    }
    
    // 解析识别数据
    let id_data = unsafe { core::slice::from_raw_parts(id_addr as *const u16, 256) };
    
    // 检查LBA48支持 (word 83, bit 10)
    let lba48_support = (id_data[83] & (1 << 10)) != 0;
    
    // 获取容量
    let capacity = if lba48_support {
        // LBA48 容量 (words 100-103)
        let lba48_0 = id_data[100] as u64;
        let lba48_1 = id_data[101] as u64;
        let lba48_2 = id_data[102] as u64;
        let lba48_3 = id_data[103] as u64;
        lba48_0 | (lba48_1 << 16) | (lba48_2 << 32) | (lba48_3 << 48)
    } else {
        // LBA28 容量 (words 60-61)
        let lba28_0 = id_data[60] as u64;
        let lba28_1 = id_data[61] as u64;
        lba28_0 | (lba28_1 << 16)
    };
    
    // 设置设备信息
    ahci_dev.blk_dev.lba48 = lba48_support;
    ahci_dev.blk_dev.lba = capacity;
    ahci_dev.blk_dev.blksz = 512;
    
    ahci_debug_print(&alloc::format!("AHCI: Device identified - LBA48: {}, Capacity: {} sectors ({} MB)", 
                     lba48_support, capacity, capacity * 512 / (1024 * 1024)));

    // 分区探测
    ahci_dev.blk_dev.lba_offset = 0;
    detect_partitions(ahci_dev);
    ahci_debug_print(&alloc::format!("AHCI: Partition LBA offset = {}", ahci_dev.blk_dev.lba_offset));

    0
}

/// 执行AHCI命令
fn ahci_exec_ata_cmd(
    ahci_dev: &ahci_device,
    port: u8,
    cfis: &[u8; 20],
    buffer: *mut u8,
    buf_len: u32,
    is_write: bool,
) -> u32 {
    let port_mmio = ahci_dev.port[port as usize].port_mmio;
    
    // 获取可用命令槽
    let ci = ahci_readl(port_mmio + 0x38); // PORT_CMD_ISSUE
    let mut cmd_slot = 32;
    for i in 0..32 {
        if (ci & (1 << i)) == 0 {
            cmd_slot = i;
            break;
        }
    }
    
    if cmd_slot == 32 {
        ahci_debug_print("AHCI: No free command slot");
        return 0;
    }
    
    // 构建命令列表条目
    let cl_base = ahci_dev.port[port as usize].cmd_slot_dma;
    let _cl_entry_addr = cl_base + (cmd_slot as u64 * 32);
    
    // 设置命令表地址
    let ct_addr = ahci_dev.port[port as usize].cmd_tbl_dma + (cmd_slot as u64 * 256);
    
    // 填充命令表（使用虚拟地址）
    let ct_virt = ahci_dev.port[port as usize].cmd_tbl + (cmd_slot as u64 * 256);
    ahci_memcpy(ct_virt as *mut u8, cfis.as_ptr(), 20);
    
    // 设置PRDT (Physical Region Descriptor Table)
    if buf_len > 0 && !buffer.is_null() {
        let prdt_addr = ct_virt + 128; // PRDT在命令表偏移128处
        let buffer_phys = ahci_virt_to_phys(buffer as u64);
        
        // PRDT条目: DBA(8字节) + Reserved(4字节) + DBC(4字节)
        unsafe {
            // Data Base Address (低32位)
            write_volatile((prdt_addr) as *mut u32, buffer_phys as u32);
            // Data Base Address (高32位)
            write_volatile((prdt_addr + 4) as *mut u32, (buffer_phys >> 32) as u32);
            // Reserved
            write_volatile((prdt_addr + 8) as *mut u32, 0);
            // Data Byte Count (DBC = 传输字节数 - 1)
            write_volatile((prdt_addr + 12) as *mut u32, buf_len - 1);
        }
    }
    
    // 填充命令列表条目（使用虚拟地址）
    let cl_virt = ahci_dev.port[port as usize].cmd_slot as u64;
    let cl_entry_virt = cl_virt + (cmd_slot as u64 * 32);
    let prdtl = if buf_len > 0 { 1 } else { 0 }; // PRDT条目数
    let cfl = 5; // 命令FIS长度 (5个DWORD)
    let opts = cfl | (prdtl << 16) | if is_write { 1 << 6 } else { 0 };
    
    unsafe {
        // Command List Entry
        write_volatile(cl_entry_virt as *mut u32, opts);
        write_volatile((cl_entry_virt + 4) as *mut u32, 0); // PRDBC
        write_volatile((cl_entry_virt + 8) as *mut u32, ct_addr as u32); // CTBA低32位
        write_volatile((cl_entry_virt + 12) as *mut u32, (ct_addr >> 32) as u32); // CTBA高32位
    }
    
    // 同步缓存
    ahci_sync_dcache();
    
    // 发出命令
    ahci_writel(1 << cmd_slot, port_mmio + 0x38); // PORT_CMD_ISSUE
    
    // 等待命令完成
    let mut timeout = 10000; // 10秒超时
    while (ahci_readl(port_mmio + 0x38) & (1 << cmd_slot)) != 0 && timeout > 0 {
        ahci_platform_delay_us(1000); // 1ms
        timeout -= 1;
    }
    
    if timeout == 0 {
        ahci_debug_print("AHCI: Command timeout");
        return 0;
    }
    
    // 检查错误
    let is_reg = ahci_readl(port_mmio + 0x10); // PORT_IS
    if (is_reg & (1 << 30)) != 0 { // TFES - Task File Error Status
        let tfd = ahci_readl(port_mmio + 0x20); // PORT_TFD
        ahci_debug_print(&alloc::format!("AHCI: Task file error - IS:0x{:x} TFD:0x{:x}", is_reg, tfd));
        return 0;
    }
    
    // 清除中断状态
    ahci_writel(is_reg, port_mmio + 0x10);
    
    ahci_sync_dcache();
    buf_len
}

/// AHCI SATA读取函数 - 真实硬件版本
pub fn ahci_sata_read_common(
    ahci_dev: &ahci_device,
    blknr: u64,
    blkcnt: u32,
    buffer: *mut u8,
) -> u64 {
    let pdev = &ahci_dev.blk_dev;
    let phys_lba = blknr.saturating_add(pdev.lba_offset);

    ahci_debug_print(&alloc::format!("Reading {} blocks from LBA {} (phys {})", blkcnt, blknr, phys_lba));
    
    // 参数检查
    if blkcnt == 0 || buffer.is_null() {
        ahci_debug_print("Invalid parameters");
        return 0;
    }
    
    if ahci_dev.port_map_linkup & 1 == 0 {
        ahci_debug_print("No device connected");
        return 0;
    }
    
    // 构建读取命令（使用 phys_lba）
    let mut cfis = [0u8; 20];
    cfis[0] = 0x27; // FIS_TYPE_REGISTER_H2D
    cfis[1] = 0x80; // Command bit
    
    if pdev.lba48 && (phys_lba >= (1u64 << 28) || blkcnt > 256) {
        cfis[2] = 0x25; // READ DMA EXT
        cfis[4] = phys_lba as u8;
        cfis[5] = (phys_lba >> 8) as u8;
        cfis[6] = (phys_lba >> 16) as u8;
        cfis[7] = 0x40;
        cfis[8] = (phys_lba >> 24) as u8;
        cfis[9] = (phys_lba >> 32) as u8;
        cfis[10] = (phys_lba >> 40) as u8;
        cfis[12] = blkcnt as u8;
        cfis[13] = (blkcnt >> 8) as u8;
    } else {
        cfis[2] = 0xC8; // READ DMA
        cfis[4] = phys_lba as u8;
        cfis[5] = (phys_lba >> 8) as u8;
        cfis[6] = (phys_lba >> 16) as u8;
        cfis[7] = 0x40 | ((phys_lba >> 24) as u8 & 0x0F);
        cfis[12] = blkcnt as u8;
    }
    
    let result = ahci_exec_ata_cmd(
        ahci_dev,
        0,
        &cfis,
        buffer,
        blkcnt * 512,
        false,
    );

    let blocks_read = result / 512;
    ahci_debug_print(&alloc::format!("Read completed: {} blocks", blocks_read));
    blocks_read as u64
}

/// AHCI SATA写入函数 - 真实硬件版本
pub fn ahci_sata_write_common(
    ahci_dev: &ahci_device,
    blknr: u64,
    blkcnt: u32,
    buffer: *mut u8,
) -> u64 {
    let pdev = &ahci_dev.blk_dev;
    let phys_lba = blknr.saturating_add(pdev.lba_offset);

    ahci_debug_print(&alloc::format!("Writing {} blocks to LBA {} (phys {})", blkcnt, blknr, phys_lba));
    
    // 参数检查
    if blkcnt == 0 || buffer.is_null() {
        ahci_debug_print("Invalid parameters");
        return 0;
    }
    
    if ahci_dev.port_map_linkup & 1 == 0 {
        ahci_debug_print("No device connected");
        return 0;
    }
    
    // 构建写入命令
    let mut cfis = [0u8; 20];
    cfis[0] = 0x27; // FIS_TYPE_REGISTER_H2D
    cfis[1] = 0x80; // Command bit
    
    if pdev.lba48 && (phys_lba >= (1u64 << 28) || blkcnt > 256) {
        // LBA48 写入
        cfis[2] = 0x35; // ATA_CMD_WRITE_DMA_EXT
        cfis[4] = phys_lba as u8;
        cfis[5] = (phys_lba >> 8) as u8;
        cfis[6] = (phys_lba >> 16) as u8;
        cfis[7] = 0x40; // LBA模式
        cfis[8] = (phys_lba >> 24) as u8;
        cfis[9] = (phys_lba >> 32) as u8;
        cfis[10] = (phys_lba >> 40) as u8;
        cfis[12] = blkcnt as u8;
        cfis[13] = (blkcnt >> 8) as u8;
    } else {
        // LBA28 写入
        cfis[2] = 0xCA; // ATA_CMD_WRITE_DMA
        cfis[4] = phys_lba as u8;
        cfis[5] = (phys_lba >> 8) as u8;
        cfis[6] = (phys_lba >> 16) as u8;
        cfis[7] = 0x40 | ((phys_lba >> 24) as u8 & 0x0F);
        cfis[12] = blkcnt as u8;
    }
    
    let result = ahci_exec_ata_cmd(
        ahci_dev,
        0, // port 0
        &cfis,
        buffer,
        blkcnt * 512,
        true,
    );
    
    let blocks_written = result / 512;
    ahci_debug_print(&alloc::format!("Write completed: {} blocks", blocks_written));
    blocks_written as u64
}

/// 输出AHCI控制器信息
fn ahci_print_info(ahci_dev: &ahci_device) {
    let vers = ahci_dev.version;
    let cap = ahci_dev.cap;
    let impl_0 = ahci_dev.port_map;
    let speed = (cap >> 20) & 0xf;

    let speed_s = match speed {
        1 => "1.5",
        2 => "3",
        3 => "6",
        _ => "?",
    };

    ahci_debug_print(&alloc::format!("AHCI vers {:02x}{:02x}.{:02x}{:02x}, {} slots, {} ports, {} Gbps, 0x{:x} impl",
        vers >> 24 & 0xff,
        vers >> 16 & 0xff,
        vers >> 8 & 0xff,
        vers & 0xff,
        (cap >> 8 & 0x1f) + 1,
        (cap & 0x1f) + 1,
        speed_s,
        impl_0
    ));

    let mut flags = Vec::new();
    if cap & (1 << 31) != 0 { flags.push("64bit"); }
    if cap & (1 << 30) != 0 { flags.push("ncq"); }
    if cap & (1 << 29) != 0 { flags.push("sntf"); }
    if cap & (1 << 28) != 0 { flags.push("ilck"); }
    if cap & (1 << 27) != 0 { flags.push("stag"); }
    if cap & (1 << 26) != 0 { flags.push("pm"); }
    
    if !flags.is_empty() {
        ahci_debug_print(&alloc::format!("flags: {}", flags.join(" ")));
    }
}

// 在设备识别成功后，尝试探测分区起始LBA
fn detect_partitions(ahci_dev: &mut ahci_device) {
    // 读取LBA 0（MBR）
    let mut sector = [0u8; 512];
    let res = ahci_sata_read_common(
        ahci_dev,
        0,
        1,
        sector.as_mut_ptr(),
    );
    if res != 1 { return; }

    // 检查MBR签名
    if sector[510] == 0x55 && sector[511] == 0xAA {
        // 第一个分区条目偏移446，起始LBA在偏移 446+8..=446+11
        let start = 446 + 8;
        let start_lba = u32::from_le_bytes([sector[start], sector[start+1], sector[start+2], sector[start+3]]) as u64;
        if start_lba > 0 && start_lba < ahci_dev.blk_dev.lba { ahci_dev.blk_dev.lba_offset = start_lba; }
    } else if &sector[0..8] == b"EFI PART" {
        // GPT头在LBA1，分区表从LBA2开始；通常第一个分区在更后面，这里简单不处理
        ahci_dev.blk_dev.lba_offset = 0; // GPT 由上层解析，这里不偏移
    } else {
        ahci_dev.blk_dev.lba_offset = 0;
    }
}