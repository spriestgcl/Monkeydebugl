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

        // For cross-page buffers, ensure the involved user pages are mapped; do NOT require physical contiguity
        if start_page != end_page {
            let task = current_task();
            if let Ok(user_task) = task.downcast_arc::<crate::tasks::UserTask>() {
                // Collect unmapped pages
                let mut unmapped_pages = Vec::new();
                for page_num in start_page..=end_page {
                    let page_addr = VirtAddr::new(page_num * PageTable::PAGE_SIZE);
                    if user_task.page_table.translate(page_addr).is_none() {
                        unmapped_pages.push(page_addr);
                    }
                }
                // Allocate missing pages in batch if needed
                if !unmapped_pages.is_empty() {
                    if !self.batch_allocate_missing_pages(user_task.clone(), &unmapped_pages) {
                        log::error!("Failed to allocate missing user pages for cross-page buffer");
                        return &mut [];
                    }
                }
            } else {
                // If we cannot get user task context, fall back to creating the slice; page faults will handle mapping
                log::warn!("UserRef: no user task context for page validation; proceeding without pre-validation");
            }
        }

        // All pages are mapped (or single page); create the slice over virtual memory (physical contiguity not required)
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
