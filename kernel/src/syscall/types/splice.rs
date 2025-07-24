use bitflags::bitflags;

bitflags! {
    /// Flags for splice system call
    #[derive(Debug, Clone, Copy)]
    pub struct SpliceFlags: u32 {
        /// Attempt to move pages instead of copying
        const SPLICE_F_MOVE = 1;
        /// Do not block on I/O
        const SPLICE_F_NONBLOCK = 2;
        /// More data will be coming in a subsequent splice
        const SPLICE_F_MORE = 4;
        /// Unused for splice(); see vmsplice(2)
        const SPLICE_F_GIFT = 8;
    }
}

impl Default for SpliceFlags {
    fn default() -> Self {
        SpliceFlags::empty()
    }
}
