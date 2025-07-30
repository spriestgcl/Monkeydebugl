use core::{
    fmt::{Debug, Display},
    marker::PhantomData,
};
use alloc::vec::Vec;

use polyhal::{VirtAddr, PageTable};
use executor::current_task;

#[derive(Clone, Copy)]
pub struct UserRef<T> {
    addr: VirtAddr,
    r#type: PhantomData<T>,
}

impl<T> From<usize> for UserRef<T> {
    fn from(value: usize) -> Self {
        Self {
            addr: value.into(),
            r#type: PhantomData,
        }
    }
}

impl<T> From<VirtAddr> for UserRef<T> {
    fn from(value: VirtAddr) -> Self {
        Self {
            addr: value,
            r#type: PhantomData,
        }
    }
}

impl<T> Into<usize> for UserRef<T> {
    fn into(self) -> usize {
        self.addr.raw()
    }
}

impl<T> UserRef<T> {
    #[inline]
    pub fn addr(&self) -> usize {
        self.addr.raw()
    }
    #[inline]
    pub fn get_ref(&self) -> &'static T {
        self.addr.get_ref::<T>()
    }

    #[inline]
    pub fn get_mut(&self) -> &'static mut T {
        self.addr.get_mut_ref::<T>()
    }

    pub fn slice_mut_with_len(&self, len: usize) -> &'static mut [T] {
        if len == 0 {
            return &mut [];
        }

        let start_addr = self.addr.raw();
        let element_size = core::mem::size_of::<T>();
        let total_bytes = len * element_size;
        let end_addr = start_addr + total_bytes;

        // Check if the buffer spans multiple pages
        let start_page = start_addr / PageTable::PAGE_SIZE;
        let end_page = (end_addr - 1) / PageTable::PAGE_SIZE;

        // If accessing multiple pages, we need to ensure all pages are mapped and contiguous
        if start_page != end_page {
            log::error!("Cross-page buffer access detected: start={:#x}, end={:#x}, pages={}-{}, len={}, element_size={}",
                       start_addr, end_addr, start_page, end_page, len, element_size);

            // Get current task to access its page table
            let task = current_task();
            if let Ok(user_task) = task.downcast_arc::<crate::tasks::UserTask>() {
                // First pass: identify unmapped pages and try to allocate them
                let mut unmapped_pages = Vec::new();

                for page_num in start_page..=end_page {
                    let page_addr = VirtAddr::new(page_num * PageTable::PAGE_SIZE);
                    if user_task.page_table.translate(page_addr).is_none() {
                        unmapped_pages.push(page_addr);
                    }
                }

                // If we found unmapped pages, try to allocate them in batch
                if !unmapped_pages.is_empty() {
                    log::warn!("Found {} unmapped pages in cross-page access, attempting batch allocation", unmapped_pages.len());

                    // Try to allocate missing pages using the batch allocation function
                    if !self.batch_allocate_missing_pages(user_task.clone(), &unmapped_pages) {
                        log::error!("Batch allocation failed for unmapped pages");
                        return &mut [];
                    }

                    log::info!("Successfully allocated {} missing pages", unmapped_pages.len());
                }

                // Second pass: verify all pages are now mapped and check contiguity
                let mut prev_phys_end = None;

                for page_num in start_page..=end_page {
                    let page_addr = VirtAddr::new(page_num * PageTable::PAGE_SIZE);

                    // Check if this page is mapped
                    if let Some((phys_addr, _flags)) = user_task.page_table.translate(page_addr) {
                        log::debug!("Page at {:#x} is mapped to physical {:#x}", page_addr.raw(), phys_addr.raw());

                        // Check if pages are physically contiguous
                        if let Some(expected_start) = prev_phys_end {
                            if phys_addr.raw() != expected_start {
                                log::error!("CRITICAL: Non-contiguous physical pages detected! Expected {:#x}, got {:#x}",
                                           expected_start, phys_addr.raw());

                                // Output all page mappings from start to end for debugging
                                log::error!("Dumping all page mappings in range:");
                                for debug_page_num in start_page..=end_page {
                                    let debug_page_addr = VirtAddr::new(debug_page_num * PageTable::PAGE_SIZE);
                                    if let Some((debug_phys_addr, _)) = user_task.page_table.translate(debug_page_addr) {
                                        log::error!("Page at {:#x} is mapped to physical {:#x}", debug_page_addr.raw(), debug_phys_addr.raw());
                                    } else {
                                        log::error!("Page at {:#x} is NOT mapped", debug_page_addr.raw());
                                    }
                                }

                                // Pages are not physically contiguous, cannot safely create a single slice
                                return &mut [];
                            }
                        }
                        prev_phys_end = Some(phys_addr.raw() + PageTable::PAGE_SIZE);
                    } else {
                        log::error!("CRITICAL: Page at {:#x} is still unmapped after batch allocation", page_addr.raw());
                        return &mut [];
                    }
                }

                log::error!("All pages are mapped and physically contiguous, proceeding with slice creation");
            } else {
                log::error!("Failed to get current user task for page validation");
                // Without task context, we cannot validate pages safely
                return &mut [];
            }
        }

        // All pages are mapped and contiguous (or single page), safe to create the slice
        self.addr.slice_mut_with_len(len)
    }

    /// Batch allocate missing pages for cross-page access
    /// Returns true if all pages were successfully allocated, false otherwise
    fn batch_allocate_missing_pages(&self, user_task: alloc::sync::Arc<crate::tasks::UserTask>, unmapped_pages: &[VirtAddr]) -> bool {
        use crate::user::batch_allocate_pages_for_area;

        // Find the memory area that contains these pages
        let pcb = user_task.pcb.lock();
        let first_page_addr = unmapped_pages[0].raw();

        if let Some(area_index) = pcb.memset.iter().position(|area| area.contains(first_page_addr)) {
            drop(pcb); // Release the lock before calling the allocation function

            // Call the batch allocation function
            batch_allocate_pages_for_area(user_task, area_index, unmapped_pages)
        } else {
            log::error!("Could not find memory area containing page {:#x}", first_page_addr);
            false
        }
    }

    #[inline]
    pub fn slice_until_valid(&self, is_valid: fn(T) -> bool) -> &'static mut [T] {
        if self.addr.raw() == 0 {
            return &mut [];
        }
        self.addr.slice_until(is_valid)
    }

    #[inline]
    pub fn get_cstr(&self) -> Result<&str, core::str::Utf8Error> {
        self.addr.get_cstr().to_str()
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.addr.raw() != 0
    }
}

impl<T> Display for UserRef<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{}({:#x})",
            core::any::type_name::<T>(),
            self.addr.raw()
        ))
    }
}

impl<T> Debug for UserRef<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "{}({:#x})",
            core::any::type_name::<T>(),
            self.addr.raw()
        ))
    }
}
