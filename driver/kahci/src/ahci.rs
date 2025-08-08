use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use sync::Mutex;

// AHCI 寄存器偏移
const AHCI_GHC: usize = 0x04;      // Global Host Control
const AHCI_IS: usize = 0x08;       // Interrupt Status
const AHCI_PI: usize = 0x0C;       // Port Implemented
const AHCI_VS: usize = 0x10;       // Version
const AHCI_CAP: usize = 0x00;      // Host Capabilities

// 端口寄存器偏移 (基于端口号 * 0x80)
const PORT_CLB: usize = 0x00;      // Command List Base Address
const PORT_CLBU: usize = 0x04;     // Command List Base Address Upper
const PORT_FB: usize = 0x08;       // FIS Base Address
const PORT_FBU: usize = 0x0C;      // FIS Base Address Upper
const PORT_IS: usize = 0x10;       // Interrupt Status
const PORT_IE: usize = 0x14;       // Interrupt Enable
const PORT_CMD: usize = 0x18;      // Command and Status
const PORT_TFD: usize = 0x20;      // Task File Data
const PORT_SIG: usize = 0x24;      // Signature
const PORT_SSTS: usize = 0x28;     // SATA Status
const PORT_SCTL: usize = 0x2C;     // SATA Control
const PORT_SERR: usize = 0x30;     // SATA Error
const PORT_SACT: usize = 0x34;     // SATA Active
const PORT_CI: usize = 0x38;       // Command Issue

// 全局主机控制位
const GHC_AE: u32 = 1 << 31;      // AHCI Enable
const GHC_IE: u32 = 1 << 1;       // Interrupt Enable
const GHC_HR: u32 = 1 << 0;       // HBA Reset

// 端口命令位
const CMD_ST: u32 = 1 << 0;       // Start
const CMD_SUD: u32 = 1 << 1;      // Spin-Up Device
const CMD_POD: u32 = 1 << 2;      // Power On Device
const CMD_FRE: u32 = 1 << 4;      // FIS Receive Enable
const CMD_FR: u32 = 1 << 14;      // FIS Receive Running
const CMD_CR: u32 = 1 << 15;      // Command List Running

// SATA 状态位
const SSTS_DET_MASK: u32 = 0xF;   // Device Detection
const SSTS_DET_PRESENT: u32 = 0x3; // Device present and communication established

// FIS 类型
const FIS_TYPE_REG_H2D: u8 = 0x27; // Register - Host to Device

// ATA 命令
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
const ATA_CMD_IDENTIFY: u8 = 0xEC;

#[repr(C, packed)]
struct CommandHeader {
    flags: u16,
    prdtl: u16,       // PRDT Length
    prdbc: u32,       // PRD Byte Count
    ctba: u32,        // Command Table Base Address
    ctbau: u32,       // Command Table Base Address Upper
    reserved: [u32; 4],
}

#[repr(C, packed)]
struct CommandTable {
    cfis: [u8; 64],   // Command FIS
    acmd: [u8; 16],   // ATAPI Command
    reserved: [u8; 48],
    prdt: [PrdtEntry; 65535], // PRDT entries (实际会少很多)
}

#[repr(C, packed)]
struct PrdtEntry {
    dba: u32,         // Data Base Address
    dbau: u32,        // Data Base Address Upper
    reserved: u32,
    dbc: u32,         // Data Byte Count (bit 31 = interrupt on completion)
}

#[repr(C, packed)]
struct RegH2DFis {
    fis_type: u8,     // FIS_TYPE_REG_H2D
    pmport: u8,       // Port multiplier
    command: u8,      // Command register
    features: u8,     // Features register
    lba0: u8,         // LBA low register, 7:0
    lba1: u8,         // LBA mid register, 15:8  
    lba2: u8,         // LBA high register, 23:16
    device: u8,       // Device register
    lba3: u8,         // LBA register, 31:24
    lba4: u8,         // LBA register, 39:32
    lba5: u8,         // LBA register, 47:40
    features_exp: u8, // Features register, expanded
    count: u8,        // Count register, 7:0
    count_exp: u8,    // Count register, 15:8
    reserved: u8,
    control: u8,      // Control register
}

pub struct AhciController {
    base: *mut u8,
    ports: u8,
    command_lists: Vec<*mut u8>,
    fis_areas: Vec<*mut u8>,
    command_tables: Vec<*mut u8>,
    port_capacities: Mutex<Vec<usize>>,
}

unsafe impl Send for AhciController {}
unsafe impl Sync for AhciController {}

#[derive(Debug)]
pub enum AhciError {
    InitializationFailed,
    PortNotConnected,
    CommandTimeout,
    DataTransferError,
    InvalidParameters,
}

impl AhciController {
    pub fn new(base: *mut u8) -> Result<Self, AhciError> {
        info!("AHCI: Initializing AHCI controller at {:#x}", base as usize);
        
        let mut controller = AhciController {
            base,
            ports: 0,
            command_lists: Vec::new(),
            fis_areas: Vec::new(), 
            command_tables: Vec::new(),
            port_capacities: Mutex::new(Vec::new()),
        };
        
        // 执行初始化
        controller.initialize()?;
        
        Ok(controller)
    }
    
    fn initialize(&mut self) -> Result<(), AhciError> {
        unsafe {
            // 1. 重置 HBA
            info!("AHCI: Resetting AHCI HBA...");
            let ghc_reg = self.base.add(AHCI_GHC) as *mut u32;
            write_volatile(ghc_reg, GHC_HR);
            
            // 等待重置完成
            let mut timeout = 1000;
            while read_volatile(ghc_reg) & GHC_HR != 0 && timeout > 0 {
                timeout -= 1;
                // 添加小延迟
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
            
            if timeout == 0 {
                error!("AHCI: HBA reset timeout");
                return Err(AhciError::InitializationFailed);
            }
            
            // 2. 启用 AHCI
            write_volatile(ghc_reg, GHC_AE | GHC_IE);
            
            // 3. 获取能力和端口信息
            let cap_reg = self.base.add(AHCI_CAP) as *mut u32;
            let cap = read_volatile(cap_reg);
            let ports_impl = read_volatile(self.base.add(AHCI_PI) as *mut u32);
            
            info!("AHCI: Capabilities: {:#x}", cap);
            info!("AHCI: Ports implemented: {:#x}", ports_impl);
            
            // 4. 初始化端口 - 根据资料，只有端口0有设备
            if ports_impl & 1 != 0 {
                info!("AHCI: Initializing port 0");
                self.init_port(0)?;
            } else {
                warn!("AHCI: Port 0 not implemented");
                return Err(AhciError::InitializationFailed);
            }
        }
        
        info!("AHCI: Controller initialization completed");
        Ok(())
    }
    
    fn init_port(&self, port: u8) -> Result<(), AhciError> {
        let port_base = unsafe { self.base.add(0x100 + (port as usize) * 0x80) };
        
        unsafe {
            // 检查设备连接状态
            let ssts = read_volatile(port_base.add(PORT_SSTS) as *mut u32);
            if (ssts & SSTS_DET_MASK) != SSTS_DET_PRESENT {
                warn!("AHCI: No device detected on port {}", port);
                return Ok(());
            }
            
            info!("AHCI: Device detected on port {}, initializing...", port);
            
            // 停止端口
            let cmd_reg = port_base.add(PORT_CMD) as *mut u32;
            let mut cmd = read_volatile(cmd_reg);
            cmd &= !(CMD_ST | CMD_FRE);
            write_volatile(cmd_reg, cmd);
            
            // 等待停止
            let mut timeout = 1000;
            while read_volatile(cmd_reg) & (CMD_FR | CMD_CR) != 0 && timeout > 0 {
                timeout -= 1;
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
            
            // 分配命令列表 (1KB 对齐)
            let command_list = devices::frame_alloc().expect("Failed to allocate command list");
            let command_list_ptr = (command_list.raw() | 0x8000_0000_0000_0000) as *mut u8;
            
            // 分配 FIS 区域 (256 字节对齐)  
            let fis_area = devices::frame_alloc().expect("Failed to allocate FIS area");
            let fis_area_ptr = (fis_area.raw() | 0x8000_0000_0000_0000) as *mut u8;
            
            // 设置命令列表和 FIS 基地址
            write_volatile(port_base.add(PORT_CLB) as *mut u32, command_list.raw() as u32);
            write_volatile(port_base.add(PORT_CLBU) as *mut u32, (command_list.raw() >> 32) as u32);
            write_volatile(port_base.add(PORT_FB) as *mut u32, fis_area.raw() as u32);
            write_volatile(port_base.add(PORT_FBU) as *mut u32, (fis_area.raw() >> 32) as u32);
            
            // 清除错误状态
            write_volatile(port_base.add(PORT_SERR) as *mut u32, 0xFFFFFFFF);
            write_volatile(port_base.add(PORT_IS) as *mut u32, 0xFFFFFFFF);
            
            // 启用端口
            cmd = read_volatile(cmd_reg);
            cmd |= CMD_FRE | CMD_SUD | CMD_POD;
            write_volatile(cmd_reg, cmd);
            
            // 等待 FIS 接收运行
            timeout = 1000;
            while read_volatile(cmd_reg) & CMD_FR == 0 && timeout > 0 {
                timeout -= 1;
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
            
            // 启动命令处理
            cmd |= CMD_ST;
            write_volatile(cmd_reg, cmd);
            
            info!("AHCI: Port {} initialized successfully", port);
            
            // 获取设备容量
            if let Ok(capacity) = self.identify_device(port) {
                self.port_capacities.lock().push(capacity);
                info!("AHCI: Port {} device capacity: {} sectors", port, capacity);
            }
        }
        
        Ok(())
    }
    
    fn identify_device(&self, port: u8) -> Result<usize, AhciError> {
        // 分配数据缓冲区
        let data_frame = devices::frame_alloc().expect("Failed to allocate identify buffer");
        let data_ptr = (data_frame.raw() | 0x8000_0000_0000_0000) as *mut u8;
        
        // 执行 IDENTIFY 命令
        self.execute_command(port, ATA_CMD_IDENTIFY, 0, 1, data_ptr)?;
        
        // 解析设备信息
        unsafe {
            let data = data_ptr as *const u16;
            // 扇区数在偏移 100-103 (48位LBA) 或 60-61 (28位LBA)
            let lba48_sectors = (data.add(103).read_volatile() as u64) << 48 |
                               (data.add(102).read_volatile() as u64) << 32 |
                               (data.add(101).read_volatile() as u64) << 16 |
                               (data.add(100).read_volatile() as u64);
            
            if lba48_sectors > 0 {
                Ok(lba48_sectors as usize)
            } else {
                let lba28_sectors = (data.add(61).read_volatile() as u32) << 16 |
                                   (data.add(60).read_volatile() as u32);
                Ok(lba28_sectors as usize)
            }
        }
    }
    
    pub fn read_blocks(&self, port: u8, lba: usize, buf: &mut [u8]) -> Result<(), AhciError> {
        let sectors = buf.len() / 512;
        if sectors == 0 || sectors > 65535 {
            return Err(AhciError::InvalidParameters);
        }
        
        self.execute_command(port, ATA_CMD_READ_DMA_EXT, lba as u64, sectors, buf.as_mut_ptr())
    }
    
    pub fn write_blocks(&self, port: u8, lba: usize, buf: &[u8]) -> Result<(), AhciError> {
        let sectors = buf.len() / 512;
        if sectors == 0 || sectors > 65535 {
            return Err(AhciError::InvalidParameters);
        }
        
        self.execute_command(port, ATA_CMD_WRITE_DMA_EXT, lba as u64, sectors, buf.as_ptr() as *mut u8)
    }
    
    fn execute_command(&self, port: u8, command: u8, lba: u64, sectors: usize, data_ptr: *mut u8) -> Result<(), AhciError> {
        let port_base = unsafe { self.base.add(0x100 + (port as usize) * 0x80) };
        
        unsafe {
            // 获取命令列表基地址
            let clb = read_volatile(port_base.add(PORT_CLB) as *mut u32) as u64 |
                     ((read_volatile(port_base.add(PORT_CLBU) as *mut u32) as u64) << 32);
            let cmd_list = (clb | 0x8000_0000_0000_0000) as *mut CommandHeader;
            
            // 分配命令表
            let cmd_table_frame = devices::frame_alloc().expect("Failed to allocate command table");
            let cmd_table_ptr = (cmd_table_frame.raw() | 0x8000_0000_0000_0000) as *mut CommandTable;
            let cmd_table = &mut *cmd_table_ptr;
            
            // 设置命令头
            let cmd_header = &mut *cmd_list.add(0);
            cmd_header.flags = 5; // FIS长度 = 5 DWORDs
            cmd_header.prdtl = 1; // 一个 PRDT 条目
            cmd_header.ctba = cmd_table_frame.raw() as u32;
            cmd_header.ctbau = (cmd_table_frame.raw() >> 32) as u32;
            
            // 清零命令表
            core::ptr::write_bytes(cmd_table_ptr, 0, core::mem::size_of::<CommandTable>());
            
            // 构建 H2D Register FIS
            let fis = &mut cmd_table.cfis;
            fis[0] = FIS_TYPE_REG_H2D;
            fis[1] = 0x80; // 命令寄存器
            fis[2] = command;
            fis[3] = 0; // Features
            
            // LBA 寄存器
            fis[4] = (lba & 0xFF) as u8;
            fis[5] = ((lba >> 8) & 0xFF) as u8;
            fis[6] = ((lba >> 16) & 0xFF) as u8;
            fis[7] = 0x40; // LBA 模式
            fis[8] = ((lba >> 24) & 0xFF) as u8;
            fis[9] = ((lba >> 32) & 0xFF) as u8;
            fis[10] = ((lba >> 40) & 0xFF) as u8;
            fis[11] = 0; // Features Expanded
            
            // 扇区计数
            fis[12] = (sectors & 0xFF) as u8;
            fis[13] = ((sectors >> 8) & 0xFF) as u8;
            
            // 设置 PRDT
            cmd_table.prdt[0].dba = data_ptr as usize as u32;
            cmd_table.prdt[0].dbau = ((data_ptr as usize) >> 32) as u32;
            cmd_table.prdt[0].dbc = (sectors * 512 - 1) as u32; // 字节数 - 1
            
            // 发送命令
            write_volatile(port_base.add(PORT_CI) as *mut u32, 1);
            
            // 等待命令完成
            let mut timeout = 10000;
            while read_volatile(port_base.add(PORT_CI) as *mut u32) & 1 != 0 && timeout > 0 {
                timeout -= 1;
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
            }
            
            if timeout == 0 {
                error!("Command timeout on port {}", port);
                return Err(AhciError::CommandTimeout);
            }
            
            // 检查错误
            let tfd = read_volatile(port_base.add(PORT_TFD) as *mut u32);
            if tfd & 0x1 != 0 { // Error bit
                error!("Command error on port {}: TFD={:#x}", port, tfd);
                return Err(AhciError::DataTransferError);
            }
        }
        
        Ok(())
    }
    
    pub fn is_port_connected(&self, port: u8) -> bool {
        let port_base = unsafe { self.base.add(0x100 + (port as usize) * 0x80) };
        unsafe {
            let ssts = read_volatile(port_base.add(PORT_SSTS) as *mut u32);
            (ssts & SSTS_DET_MASK) == SSTS_DET_PRESENT
        }
    }
    
    pub fn get_capacity(&self, port: u8) -> usize {
        self.port_capacities.lock().get(port as usize).copied().unwrap_or(0) * 512
    }
    
    pub fn handle_interrupt(&self, port: u8) -> bool {
        let port_base = unsafe { self.base.add(0x100 + (port as usize) * 0x80) };
        unsafe {
            let is = read_volatile(port_base.add(PORT_IS) as *mut u32);
            if is != 0 {
                // 清除中断状态
                write_volatile(port_base.add(PORT_IS) as *mut u32, is);
                true
            } else {
                false
            }
        }
    }
}