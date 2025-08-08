//! AHCI Device Implementation
//! 
//! Contains the main AHCI device and SATA block device implementations

use crate::registers::*;
use crate::platform;
use crate::{SATA_BLOCK_SIZE, AHCI_MAX_PORTS, ahci_debug};
use spin::Mutex;
use volatile::Volatile;
use core::ptr::{read_volatile, write_volatile};
use core::mem::{size_of, align_of};

/// AHCI Device representing the controller
pub struct AhciDevice {
    base_addr: usize,
    ghc: &'static mut AhciGenericHostControl,
    ports: [Option<&'static mut AhciPortRegs>; AHCI_MAX_PORTS],
    devices: [Option<AhciBlkDev>; AHCI_MAX_PORTS],
    capabilities: HostCapabilities,
    ports_implemented: u32,
}

/// AHCI Block Device representing a SATA drive
pub struct AhciBlkDev {
    port_num: usize,
    port_regs: &'static mut AhciPortRegs,
    capacity: u64,
    model_string: [u8; 40],
    serial_string: [u8; 20],
    supports_lba48: bool,
    command_list: *mut AhciCommandListEntry,
    received_fis: *mut u8,
    command_tables: [*mut AhciCommandTable; 32],
}

impl AhciDevice {
    /// Create a new AHCI device
    pub fn new(base_addr: usize) -> Result<Self, &'static str> {
        ahci_debug!("Creating AHCI device at base address 0x{:x}", base_addr);
        
        // Map the AHCI controller registers
        let ghc = unsafe { &mut *(base_addr as *mut AhciGenericHostControl) };
        
        let mut device = AhciDevice {
            base_addr,
            ghc,
            ports: [None; AHCI_MAX_PORTS],
            devices: [None; AHCI_MAX_PORTS],
            capabilities: HostCapabilities::empty(),
            ports_implemented: 0,
        };
        
        Ok(device)
    }
    
    /// Initialize AHCI controller
    pub fn init_controller(&mut self) -> Result<(), &'static str> {
        ahci_debug!("Initializing AHCI controller");
        
        // Read capabilities
        let cap = self.ghc.cap.read();
        self.capabilities = HostCapabilities::from_bits_truncate(cap);
        
        ahci_debug!("AHCI Capabilities: 0x{:08x}", cap);
        ahci_debug!("Number of ports: {}", (cap & 0x1F) + 1);
        ahci_debug!("Supports 64-bit addressing: {}", self.capabilities.contains(HostCapabilities::S64A));
        
        // Read ports implemented
        self.ports_implemented = self.ghc.pi.read();
        ahci_debug!("Ports implemented: 0x{:08x}", self.ports_implemented);
        
        // Enable AHCI mode
        let mut ghc_val = self.ghc.ghc.read();
        ghc_val |= GlobalHostControl::AE.bits();
        self.ghc.ghc.write(ghc_val);
        
        // Initialize port registers
        for port in 0..AHCI_MAX_PORTS {
            if (self.ports_implemented & (1 << port)) != 0 {
                let port_base = self.base_addr + 0x100 + port * 0x80;
                self.ports[port] = Some(unsafe { &mut *(port_base as *mut AhciPortRegs) });
                ahci_debug!("Initialized port {} registers at 0x{:x}", port, port_base);
            }
        }
        
        ahci_debug!("AHCI controller initialization completed");
        Ok(())
    }
    
    /// Scan a port for SATA devices
    pub fn scan_port(&mut self, port_num: usize) -> Result<Option<AhciBlkDev>, &'static str> {
        ahci_debug!("Scanning port {}", port_num);
        
        if port_num >= AHCI_MAX_PORTS || self.ports[port_num].is_none() {
            return Err("Invalid port number");
        }
        
        let port_regs = self.ports[port_num].as_mut().unwrap();
        
        // Check if device is present
        let ssts = port_regs.ssts.read();
        let det = ssts & SerialATAStatus::DET_MASK.bits();
        
        ahci_debug!("Port {} SSTS: 0x{:08x}, DET: 0x{:x}", port_num, ssts, det);
        
        if det != SerialATAStatus::DET_ESTABLISHED.bits() {
            ahci_debug!("No device detected on port {}", port_num);
            return Ok(None);
        }
        
        ahci_debug!("Device detected on port {}", port_num);
        
        // Check device signature
        let sig = port_regs.sig.read();
        ahci_debug!("Device signature: 0x{:08x}", sig);
        
        // Initialize port
        self.init_port(port_num)?;
        
        // Create SATA block device
        let mut sata_dev = AhciBlkDev::new(port_num, port_regs)?;
        
        // For now, set a dummy capacity to test the block device interface
        sata_dev.capacity = 2048; // 1MB in 512-byte sectors
        
        // Set dummy strings
        sata_dev.model_string[..12].copy_from_slice(b"Test SSD    ");
        sata_dev.serial_string[..8].copy_from_slice(b"12345678");
        
        ahci_debug!("Created SATA device with {} sectors", sata_dev.capacity);
        
        Ok(Some(sata_dev))
    }
    
    /// Initialize a port
    fn init_port(&mut self, port_num: usize) -> Result<(), &'static str> {
        ahci_debug!("Initializing port {}", port_num);
        
        let port_regs = self.ports[port_num].as_mut().unwrap();
        
        // Stop port
        let mut cmd = port_regs.cmd.read();
        cmd &= !(PortCommand::ST.bits() | PortCommand::FRE.bits());
        port_regs.cmd.write(cmd);
        
        // Wait for port to stop
        let mut timeout = 500000;
        while (port_regs.cmd.read() & (PortCommand::CR.bits() | PortCommand::FR.bits())) != 0 && timeout > 0 {
            platform::ahci_platform_delay_us(1);
            timeout -= 1;
        }
        
        if timeout == 0 {
            return Err("Port stop timeout");
        }
        
        // Allocate command list (1KB aligned)
        let cl_size = 32 * size_of::<AhciCommandListEntry>();
        let cl_ptr = platform::ahci_platform_alloc_coherent(cl_size, 1024)?;
        
        // Allocate received FIS (256 bytes aligned)
        let rfis_size = 256;
        let rfis_ptr = platform::ahci_platform_alloc_coherent(rfis_size, 256)?;
        
        // Set command list and FIS base addresses
        let cl_phys = platform::ahci_platform_virt_to_phys(cl_ptr as usize);
        let rfis_phys = platform::ahci_platform_virt_to_phys(rfis_ptr as usize);
        
        port_regs.clb.write(cl_phys as u64);
        port_regs.fb.write(rfis_phys as u64);
        
        // Clear interrupt status
        port_regs.is.write(0xFFFFFFFF);
        
        // Enable FIS receive
        cmd = port_regs.cmd.read();
        cmd |= PortCommand::FRE.bits();
        port_regs.cmd.write(cmd);
        
        // Power on device
        cmd |= PortCommand::POD.bits() | PortCommand::SUD.bits();
        port_regs.cmd.write(cmd);
        
        // Wait for device to be ready
        platform::ahci_platform_delay_us(10000); // 10ms
        
        // Start port
        cmd |= PortCommand::ST.bits();
        port_regs.cmd.write(cmd);
        
        ahci_debug!("Port {} initialized successfully", port_num);
        Ok(())
    }
    
    /// Add a device to the controller
    pub fn add_device(&mut self, port_num: usize, device: AhciBlkDev) {
        if port_num < AHCI_MAX_PORTS {
            self.devices[port_num] = Some(device);
        }
    }
    
    /// Get device by port number
    pub fn get_device(&self, port_num: usize) -> Option<&AhciBlkDev> {
        if port_num < AHCI_MAX_PORTS {
            self.devices[port_num].as_ref()
        } else {
            None
        }
    }
    
    /// Get mutable device by port number
    pub fn get_device_mut(&mut self, port_num: usize) -> Option<&mut AhciBlkDev> {
        if port_num < AHCI_MAX_PORTS {
            self.devices[port_num].as_mut()
        } else {
            None
        }
    }
}

impl AhciBlkDev {
    /// Create a new SATA block device
    pub fn new(port_num: usize, port_regs: &'static mut AhciPortRegs) -> Result<Self, &'static str> {
        ahci_debug!("Creating SATA block device for port {}", port_num);
        
        // Allocate command tables for 32 command slots
        let mut command_tables = [core::ptr::null_mut(); 32];
        
        for i in 0..32 {
            let ct_size = size_of::<AhciCommandTable>();
            let ct_ptr = platform::ahci_platform_alloc_coherent(ct_size, 128)?;
            command_tables[i] = ct_ptr as *mut AhciCommandTable;
        }
        
        // Get command list pointer
        let cl_phys = port_regs.clb.read();
        let cl_ptr = platform::ahci_platform_phys_to_virt(cl_phys as usize);
        
        // Get received FIS pointer  
        let rfis_phys = port_regs.fb.read();
        let rfis_ptr = platform::ahci_platform_phys_to_virt(rfis_phys as usize);
        
        Ok(AhciBlkDev {
            port_num,
            port_regs,
            capacity: 0,
            model_string: [0; 40],
            serial_string: [0; 20],
            supports_lba48: false,
            command_list: cl_ptr as *mut AhciCommandListEntry,
            received_fis: rfis_ptr as *mut u8,
            command_tables,
        })
    }
    
    /// Read sectors from the device
    pub fn read_sectors(&mut self, start_lba: u64, sector_count: u32, buffer: &mut [u8]) -> Result<(), &'static str> {
        ahci_debug!("Reading {} sectors from LBA {}", sector_count, start_lba);
        
        if buffer.len() < (sector_count as usize * SATA_BLOCK_SIZE) {
            return Err("Buffer too small");
        }
        
        // For testing, just fill with dummy data
        for i in 0..buffer.len() {
            buffer[i] = (i % 256) as u8;
        }
        
        ahci_debug!("Read operation completed (dummy data)");
        Ok(())
    }
    
    /// Write sectors to the device
    pub fn write_sectors(&mut self, start_lba: u64, sector_count: u32, buffer: &[u8]) -> Result<(), &'static str> {
        ahci_debug!("Writing {} sectors to LBA {}", sector_count, start_lba);
        
        if buffer.len() < (sector_count as usize * SATA_BLOCK_SIZE) {
            return Err("Buffer too small");
        }
        
        // For testing, just log the operation
        ahci_debug!("Write operation completed (dummy)");
        Ok(())
    }
    
    /// Get device model string
    pub fn model(&self) -> &str {
        let model_str = core::str::from_utf8(&self.model_string).unwrap_or("Unknown");
        model_str.trim_end_matches('\0').trim()
    }
    
    /// Get device serial string
    pub fn serial(&self) -> &str {
        let serial_str = core::str::from_utf8(&self.serial_string).unwrap_or("Unknown");
        serial_str.trim_end_matches('\0').trim()
    }
    
    /// Get device capacity in sectors
    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    
    /// Check if device supports LBA48
    pub fn supports_lba48(&self) -> bool {
        self.supports_lba48
    }
}

unsafe impl Send for AhciDevice {}
unsafe impl Sync for AhciDevice {}
unsafe impl Send for AhciBlkDev {}
unsafe impl Sync for AhciBlkDev {}