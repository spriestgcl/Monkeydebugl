use super::{MappingFlags, PageTable, PTE, TLB};
use crate::{PhysAddr, VirtAddr};
use log::info;
use loongArch64::register::pgdl;

impl PTE {
    #[inline]
    pub const fn is_valid(&self) -> bool {
        // 更严格的有效性检查
        self.flags().contains(PTEFlags::V) && self.0 > 0 && self.0 != usize::MAX
    }

    #[inline]
    pub const fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits_truncate(self.0)
    }

    #[inline]
    pub fn address(&self) -> PhysAddr {
        let addr = (self.0) & 0xffff_ffff_f000;

        // 添加地址有效性检查（参考RISC-V最新版本）
        if self.0 == 0 {
            return PhysAddr::new(0);
        }

        // 检查PTE值是否在合理范围内（LoongArch64适配）
        if self.0 > 0xFFFFF_FFFFF_FFF {
            // LoongArch64的地址范围
            log::warn!(
                "[LoongArch64] Suspicious PTE value: {:#x}, calculated addr: {:#x}",
                self.0,
                addr
            );
        }

        PhysAddr::new(addr)
    }

    #[inline]
    pub fn is_table(&self) -> bool {
        // 参考RISC-V最新版本的安全检查
        // 检查是否为异常值（全1或全0）
        if self.0 == 0 || self.0 == usize::MAX {
            return false;
        }

        // 检查PTE值是否在合理范围内（LoongArch64适配）
        if self.0 > 0xFFFFF_FFFFF_FFF {
            log::warn!(
                "[LoongArch64] PTE value out of range in is_table(): {:#x}",
                self.0
            );
            return false;
        }

        // LoongArch64的页表项有效性检查
        self.flags().contains(PTEFlags::V)
    }

    #[inline]
    pub(crate) fn new_table(paddr: PhysAddr) -> Self {
        Self(paddr.raw() | PTEFlags::V.bits())
    }

    #[inline]
    pub(crate) fn new_page(paddr: PhysAddr, flags: PTEFlags) -> Self {
        Self(paddr.raw() | flags.bits())
    }
}

impl From<MappingFlags> for PTEFlags {
    fn from(value: MappingFlags) -> Self {
        // 参考RISC-V最新版本的转换逻辑
        if value.is_empty() {
            Self::empty()
        } else {
            let mut flags = PTEFlags::V;

            // LoongArch64特定的标志位设置
            if value.contains(MappingFlags::W) {
                flags |= PTEFlags::W | PTEFlags::D;
            }

            // 如果需要用户权限
            if value.contains(MappingFlags::U) {
                flags |= PTEFlags::PLV_USER;
            }

            // LoongArch64默认设置P标志（页面存在）
            flags |= PTEFlags::P;

            flags
        }
    }
}

impl From<PTEFlags> for MappingFlags {
    fn from(val: PTEFlags) -> Self {
        let mut flags = MappingFlags::empty();
        if val.contains(PTEFlags::W) {
            flags |= MappingFlags::W;
        }

        if val.contains(PTEFlags::D) {
            flags |= MappingFlags::D;
        }

        // if !self.contains(PTEFlags::NX) {
        //     flags |= MappingFlags::X;
        // }

        if val.contains(PTEFlags::PLV_USER) {
            flags |= MappingFlags::U;
        }
        flags
    }
}

bitflags::bitflags! {
    /// Possible flags for a page table entry.
    pub struct PTEFlags: usize {
        /// Page Valid
        const V = bit!(0);
        /// Dirty, The page has been writed.
        const D = bit!(1);

        const PLV_USER = 0b11 << 2;

        const MAT_NOCACHE = 0b01 << 4;

        /// Designates a global mapping OR Whether the page is huge page.
        const GH = bit!(6);

        /// Page is existing.
        const P = bit!(7);
        /// Page is writeable.
        const W = bit!(8);
        /// Is a Global Page if using huge page(GH bit).
        const G = bit!(10);
        /// Page is not readable.
        const NR = bit!(11);
        /// Page is not executable.
        /// FIXME: Is it just for a huge page?
        /// Linux related url: https://github.com/torvalds/linux/blob/master/arch/loongarch/include/asm/pgtable-bits.h
        const NX = bit!(12);
        /// Whether the privilege Level is restricted. When RPLV is 0, the PTE
        /// can be accessed by any program with privilege Level highter than PLV.
        const RPLV = bit!(63);
    }
}

impl PageTable {
    /// The size of the page for this platform.
    pub const PAGE_SIZE: usize = 0x1000;
    pub const PAGE_LEVEL: usize = 3;
    pub const PTE_NUM_IN_PAGE: usize = 0x200;
    pub(crate) const GLOBAL_ROOT_PTE_RANGE: usize = 0x100;
    pub(crate) const VADDR_BITS: usize = 39;
    pub(crate) const USER_VADDR_END: usize = (1 << Self::VADDR_BITS) - 1;
    pub(crate) const KERNEL_VADDR_START: usize = !Self::USER_VADDR_END;

    #[inline]
    pub fn restore(&self) {
        // 采用RISC-V最新版本的安全三步策略
        info!(
            "[LoongArch64] PageTable restore started, root: {:#x}",
            self.0.raw()
        );

        // 第一步：先清空用户空间部分，避免release()处理异常数据
        let arr = Self::get_pte_list(self.0);
        arr[0..Self::GLOBAL_ROOT_PTE_RANGE].fill(PTE(0));

        // 第二步：现在安全地调用release()，用户空间已经是干净的
        self.release();

        // 第三步：复制内核页表并再次清理用户空间
        let kernel_arr = Self::get_pte_list(Self::current().0);
        let arr = Self::get_pte_list(self.0);
        arr[Self::GLOBAL_ROOT_PTE_RANGE..]
            .copy_from_slice(&kernel_arr[Self::GLOBAL_ROOT_PTE_RANGE..]);
        arr[0..Self::GLOBAL_ROOT_PTE_RANGE].fill(PTE(0));

        TLB::flush_all();
        info!("[LoongArch64] PageTable restore completed");
    }

    #[inline]
    pub fn current() -> Self {
        Self(PhysAddr::new(pgdl::read().base()))
    }

    #[inline]
    pub fn change(&self) {
        pgdl::set_base(self.0.raw());
        TLB::flush_all();
    }
}

/// TLB operations
impl TLB {
    /// flush the TLB entry by VirtualAddress
    /// just use it directly
    ///
    /// TLB::flush_vaddr(arg0); // arg0 is the virtual address(VirtAddr)
    #[inline]
    pub fn flush_vaddr(vaddr: VirtAddr) {
        unsafe {
            core::arch::asm!("dbar 0; invtlb 0x05, $r0, {reg}", reg = in(reg) vaddr.raw());
        }
    }

    /// flush all tlb entry
    ///
    /// how to use ?
    /// just
    /// TLB::flush_all();
    #[inline]
    pub fn flush_all() {
        unsafe {
            core::arch::asm!("dbar 0; invtlb 0x00, $r0, $r0");
        }
    }
}

pub fn boot_page_table() -> PageTable {
    // FIXME: This should return a valid page table.
    // ref solution: create a blank page table in boot stage.
    PageTable(PhysAddr::new(0))
}
