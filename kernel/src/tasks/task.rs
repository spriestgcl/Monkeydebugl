// use crate::tasks::memset;
use super::{
    filetable::{rlimits_new, FileTable},
    memset::{MemSet, MemType},
    shm::MapedSharedMemory,
    SignalList,
};
use crate::{
    syscall::types::{
        fd::AT_CWD,
        time::{ProcessTimer, TMS},
    },
    tasks::{
        futex_wake,
        memset::{MapTrack, MemArea},
    },
};
use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{cmp::max, mem::size_of};
use devices::PAGE_SIZE;
use executor::{release_task, task::TaskType, task_id_alloc, AsyncTask, TaskId};
use fs::{file::File, pathbuf::PathBuf, INodeInterface};
use log::{debug, info, warn};
use polyhal::{va, MappingFlags, MappingSize, PageTableWrapper, PhysAddr, VirtAddr};
use polyhal_trap::trapframe::{TrapFrame, TrapFrameArgs};
use runtime::frame::{alignup, frame_alloc_much};
use signal::{SigAction, SigProcMask, SignalFlags, REAL_TIME_SIGNAL_NUM};
use sync::{Mutex, MutexGuard, RwLock};
use syscalls::Errno;
use vfscore::{OpenFlags, VfsResult};

pub type FutexTable = BTreeMap<usize, Vec<usize>>;

pub struct ProcessControlBlock {
    pub memset: MemSet,
    pub fd_table: FileTable,
    pub curr_dir: Arc<File>,
    pub heap: usize,
    pub entry: usize,
    pub children: Vec<Arc<UserTask>>,
    pub tms: TMS,
    pub rlimits: Vec<usize>,
    pub sigaction: [SigAction; 65],
    pub futex_table: Arc<Mutex<FutexTable>>,
    pub shms: Vec<MapedSharedMemory>,
    pub timer: [ProcessTimer; 3],
    pub threads: Vec<Weak<UserTask>>,
    pub exit_code: Option<usize>,
    pub exit_signal: Option<usize>, // 如果进程被信号杀死，记录信号号
    pub core_dumped: bool, // 如果进程产生了core dump
    pub umask: usize,
}

pub struct ThreadControlBlock {
    pub cx: TrapFrame,
    pub sigmask: SigProcMask,
    pub clear_child_tid: usize,
    pub set_child_tid: usize,
    pub signal: SignalList,
    pub signal_queue: [usize; REAL_TIME_SIGNAL_NUM], // a queue for real time signals
    pub exit_signal: u8,
    pub thread_exit_code: Option<u32>,
    pub robust_list_head: usize,    // 添加 robust list 头指针
    pub robust_list_len: usize,     // 添加 robust list 长度
}

#[allow(dead_code)]
pub struct UserTask {
    pub task_id: TaskId,
    pub process_id: TaskId,
    pub page_table: Arc<PageTableWrapper>,
    pub pcb: Arc<Mutex<ProcessControlBlock>>,
    pub parent: RwLock<Weak<UserTask>>,
    pub tcb: RwLock<ThreadControlBlock>,
}

impl UserTask {
    pub fn release(&self) {
        // Ensure that the task was exited successfully.
        assert!(self.exit_code().is_some() || self.tcb.read().thread_exit_code.is_some());
        release_task(self.task_id);
    }
}

impl UserTask {
    pub fn new(parent: Weak<UserTask>, work_dir: PathBuf) -> Arc<Self> {
        let task_id = task_id_alloc();
        // initialize memset
        let memset = MemSet::new(vec![]);

        let curr_dir = File::open(work_dir, OpenFlags::O_DIRECTORY)
            .map(Arc::new)
            .expect("dont' have the home dir");

        let inner = ProcessControlBlock {
            memset,
            fd_table: FileTable::new(),
            curr_dir,
            heap: 0,
            children: Vec::new(),
            entry: 0,
            tms: Default::default(),
            rlimits: rlimits_new(),
            sigaction: [SigAction::new(); 65],
            futex_table: Arc::new(Mutex::new(BTreeMap::new())),
            shms: vec![],
            timer: [Default::default(); 3],
            exit_code: None,
            exit_signal: None,
            core_dumped: false,
            threads: Vec::new(),
            umask: 0o022, // 默认umask值
        };

        let tcb = RwLock::new(ThreadControlBlock {
            cx: TrapFrame::new(),
            sigmask: SigProcMask::new(),
            clear_child_tid: 0,
            set_child_tid: 0,
            signal: SignalList::new(),
            signal_queue: [0; REAL_TIME_SIGNAL_NUM],
            exit_signal: 0,
            thread_exit_code: Option::None,
            robust_list_head: 0,            // 初始化为 0
            robust_list_len: 0,             // 初始化为 0
        });

        let task = Arc::new(Self {
            page_table: Arc::new(PageTableWrapper::alloc()),
            task_id,
            process_id: task_id,
            parent: RwLock::new(parent),
            pcb: Arc::new(Mutex::new(inner)),
            tcb,
        });
        task.pcb.lock().threads.push(Arc::downgrade(&task));
        task
    }

    pub fn inner_map<T>(&self, mut f: impl FnMut(&mut MutexGuard<ProcessControlBlock>) -> T) -> T {
        f(&mut self.pcb.lock())
    }

    #[inline]
    pub fn map(&self, paddr: PhysAddr, vaddr: VirtAddr, flags: MappingFlags) {
        assert_eq!(paddr.raw() % PAGE_SIZE, 0);
        assert_eq!(vaddr.raw() % PAGE_SIZE, 0);
        // self.page_table.map(ppn, vpn, flags, 3);
        self.page_table
            .map_page(vaddr, paddr, flags, MappingSize::Page4KB);
    }

    pub fn frame_alloc(&self, vaddr: VirtAddr, mtype: MemType, count: usize) -> Option<PhysAddr> {
        // 根据内存类型选择合适的权限
        let mapping_flags = match mtype {
            MemType::Stack => MappingFlags::URWX,      // 栈需要读写执行权限
            MemType::CodeSection => MappingFlags::URWX, // 代码段默认读写执行（向后兼容，现已使用Mmap）
            MemType::Mmap => MappingFlags::URWX,       // mmap区域默认读写执行
            MemType::Shared => MappingFlags::URWX,     // 共享内存读写执行
            MemType::ShareFile => MappingFlags::URW,   // 共享文件读写
        };
        
        // 改进的内存分配策略：严格保证连续性，避免分段分配破坏内存连续性
        // 在loongarch架构下暂时禁用严格分配策略，避免兼容性问题
        #[cfg(not(target_arch = "loongarch64"))]
        if count > 1 {
            // 记录分配请求的统计信息
            if count > 16 {
                let free_pages = runtime::frame::get_free_pages();
                debug!("Large allocation request: {} pages, current free pages: {}", count, free_pages);
            }

            // 策略1: 尝试标准连续分配
            if let Some(trackers) = frame_alloc_much(count) {
                return self.create_memory_area_from_trackers(
                    trackers, vaddr, mtype, mapping_flags, count
                );
            }

            // 策略2: 对于大块分配（超过32页），再次尝试连续分配
            if count >= 32 {
                debug!("Large allocation request: {} pages, retrying continuous allocation", count);
                // 这里可以添加更复杂的大块分配策略，目前先重试一次
                if let Some(trackers) = frame_alloc_much(count) {
                    return self.create_memory_area_from_trackers(
                        trackers, vaddr, mtype, mapping_flags, count
                    );
                }
            }

            // 对于关键的内存类型（如Mmap用于堆），严格要求连续性
            // 但对于单页分配（如TLS），允许fallback到其他分配策略
            if mtype == MemType::Mmap && count > 1 {
                warn!("Failed to allocate {} continuous pages for heap (MemType::Mmap), refusing segmented allocation to maintain heap integrity", count);
                return None; // 拒绝分段分配，保证堆的连续性
            }

            // 对于其他类型，在严格模式下也拒绝分段分配
            if count >= 8 { // 对于8页以上的分配，严格要求连续性
                warn!("Failed to allocate {} continuous pages, refusing segmented allocation to maintain memory integrity", count);
                return None;
            }

            // 只有在小块分配且非关键内存类型时，才允许有限的分段分配
            warn!("Attempting limited segmented allocation for {} pages (non-critical memory type)", count);
            if let Some(ppn) = self.try_limited_segmented_allocation(vaddr, mtype, count, mapping_flags) {
                return Some(ppn);
            }

            // 所有策略都失败
            warn!("All allocation strategies failed for {} pages", count);
            return None;
        }
        
        self.map_frames(vaddr, mtype, count, None, 0, vaddr.raw(), count * PAGE_SIZE, mapping_flags)
    }

    /// 从trackers创建内存区域的辅助函数
    fn create_memory_area_from_trackers(
        &self,
        trackers: Vec<runtime::frame::FrameTracker>,
        vaddr: VirtAddr,
        mtype: MemType,
        mapping_flags: MappingFlags,
        count: usize,
    ) -> Option<PhysAddr> {
        let ppn = trackers[0].0;

        // 构建MapTrack
        let map_trackers: Vec<_> = trackers
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                let vaddr_i = match vaddr.raw() == 0 {
                    true => vaddr,
                    false => va!(vaddr.raw() + i * PAGE_SIZE),
                };
                MapTrack {
                    vaddr: vaddr_i,
                    tracker: Arc::new(x),
                    rwx: 0,
                }
            })
            .collect();

        // 映射到页表
        if vaddr.raw() != 0 {
            map_trackers
                .iter()
                .filter(|x| x.vaddr.raw() != 0)
                .for_each(|x| self.map(x.tracker.0, x.vaddr, mapping_flags));
        }

        // 添加到内存集合
        let mut inner = self.pcb.lock();
        inner.memset.push(MemArea {
            mtype,
            mtrackers: map_trackers,
            file: None,
            offset: 0,
            start: vaddr.raw(),
            len: count * PAGE_SIZE,
        });

        debug!("Successfully allocated {} continuous pages", count);
        Some(ppn)
    }



    /// 有限的分段分配（仅用于非关键场景，最多分成2段）
    fn try_limited_segmented_allocation(
        &self,
        vaddr: VirtAddr,
        mtype: MemType,
        count: usize,
        mapping_flags: MappingFlags,
    ) -> Option<PhysAddr> {
        // 只允许最多分成2段，避免过度碎片化
        let max_segments = 2;
        let segment_size = count / max_segments;
        let remainder = count % max_segments;

        let mut allocated_trackers = Vec::new();
        let mut current_vaddr = vaddr;

        // 尝试分配第一段（包含余数）
        if let Some(trackers) = frame_alloc_much(segment_size + remainder) {
            for (i, tracker) in trackers.into_iter().enumerate() {
                let vaddr_i = if current_vaddr.raw() == 0 {
                    current_vaddr
                } else {
                    va!(current_vaddr.raw() + i * PAGE_SIZE)
                };

                allocated_trackers.push(MapTrack {
                    vaddr: vaddr_i,
                    tracker: Arc::new(tracker),
                    rwx: 0,
                });
            }

            if current_vaddr.raw() != 0 {
                current_vaddr = va!(current_vaddr.raw() + (segment_size + remainder) * PAGE_SIZE);
            }

            // 尝试分配第二段
            if segment_size > 0 {
                if let Some(trackers) = frame_alloc_much(segment_size) {
                    for (i, tracker) in trackers.into_iter().enumerate() {
                        let vaddr_i = if current_vaddr.raw() == 0 {
                            current_vaddr
                        } else {
                            va!(current_vaddr.raw() + i * PAGE_SIZE)
                        };

                        allocated_trackers.push(MapTrack {
                            vaddr: vaddr_i,
                            tracker: Arc::new(tracker),
                            rwx: 0,
                        });
                    }
                } else {
                    warn!("Limited segmented allocation failed at second segment");
                    return None;
                }
            }

            let ppn = allocated_trackers[0].tracker.0;

            // 映射到页表
            if vaddr.raw() != 0 {
                allocated_trackers
                    .iter()
                    .filter(|x| x.vaddr.raw() != 0)
                    .for_each(|x| self.map(x.tracker.0, x.vaddr, mapping_flags));
            }

            // 添加到内存集合
            let mut inner = self.pcb.lock();
            inner.memset.push(MemArea {
                mtype,
                mtrackers: allocated_trackers,
                file: None,
                offset: 0,
                start: vaddr.raw(),
                len: count * PAGE_SIZE,
            });

            warn!("Limited segmented allocation successful: {} pages in {} segments", count, max_segments);
            return Some(ppn);
        }

        None
    }

    pub fn map_frames(
        &self,
        vaddr: VirtAddr,
        mtype: MemType,
        count: usize,
        file: Option<Arc<dyn INodeInterface>>,
        offset: usize,
        start: usize,
        len: usize,
        mapping_flags: MappingFlags,
    ) -> Option<PhysAddr> {
        assert!(count > 0, "can't alloc count = 0 in user_task frame_alloc");
        // alloc trackers and map vpn
        let trackers: Vec<_> = frame_alloc_much(count)?
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                let vaddr = match vaddr.raw() == 0 {
                    true => vaddr,
                    false => va!(vaddr.raw() + i * PAGE_SIZE),
                };
                MapTrack {
                    vaddr,
                    tracker: Arc::new(x),
                    rwx: 0,
                }
            })
            .collect();
        if vaddr.raw() != 0 {
            debug!(
                "map {:?} @ {:#x} size: {:#x} flags: {:?}",
                vaddr,
                trackers[0].tracker.raw(),
                count * PAGE_SIZE,
                mapping_flags
            );
            // map vpn to ppn
            trackers
                .clone()
                .iter()
                .filter(|x| x.vaddr.raw() != 0)
                .for_each(|x| self.map(x.tracker.0, x.vaddr, mapping_flags));
        }
        let mut inner = self.pcb.lock();
        let ppn = trackers[0].tracker.0;
        if mtype == MemType::Stack {
            let finded_area = inner.memset.iter_mut().find(|x| x.mtype == mtype);
            if let Some(area) = finded_area {
                area.mtrackers.extend(trackers);
            } else if mtype == MemType::Stack {
                inner.memset.push(MemArea {
                    mtype,
                    mtrackers: trackers.clone(),
                    file: None,
                    offset: 0,
                    start: 0x7000_0000,
                    len: 0x1000_0000,
                });
            }
        } else {
            inner.memset.push(MemArea {
                mtype,
                mtrackers: trackers.clone(),
                file,
                offset,
                start,
                len,
            });
        }
        drop(inner);

        Some(ppn)
    }

    pub fn force_cx_ref(&self) -> &'static mut TrapFrame {
        unsafe { &mut self.tcb.as_mut_ptr().as_mut().unwrap().cx }
    }

    pub fn sbrk(&self, addr: usize) -> usize {
        let curr_heap = self.pcb.lock().heap;
        let curr_page = curr_heap.div_ceil(PAGE_SIZE);
        let after_page = addr.div_ceil(PAGE_SIZE);

        if after_page > curr_page {
            let pages_needed = after_page - curr_page;
            let start_vaddr = va!(curr_page * PAGE_SIZE);

            // 改进的sbrk：批量分配内存，严格要求连续性
            // 在loongarch架构下使用传统的逐页分配方式
            #[cfg(target_arch = "loongarch64")]
            {
                // 逐页分配，避免连续分配问题
                for i in curr_page..after_page {
                    if self.frame_alloc(va!(i * PAGE_SIZE), MemType::Mmap, 1).is_none() {
                        warn!("sbrk: Failed to allocate page {} for heap expansion", i);
                        return curr_heap;
                    }
                }
            }

            #[cfg(not(target_arch = "loongarch64"))]
            {
                if pages_needed > 1 {
                    // 对于多页分配，使用改进的frame_alloc，严格要求连续分配
                    if self.frame_alloc(start_vaddr, MemType::Mmap, pages_needed).is_none() {
                        warn!("sbrk: Failed to allocate {} continuous pages for heap expansion", pages_needed);
                        return curr_heap; // 分配失败，返回当前堆大小，不扩展堆
                    }
                } else {
                    // 单页分配
                    if self.frame_alloc(start_vaddr, MemType::Mmap, 1).is_none() {
                        warn!("sbrk: Failed to allocate single page for heap expansion");
                        return curr_heap;
                    }
                }
            }

            debug!("sbrk: Successfully expanded heap from {:#x} to {:#x} ({} pages)",
                   curr_heap, addr, pages_needed);
        }

        self.pcb.lock().heap = addr;
        addr
    }

    pub fn heap(&self) -> usize {
        self.pcb.lock().heap
    }

    #[inline]
    pub fn thread_exit(&self, exit_code: usize) {
        warn!("Thread exit: task_id={}, process_id={}, exit_code={}, arch={}", 
            self.task_id, self.process_id, exit_code,
            if cfg!(target_arch = "loongarch64") { "loongarch64" } else { "other" });
            
        let mut tcb_writer = self.tcb.write();
        let uaddr = tcb_writer.clear_child_tid;
        if uaddr != 0 {
            debug!("write addr: {:#x}", uaddr);
            if let Some((addr, _)) = self.page_table.translate(VirtAddr::from(uaddr)) {
                unsafe {
                    addr.get_mut_ptr::<u32>().write(0);
                }
                futex_wake(self.pcb.lock().futex_table.clone(), uaddr, 1);
            } else {
                warn!("Failed to translate clear_child_tid address in thread_exit: {:#x}", uaddr);
            }
        }
        tcb_writer.thread_exit_code = Some(exit_code as u32);
        let exit_signal = tcb_writer.exit_signal;
        drop(tcb_writer);

        // 改进的线程退出逻辑 - 不应该直接清理进程资源
        let should_cleanup_process = {
            let mut pcb = self.pcb.lock();
            
            // 清理线程列表
            pcb.threads.retain(|weak_ref| {
                if let Some(thread) = weak_ref.upgrade() {
                    thread.task_id != self.task_id
                } else {
                    false
                }
            });
            
            let remaining_threads = pcb.threads.len();
            let is_main_thread = self.task_id == self.process_id;
            
            // 更保守的清理条件：只有主线程退出且没有其他活跃线程时才清理
            is_main_thread && remaining_threads == 0
        };

        if should_cleanup_process {
            warn!("Thread exit triggering process cleanup for process_id={}", self.process_id);
            let mut pcb = self.pcb.lock();
            pcb.memset.clear();
            pcb.fd_table.clear();
            pcb.children.clear();
            pcb.exit_code = Some(exit_code);

            if let Some(parent) = self.parent.read().upgrade() {
                if exit_signal != 0 {
                    parent
                        .tcb
                        .write()
                        .signal
                        .add_signal(SignalFlags::from_num(exit_signal as _));
                } else {
                    parent.tcb.write().signal.add_signal(SignalFlags::SIGCHLD);
                }
            }
        } else {
            warn!("Thread exit: task_id={} exited, but process continues", self.task_id);
        }
    }

    #[inline]
    pub fn exit_with_signal(&self, signal: usize) {
        info!("Process {} terminated by signal {}", self.task_id, signal);
        info!("LTP_SIGNAL_DEBUG: Process {} being terminated by signal {}", self.task_id, signal);
        
        // 检查信号是否应该产生core dump
        let should_dump_core = match signal {
            3 | 4 | 5 | 6 | 8 | 10 | 11 => true, // SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGFPE, SIGBUS, SIGSEGV
            _ => false,
        };
        
        // 特别关注abort()相关的测试
        if signal == 6 {  // SIGABRT/SIGIOT
            info!("LTP_ABORT_SIGNAL_DEBUG: abort() signal (SIGABRT/SIGIOT) received in process {}, will dump core: {}", 
                  self.task_id, should_dump_core);
        }
        
        // 标记进程是被信号杀死的
        {
            let mut pcb = self.pcb.lock();
            pcb.exit_signal = Some(signal);
            pcb.exit_code = Some(0); // 被信号杀死时，退出码设为0
            pcb.core_dumped = should_dump_core;
            
            if should_dump_core {
                info!("Process {} will dump core due to signal {}", self.task_id, signal);
                info!("LTP_CORE_DEBUG: Process {} will dump core due to signal {}", self.task_id, signal);
            }
        }
        
        // 执行正常的退出清理
        self.exit(0);
    }

    pub fn cow_fork(self: Arc<Self>) -> Arc<Self> {
        let parent_task: Arc<UserTask> = self.clone();
        let work_dir = parent_task.clone().pcb.lock().curr_dir.path_buf();
        let new_task = Self::new(Arc::downgrade(&parent_task), work_dir);
        let mut new_tcb_writer = new_task.tcb.write();

        // 克隆基本信息
        let mut new_pcb = new_task.pcb.lock();
        let mut pcb = self.pcb.lock();
        new_pcb.fd_table.0 = pcb.fd_table.0.clone();
        new_pcb.heap = pcb.heap;
        new_pcb.curr_dir = pcb.curr_dir.clone();
        new_pcb.shms = pcb.shms.clone();
        pcb.children.push(new_task.clone());
        drop(new_pcb);

        new_tcb_writer.cx = self.tcb.read().cx.clone();
        new_tcb_writer.cx[TrapFrameArgs::RET] = 0;
        drop(new_tcb_writer);

        pcb.memset.iter().for_each(|area| {
            let map_area = area.clone();

            // 禁用内存空隙填补逻辑 - 这会导致段错误
            // 这个逻辑创建了没有实际物理页面映射的内存区域
            // 当程序访问这些区域时会触发SIGSEGV
            #[cfg(never)]  // 完全禁用这个逻辑
            #[cfg(not(target_arch = "loongarch64"))]
            {
                // 检查是否存在内存空隙，如果存在则填补
                if let Some(next_area) = pcb
                    .memset
                    .iter()
                    .find(|next| next.start > area.start + area.len)
                {
                    let gap_start = area.start + area.len;
                    let gap_end = next_area.start;

                    if gap_end - gap_start > 0 && gap_end - gap_start < 0x10000 {
                        // 只处理小于64KB的空隙
                        warn!(
                            "Detected memory gap: {:#x} - {:#x}, filling it",
                            gap_start, gap_end
                        );

                        // 创建一个新的内存区域来填补空隙
                        let gap_area = MemArea {
                            mtype: MemType::Mmap,
                            mtrackers: Vec::new(),
                            file: area.file.clone(), // 使用相同的文件引用
                            offset: area.offset + area.len,
                            start: gap_start,
                            len: gap_end - gap_start,
                        };

                        new_task.pcb.lock().memset.push(gap_area);
                    }
                }
            }

            // 根据内存区域类型选择映射权限：
            //   - 对于 Shared / ShareFile ，必须带写权限，避免 fork 后写入触发 COW
            //   - 其它区域沿用只读 + 可执行，后续由写时复制处理
            let map_flags = match map_area.mtype {
                MemType::Shared | MemType::ShareFile => MappingFlags::URWX,
                _ => MappingFlags::URX,
            };

            map_area.mtrackers.iter().for_each(|mtracker| {
                new_task.map(mtracker.tracker.0, mtracker.vaddr, map_flags);
                self.map(mtracker.tracker.0, mtracker.vaddr, map_flags);
            });
            new_task.pcb.lock().memset.push(map_area);
        });

        // 处理共享内存
        pcb.shms.iter().for_each(|shm| {
            shm.mem
                .trackers
                .iter()
                .enumerate()
                .for_each(|(i, tracker)| {
                    new_task.map(
                        tracker.0,
                        va!(shm.start + i * PAGE_SIZE),
                        MappingFlags::URWX,
                    );
                });
        });

        drop(pcb);
        new_task
    }

    #[inline]
    pub fn thread_clone(self: Arc<Self>) -> Arc<Self> {
        let parent_tcb = self.tcb.read();
        let task_id = task_id_alloc();
        
        let tcb = RwLock::new(ThreadControlBlock {
            cx: parent_tcb.cx.clone(),
            sigmask: parent_tcb.sigmask.clone(),
            clear_child_tid: 0,
            set_child_tid: 0,
            signal: SignalList::new(),
            signal_queue: [0; REAL_TIME_SIGNAL_NUM],
            exit_signal: 0,
            thread_exit_code: Option::None,
            robust_list_head: 0,            // 新线程不继承父线程的 robust list
            robust_list_len: 0,             // 新线程不继承父线程的 robust list
        });

        tcb.write().cx[TrapFrameArgs::RET] = 0;
        drop(parent_tcb);

        let new_task = Arc::new(Self {
            page_table: self.page_table.clone(),
            task_id,
            process_id: self.process_id,  // 重要：保持相同的process_id
            parent: RwLock::new(self.parent.read().clone()),
            pcb: self.pcb.clone(),       // 重要：共享PCB
            tcb,
        });

        // 维护线程列表
        {
            let mut pcb = self.pcb.lock();
            // 清理死亡的线程引用
            pcb.threads.retain(|weak_ref| weak_ref.strong_count() > 0);
            // 添加新线程
            pcb.threads.push(Arc::downgrade(&new_task));
            
            warn!("Thread created: task_id={}, process_id={}, total_active_threads={}", 
                new_task.task_id, new_task.process_id, pcb.threads.len());
        }

        new_task
    }

    pub fn push_str(&self, str: &str) -> usize {
        self.push_arr(str.as_bytes())
    }

    pub fn push_arr(&self, buffer: &[u8]) -> usize {
        let mut tcb = self.tcb.write();

        const ULEN: usize = size_of::<usize>();
        let len = buffer.len();
        let sp = tcb.cx[TrapFrameArgs::SP] - alignup(len + 1, ULEN);
        VirtAddr::from(sp)
            .slice_mut_with_len(len)
            .copy_from_slice(buffer);
        tcb.cx[TrapFrameArgs::SP] = sp;
        sp
    }

    pub fn push_num(&self, num: usize) -> usize {
        let mut tcb = self.tcb.write();

        const ULEN: usize = size_of::<usize>();
        let sp = tcb.cx[TrapFrameArgs::SP] - ULEN;

        *VirtAddr::from(sp).get_mut_ref() = num;
        tcb.cx[TrapFrameArgs::SP] = sp;
        sp
    }

    pub fn get_last_free_addr(&self) -> VirtAddr {
        let map_last = self
            .pcb
            .lock()
            .memset
            .iter()
            .filter(|x| x.mtype != MemType::Stack)
            .fold(0, |acc, x| max(acc, x.start + x.len));
        let shm_last = self
            .pcb
            .lock()
            .shms
            .iter()
            .fold(0, |acc, v| max(v.start + v.size, acc));
        VirtAddr::new(max(map_last, shm_last))
    }

    pub fn get_fd(&self, index: usize) -> Option<Arc<File>> {
        let pcb = self.pcb.lock();
        match index >= pcb.rlimits[7] {
            true => None,
            false => pcb.fd_table.0[index].clone(),
        }
    }

    pub fn set_fd(&self, index: usize, value: Arc<File>) {
        let mut pcb = self.pcb.lock();
        match index >= pcb.rlimits[7] {
            true => {}
            false => pcb.fd_table.0[index] = Some(value),
        }
    }

    pub fn clear_fd(&self, index: usize) {
        let mut pcb = self.pcb.lock();
        match index >= pcb.fd_table.len() {
            true => {}
            false => pcb.fd_table.0[index] = None,
        }
    }

    pub fn alloc_fd(&self) -> Option<usize> {
        let mut pcb = self.pcb.lock();
        let index = pcb
            .fd_table
            .0
            .iter()
            .enumerate()
            .find(|(i, x)| x.is_none() && *i < pcb.rlimits[7])
            .map(|(i, _)| i);
        if index.is_none() && pcb.fd_table.0.len() < pcb.rlimits[7] {
            pcb.fd_table.0.push(None);
            Some(pcb.fd_table.0.len() - 1)
        } else {
            index
        }
    }

    pub fn fd_open(&self, fd: isize, filename: &str, flags: OpenFlags) -> VfsResult<File> {
        let path = self.fd_resolve(fd, filename)?;
        File::open(path, flags)
    }

    #[inline]
    pub fn fd_resolve(&self, fd: isize, filename: &str) -> VfsResult<PathBuf> {
        if filename.starts_with("/") {
            Ok(filename.into())
        } else {
            let parent = match fd {
                AT_CWD => self.pcb.lock().curr_dir.clone(),
                _ => self
                    .pcb
                    .lock()
                    .fd_table
                    .get(fd as usize)
                    .cloned()
                    .flatten()
                    .ok_or(Errno::EBADF)?,
            };
            Ok(parent.path_buf().join(filename))
        }
    }

    pub fn get_elf_segment_for_addr(
        &self,
        vaddr: VirtAddr,
    ) -> Option<(Arc<dyn INodeInterface>, usize, usize)> {
        let pcb = self.pcb.lock();

        warn!(
            "get_elf_segment_for_addr: task_id={}, process_id={}, searching for vaddr={:#x}",
            self.task_id,
            self.process_id,
            vaddr.raw()
        );
        warn!("Total memory areas: {}", pcb.memset.len());

        // 只在非 LoongArch 架构下进行内存空隙检查，避免干扰 LoongArch 系统调用
        #[cfg(not(target_arch = "loongarch64"))]
        {
            // 检查是否存在内存空隙
            for i in 0..pcb.memset.len() {
                if i + 1 < pcb.memset.len() {
                    let current = &pcb.memset[i];
                    let next = &pcb.memset[i + 1];
                    let gap_start = current.start + current.len;
                    let gap_end = next.start;

                    if gap_end > gap_start && vaddr.raw() >= gap_start && vaddr.raw() < gap_end {
                        warn!(
                            "Address {:#x} falls in memory gap between areas {} and {}",
                            vaddr.raw(),
                            i,
                            i + 1
                        );
                        warn!("Gap: {:#x} - {:#x}", gap_start, gap_end);

                        // 尝试从前一个区域的文件中读取
                        if let Some(file) = &current.file {
                            let file_offset = current.offset + (vaddr.raw() - current.start);
                            warn!(
                                "Attempting to use file from previous area with offset {:#x}",
                                file_offset
                            );
                            return Some((file.clone(), file_offset, gap_end - gap_start));
                        }
                    }
                }
            }
        }

        warn!(
            "get_elf_segment_for_addr: task_id={}, process_id={}, searching for vaddr={:#x}",
            self.task_id,
            self.process_id,
            vaddr.raw()
        );
        warn!("Total memory areas: {}", pcb.memset.len());

        // 查找包含该地址且有文件引用的内存区域
        for (i, area) in pcb.memset.iter().enumerate() {
            warn!("  Area {}: start={:#x}, end={:#x}, len={:#x}, mtype={:?}, has_file={}, contains={}", 
                i, area.start, area.start + area.len, area.len, area.mtype, area.file.is_some(), area.contains(vaddr.raw()));

            if area.contains(vaddr.raw()) && area.file.is_some() {
                let file = area.file.as_ref().unwrap();

                // 改进偏移计算
                let page_aligned_vaddr = vaddr.floor().raw();
                let file_offset = area.offset + (page_aligned_vaddr - area.start);

                warn!("get_elf_segment_for_addr: FOUND! vaddr={:#x}, area_start={:#x}, area_offset={:#x}, calculated_offset={:#x}", 
                    vaddr.raw(), area.start, area.offset, file_offset);

                return Some((file.clone(), file_offset, area.len));
            }
        }

        warn!(
            "get_elf_segment_for_addr: NOT FOUND for vaddr={:#x}",
            vaddr.raw()
        );
        None
    }

    /// 简单的资源清理函数
    pub fn simple_cleanup(&self) {
        log::info!("Simple resource cleanup for task {}", self.task_id);
        
        // 内存屏障
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        
        // 清理文件描述符
        {
            let mut pcb = self.pcb.lock();
            for i in 0..pcb.fd_table.len() {
                if pcb.fd_table[i].is_some() {
                    pcb.fd_table[i] = None;
                }
            }
        }
        
        // 短暂等待
        for _ in 0..50000 {
            core::hint::spin_loop();
        }
        
        log::info!("Simple cleanup completed for task {}", self.task_id);
    }
}

impl AsyncTask for UserTask {
    fn before_run(&self) {
        self.page_table.change();
    }

    fn get_task_id(&self) -> TaskId {
        self.task_id
    }

    fn get_task_type(&self) -> TaskType {
        TaskType::MonolithicTask
    }

    #[inline]
    fn exit(&self, exit_code: usize) {
        info!("LTP_EXIT_DEBUG: Process {} exiting with code {}", self.task_id, exit_code);
        
        warn!("Process exit: task_id={}, process_id={}, exit_code={}, arch={}", 
            self.task_id, self.process_id, exit_code, 
            if cfg!(target_arch = "loongarch64") { "loongarch64" } else { "other" });
        
        let tcb_writer = self.tcb.write();
        let uaddr = tcb_writer.clear_child_tid;
        if uaddr != 0 {
            debug!("write addr: {:#x}", uaddr);
            if let Some((addr, _)) = self.page_table.translate(VirtAddr::from(uaddr)) {
                unsafe {
                    addr.get_mut_ptr::<u32>().write(0);
                }
                futex_wake(self.pcb.lock().futex_table.clone(), uaddr, 1);
            } else {
                warn!("Failed to translate clear_child_tid address: {:#x}", uaddr);
            }
        }
        
        let exit_signal = tcb_writer.exit_signal;
        drop(tcb_writer);
        
        // 改进的进程退出逻辑
        let should_cleanup_process = {
            let mut pcb = self.pcb.lock();
            
            pcb.exit_code = Some(exit_code);
            
            pcb.threads.retain(|weak_ref| {
                if let Some(thread) = weak_ref.upgrade() {
                    thread.task_id != self.task_id
                } else {
                    false
                }
            });
            
            let remaining_threads = pcb.threads.len();
            let is_main_thread = self.task_id == self.process_id;
            
            // 只有主线程退出且没有其他线程时才清理
            is_main_thread && remaining_threads == 0
        };

        if should_cleanup_process {
            warn!("Thread exit triggering process cleanup for process_id={}", self.process_id);
            let mut pcb = self.pcb.lock();
            pcb.memset.clear();
            pcb.fd_table.clear();
            pcb.children.clear();
            pcb.exit_code = Some(exit_code);

            if let Some(parent) = self.parent.read().upgrade() {
                if exit_signal != 0 {
                    parent
                        .tcb
                        .write()
                        .signal
                        .add_signal(SignalFlags::from_num(exit_signal as _));
                } else {
                    parent.tcb.write().signal.add_signal(SignalFlags::SIGCHLD);
                }
            }
        } else {
            warn!("Thread exit: task_id={} exited, but process continues", self.task_id);
        }
    }

    #[inline]
    fn exit_code(&self) -> Option<usize> {
        self.pcb.lock().exit_code
    }
}