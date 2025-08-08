//! Platform-specific functions for AHCI driver
//! 
//! These functions need to be implemented by the target platform

/// Initialize platform-specific AHCI resources
pub fn ahci_platform_init() -> Result<(), &'static str> {
    // TODO: Initialize platform-specific resources
    Ok(())
}

/// Delay for specified microseconds
pub fn ahci_platform_delay_us(us: u32) {
    // Simple busy wait implementation
    for _ in 0..(us * 100) {
        unsafe { core::arch::asm!("nop") };
    }
}

/// Allocate coherent DMA memory
pub fn ahci_platform_alloc_coherent(size: usize, _alignment: usize) -> Result<*mut u8, &'static str> {
    use devices::{frame_alloc_much, PAGE_SIZE};
    
    let pages_needed = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    
    if let Some(frames) = frame_alloc_much(pages_needed) {
        let ptr = frames.as_ptr() as *mut u8;
        // Clear the allocated memory
        unsafe {
            core::ptr::write_bytes(ptr, 0, size);
        }
        
        // We need to leak the FrameTracker to keep the memory allocated
        core::mem::forget(frames);
        
        Ok(ptr)
    } else {
        Err("Failed to allocate DMA memory")
    }
}

/// Convert virtual address to physical address
pub fn ahci_platform_virt_to_phys(virt_addr: usize) -> usize {
    // For LoongArch64 with direct mapping
    if virt_addr >= 0x9000000000000000 {
        virt_addr - 0x9000000000000000
    } else {
        virt_addr // Already physical or identity mapped
    }
}

/// Convert physical address to virtual address
pub fn ahci_platform_phys_to_virt(phys_addr: usize) -> usize {
    // For LoongArch64 with direct mapping
    if phys_addr < 0x9000000000000000 {
        phys_addr + 0x9000000000000000
    } else {
        phys_addr // Already virtual
    }
}