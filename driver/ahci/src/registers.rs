//! AHCI Register Definitions
//! 
//! Based on AHCI 1.1 specification and compatible with SATA 2.6

use volatile::{Volatile, ReadOnly};
use bitflags::bitflags;

/// AHCI Generic Host Control registers
#[repr(C)]
pub struct AhciGenericHostControl {
    pub cap: ReadOnly<u32>,          // 0x00: Host Capabilities
    pub ghc: Volatile<u32>,          // 0x04: Global Host Control
    pub is: Volatile<u32>,           // 0x08: Interrupt Status
    pub pi: ReadOnly<u32>,           // 0x0C: Ports Implemented
    pub vs: ReadOnly<u32>,           // 0x10: Version
    pub ccc_ctl: Volatile<u32>,      // 0x14: Command Completion Coalescing Control
    pub ccc_ports: Volatile<u32>,    // 0x18: Command Completion Coalescing Ports
    pub em_loc: ReadOnly<u32>,       // 0x1C: Enclosure Management Location
    pub em_ctl: Volatile<u32>,       // 0x20: Enclosure Management Control
    pub cap2: ReadOnly<u32>,         // 0x24: Host Capabilities Extended
    pub bohc: Volatile<u32>,         // 0x28: BIOS/OS Handoff Control and Status
}

/// AHCI Port registers
#[repr(C)]
pub struct AhciPortRegs {
    pub clb: Volatile<u64>,          // 0x00: Command List Base Address
    pub fb: Volatile<u64>,           // 0x08: FIS Base Address
    pub is: Volatile<u32>,           // 0x10: Interrupt Status
    pub ie: Volatile<u32>,           // 0x14: Interrupt Enable
    pub cmd: Volatile<u32>,          // 0x18: Command and Status
    pub _reserved1: u32,             // 0x1C: Reserved
    pub tfd: ReadOnly<u32>,          // 0x20: Task File Data
    pub sig: ReadOnly<u32>,          // 0x24: Signature
    pub ssts: ReadOnly<u32>,         // 0x28: Serial ATA Status
    pub sctl: Volatile<u32>,         // 0x2C: Serial ATA Control
    pub serr: Volatile<u32>,         // 0x30: Serial ATA Error
    pub sact: Volatile<u32>,         // 0x34: Serial ATA Active
    pub ci: Volatile<u32>,           // 0x38: Command Issue
    pub sntf: Volatile<u32>,         // 0x3C: Serial ATA Notification
    pub fbs: Volatile<u32>,          // 0x40: FIS-based Switching Control
    pub devslp: Volatile<u32>,       // 0x44: Device Sleep
    pub _reserved2: [u32; 10],       // 0x48-0x6F: Reserved
    pub vs: [Volatile<u32>; 4],      // 0x70-0x7F: Vendor Specific
}

bitflags! {
    /// Host Capabilities (CAP) register flags
    pub struct HostCapabilities: u32 {
        const S64A = 1 << 31;           // Supports 64-bit Addressing
        const SNCQ = 1 << 30;           // Supports Native Command Queuing
        const SSNTF = 1 << 29;          // Supports SNotification Register
        const SMPS = 1 << 28;           // Supports Mechanical Presence Switch
        const SSS = 1 << 27;            // Supports Staggered Spin-up
        const SALP = 1 << 26;           // Supports Aggressive Link Power Management
        const SAL = 1 << 25;            // Supports Activity LED
        const SCLO = 1 << 24;           // Supports Command List Override
        const SAM = 1 << 18;            // Supports AHCI mode only
        const SPM = 1 << 17;            // Supports Port Multiplier
        const FBSS = 1 << 16;           // FIS-based Switching Supported
        const PMD = 1 << 15;            // PIO Multiple DRQ Block
        const SSC = 1 << 14;            // Slumber State Capable
        const PSC = 1 << 13;            // Partial State Capable
        const CCCS = 1 << 7;            // Command Completion Coalescing Supported
        const EMS = 1 << 6;             // Enclosure Management Supported
        const SXS = 1 << 5;             // Supports External SATA
    }
}

bitflags! {
    /// Global Host Control (GHC) register flags
    pub struct GlobalHostControl: u32 {
        const AE = 1 << 31;             // AHCI Enable
        const MRSM = 1 << 2;            // MSI Revert to Single Message
        const IE = 1 << 1;              // Interrupt Enable
        const HR = 1 << 0;              // HBA Reset
    }
}

bitflags! {
    /// Port Command and Status (PxCMD) register flags
    pub struct PortCommand: u32 {
        const ICC_MASK = 0xF << 28;     // Interface Communication Control
        const ICC_ACTIVE = 0x1 << 28;   // Interface in active state
        const ASP = 1 << 27;            // Aggressive Slumber/Partial
        const ALPE = 1 << 26;           // Aggressive Link Power Management Enable
        const DLAE = 1 << 25;           // Drive LED on ATAPI Enable
        const ATAPI = 1 << 24;          // Device is ATAPI
        const APSTE = 1 << 23;          // Automatic Partial to Slumber Transitions Enabled
        const FBSCP = 1 << 22;          // FIS-based Switching Capable Port
        const ESP = 1 << 21;            // External SATA Port
        const CPD = 1 << 20;            // Cold Presence Detection
        const MPSP = 1 << 19;           // Mechanical Presence Switch Attached to Port
        const HPCP = 1 << 18;           // Hot Plug Capable Port
        const PMA = 1 << 17;            // Port Multiplier Attached
        const CPS = 1 << 16;            // Cold Presence State
        const CR = 1 << 15;             // Command List Running
        const FR = 1 << 14;             // FIS Receive Running
        const MPSS = 1 << 13;           // Mechanical Presence Switch State
        const CCS_MASK = 0x1F << 8;     // Current Command Slot
        const FRE = 1 << 4;             // FIS Receive Enable
        const CLO = 1 << 3;             // Command List Override
        const POD = 1 << 2;             // Power On Device
        const SUD = 1 << 1;             // Spin-Up Device
        const ST = 1 << 0;              // Start
    }
}

bitflags! {
    /// Port Interrupt Status (PxIS) register flags
    pub struct PortInterruptStatus: u32 {
        const CPDS = 1 << 31;           // Cold Port Detect Status
        const TFES = 1 << 30;           // Task File Error Status
        const HBFS = 1 << 29;           // Host Bus Fatal Error Status
        const HBDS = 1 << 28;           // Host Bus Data Error Status
        const IFS = 1 << 27;            // Interface Fatal Error Status
        const INFS = 1 << 26;           // Interface Non-fatal Error Status
        const OFS = 1 << 24;            // Overflow Status
        const IPMS = 1 << 23;           // Incorrect Port Multiplier Status
        const PRCS = 1 << 22;           // PhyRdy Change Status
        const DMPS = 1 << 7;            // Device Mechanical Presence Status
        const PCS = 1 << 6;             // Port Connect Change Status
        const DPS = 1 << 5;             // Descriptor Processed
        const UFS = 1 << 4;             // Unknown FIS Interrupt
        const SDBS = 1 << 3;            // Set Device Bits Interrupt
        const DSS = 1 << 2;             // DMA Setup FIS Interrupt
        const PSS = 1 << 1;             // PIO Setup FIS Interrupt
        const DHRS = 1 << 0;            // Device to Host Register FIS Interrupt
    }
}

bitflags! {
    /// Serial ATA Status (PxSSTS) register flags
    pub struct SerialATAStatus: u32 {
        const IPM_MASK = 0xF << 8;      // Interface Power Management
        const IPM_ACTIVE = 0x1 << 8;    // Interface in active state
        const IPM_PARTIAL = 0x2 << 8;   // Interface in partial power management state
        const IPM_SLUMBER = 0x6 << 8;   // Interface in slumber power management state
        const IPM_DEVSLEEP = 0x8 << 8;  // Interface in DevSleep power management state
        const SPD_MASK = 0xF << 4;      // Current Interface Speed
        const SPD_GEN1 = 0x1 << 4;      // Generation 1 communication rate negotiated
        const SPD_GEN2 = 0x2 << 4;      // Generation 2 communication rate negotiated
        const SPD_GEN3 = 0x3 << 4;      // Generation 3 communication rate negotiated
        const DET_MASK = 0xF;           // Device Detection
        const DET_NO_DEVICE = 0x0;      // No device detected and Phy communication not established
        const DET_PRESENT = 0x1;        // Device presence detected but Phy communication not established
        const DET_ESTABLISHED = 0x3;    // Device presence detected and Phy communication established
        const DET_OFFLINE = 0x4;        // Phy in offline mode
    }
}

/// AHCI Command List Entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AhciCommandListEntry {
    pub flags_prdtl: u32,       // Command FIS length and PRDT length
    pub prdbc: u32,             // Physical Region Descriptor Byte Count
    pub ctba: u64,              // Command Table Base Address
    pub reserved: [u32; 4],     // Reserved
}

/// AHCI Physical Region Descriptor Table Entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AhciPRDTEntry {
    pub dba: u64,               // Data Base Address
    pub reserved: u32,          // Reserved
    pub flags_dbc: u32,         // Interrupt on completion and Data Byte Count
}

/// AHCI Command Table
#[repr(C)]
pub struct AhciCommandTable {
    pub cfis: [u8; 64],         // Command FIS
    pub acmd: [u8; 16],         // ATAPI Command
    pub reserved: [u8; 48],     // Reserved
    pub prdt: [AhciPRDTEntry; 65535], // Physical Region Descriptor Table
}

/// SATA FIS Types
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum FISType {
    RegH2D = 0x27,              // Register FIS - host to device
    RegD2H = 0x34,              // Register FIS - device to host
    DMAActivate = 0x39,         // DMA activate FIS - device to host
    DMASetup = 0x41,            // DMA setup FIS - bidirectional
    Data = 0x46,                // Data FIS - bidirectional
    BIST = 0x58,                // BIST activate FIS - bidirectional
    PIOSetup = 0x5F,            // PIO setup FIS - device to host
    DevBits = 0xA1,             // Set device bits FIS - device to host
}

/// ATA Commands
pub mod ata_commands {
    pub const READ_SECTORS: u8 = 0x20;
    pub const READ_SECTORS_EXT: u8 = 0x24;
    pub const WRITE_SECTORS: u8 = 0x30;
    pub const WRITE_SECTORS_EXT: u8 = 0x34;
    pub const IDENTIFY_DEVICE: u8 = 0xEC;
    pub const SET_FEATURES: u8 = 0xEF;
    pub const READ_DMA: u8 = 0xC8;
    pub const READ_DMA_EXT: u8 = 0x25;
    pub const WRITE_DMA: u8 = 0xCA;
    pub const WRITE_DMA_EXT: u8 = 0x35;
}