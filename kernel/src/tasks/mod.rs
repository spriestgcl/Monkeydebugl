mod async_ops;
pub mod elf;
pub mod exec;
mod filetable;
mod initproc;
mod memset;
mod shm;
mod signal;
mod task;

use self::initproc::initproc;
use crate::{consts::USER_WORK_DIR, syscall::NET_SERVER, user::entry::user_entry};
use alloc::{
    string::String,
    sync::Weak,
    {sync::Arc, vec::Vec},
};
pub use async_ops::{
    futex_requeue, futex_wake, WaitFutex, WaitHandleAbleSignal, WaitPid, WaitSignal,
};
use devices::get_net_device;
use exec::exec_with_process;
use executor::{current_task, thread, yield_now, AsyncTask, TaskId, DEFAULT_EXECUTOR};
use fs::pathbuf::PathBuf;
pub use memset::{MapTrack, MemArea, MemType};
use polyhal::common::get_cpu_num;
pub use shm::{MapedSharedMemory, SharedMemory, SHARED_MEMORY};
pub use signal::SignalList;
pub use task::UserTask;

pub enum UserTaskControlFlow {
    Continue,
    Break,
}

#[allow(dead_code)]
pub async fn handle_net() {
    let mut buffer = vec![0u8; 2048];
    // #[cfg(feature = "net")]
    loop {
        let res = get_net_device(0).recv(&mut buffer);
        if let Ok(rlen) = res {
            NET_SERVER.analysis_net_data(&buffer[..rlen]);
        }
        yield_now().await;
    }
}

pub fn init() {
    // 使用直接UART输出进行调试
    unsafe fn debug_uart_char(c: u8) {
        let uart_base = 0x800000001fe20000 as *mut u8;
        let lsr_offset = 5;
        let lsr_addr = uart_base.add(lsr_offset);
        while (lsr_addr.read_volatile() & 0x20) == 0 {}
        uart_base.write_volatile(c);
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }

    unsafe fn debug_uart_string(s: &str) {
        for byte in s.bytes() {
            debug_uart_char(byte);
            if byte == b'\n' {
                for _ in 0..2000 {
                    core::hint::spin_loop();
                }
            }
        }
    }

    unsafe {
        debug_uart_string("Tasks init: Starting executor initialization...\r\n");
    }

    // 直接使用硬编码的CPU数量，避免调用get_cpu_num()
    let cpu_count = 1;

    unsafe {
        debug_uart_string("Tasks init: Using hardcoded CPU count: 1\r\n");
    }

    unsafe {
        debug_uart_string("Tasks init: About to initialize DEFAULT_EXECUTOR...\r\n");
    }

    DEFAULT_EXECUTOR.init(cpu_count);

    unsafe {
        debug_uart_string("Tasks init: Executor initialized successfully\r\n");
    }

    unsafe {
        debug_uart_string("Tasks init: Spawning initproc task...\r\n");
    }

    thread::spawn_blank(initproc());

    unsafe {
        debug_uart_string("Tasks init: initproc task spawned successfully\r\n");
        debug_uart_string("Tasks init: Function completed\r\n");
    }

    // #[cfg(feature = "net")]
    // thread::spawn_blank(KernelTask::new(handle_net()));
}

pub fn run_tasks() {
    DEFAULT_EXECUTOR.run()
}

pub async fn add_user_task(filename: &str, args: Vec<&str>, envp: Vec<&str>) -> TaskId {
    let curr_task = current_task();
    let task = UserTask::new(Weak::new(), USER_WORK_DIR);
    task.before_run();
    exec_with_process(
        task.clone(),
        PathBuf::new(),
        String::from(filename),
        args.into_iter().map(String::from).collect(),
        envp.into_iter().map(String::from).collect(),
    )
    .await
    .expect("can't add task to excutor");
    curr_task.before_run();
    thread::spawn(task.clone(), user_entry());

    task.get_task_id()
}

#[inline]
pub fn current_user_task() -> Arc<UserTask> {
    current_task().downcast_arc::<UserTask>().ok().unwrap()
}
