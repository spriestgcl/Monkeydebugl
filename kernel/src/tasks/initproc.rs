#![allow(dead_code)]
#![allow(unused_imports)]
use crate::tasks::current_user_task;

extern crate alloc;

use alloc::sync::Arc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use devices::utils::get_char;
use executor::{current_task, release_task, task::TaskType, tid2task, yield_now, TASK_MAP};
use fs::{file::File, FileType, OpenFlags};
use log::debug;
use polyhal::{debug_console::DebugConsole, instruction::shutdown};
use vfscore::INodeInterface;

use crate::tasks::add_user_task;
use executor::AsyncTask;
use sync::Mutex;

use crate::tasks::exec_with_process;
use crate::user::entry::user_entry;
use alloc::sync::Weak;
use executor::thread;
use fs::pathbuf::PathBuf;

use super::UserTask;

/*Add：全局配置libc.so的路径 */
static LIBC_PATH: Mutex<String> = Mutex::new(String::new());
static GLIBC_PATH: Mutex<String> = Mutex::new(String::new());

pub fn set_libc_path(path: String) {
    *LIBC_PATH.lock() = path;
}

pub fn get_libc_path() -> String {
    LIBC_PATH.lock().clone()
}

pub fn set_glibc_path(path: String) {
    *GLIBC_PATH.lock() = path;
}

pub fn get_glibc_path() -> String {
    GLIBC_PATH.lock().clone()
}
fn clear() {
    DebugConsole::putchar(0x1b);
    DebugConsole::putchar(0x5b);
    DebugConsole::putchar(0x48);
    DebugConsole::putchar(0x1b);
    DebugConsole::putchar(0x5b);
    DebugConsole::putchar(0x32);
    DebugConsole::putchar(0x4a);
}

async fn kill_all_tasks() {
    TASK_MAP.lock().values().into_iter().for_each(|task| {
        task.upgrade().inspect(|x| {
            if x.get_task_type() == TaskType::MonolithicTask {
                x.exit(100)
            }
        });
    });
}

/*async fn command(cmd: &str) {
    let mut args: Vec<&str> = cmd.split(" ").filter(|x| *x != "").collect();
    debug!("cmd: {}  args: {:?}", cmd, args);
    let filename = args.drain(..1).last().unwrap();
    match File::open(filename.into(), OpenFlags::O_RDONLY) {
        Ok(_) => {
            info!("exec: {}", filename);
            let mut args_extend = vec![filename];
            args_extend.extend(args.into_iter());
            let task_id = add_user_task(&filename, args_extend, Vec::new()).await;
            let task = tid2task(task_id).unwrap();
            loop {
                if task.exit_code().is_some() {
                    release_task(task_id);
                    break;
                }
                yield_now().await;
            }
            // syscall(SYS_WAIT4, [0,0,0,0,0,0,0])
            //     .await
            //     .expect("can't wait a pid");
        }
        Err(_) => {
            println!("unknown command: {}", cmd);
        }
    }
}*/
async fn command(cmd: &str, work_dir: PathBuf) {
    let mut args: Vec<&str> = cmd.split(" ").filter(|x| *x != "").collect();
    debug!("cmd: {}  args: {:?}", cmd, args);
    let filename = args.drain(..1).last().unwrap();
    match File::open(filename.into(), OpenFlags::O_RDONLY) {
        Ok(_) => {
            info!("exec: {}", filename);
            let mut args_extend = vec![filename];
            args_extend.extend(args.into_iter());
            // 在这里克隆 work_dir，以便后续使用
            let work_dir_clone = work_dir.clone();
            // Use custom working directory to create task
            let curr_task = current_task();
            let task = UserTask::new(Weak::new(), work_dir);
            task.before_run();
            exec_with_process(
                task.clone(),
                work_dir_clone, // 使用传入的工作目录，而不是空的PathBuf
                String::from(filename),
                args_extend.into_iter().map(String::from).collect(),
                Vec::<&str>::new().into_iter().map(String::from).collect(),
            )
            .await
            .expect("can't add task to excutor");
            curr_task.before_run();
            let task_id = task.get_task_id();
            thread::spawn(task.clone(), user_entry());

            let task = tid2task(task_id).unwrap();
            loop {
                if task.exit_code().is_some() {
                    release_task(task_id);
                    break;
                }
                yield_now().await;
            }
        }
        Err(_) => {
            println!("unknown command: {}", cmd);
        }
    }
}

pub async fn initproc() {
    #[cfg(not(target_arch = "loongarch64"))]
    {
        set_libc_path("/musl/lib/libc.so".to_string());
        println!("start kernel tasks");
        set_glibc_path("/glibc/lib/ld-linux-riscv64-lp64d.so.1".to_string());
        let home_dir = PathBuf::from("/glibc");
         command("/glibc/runtest.exe -w entry-static.exe pthread_exit_cancel", home_dir.clone()).await;

        
        let home_dir = PathBuf::from("/musl/basic");        
         command("/musl/busybox sh", home_dir.clone()).await;

        command(
            "/musl/busybox echo #### OS COMP TEST GROUP START basic-musl ####",
            home_dir.clone(),
        )
        .await;
        command("/musl/busybox sh /musl/basic/run-all.sh", home_dir.clone()).await;
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP END basic-musl ####",
            home_dir.clone(),
        )
        .await;
        let home_dir = PathBuf::from("/musl");
        //command("/musl/busybox sh ", home_dir.clone()).await;
        command("/musl/busybox sh libcbench_testcode.sh", home_dir.clone()).await;
        // command(
        //     "/musl/busybox echo #### OS COMP TEST GROUP START iozone-musl ####",
        //     home_dir.clone(),
        // )
        // .await;
        // command(
        //     "/musl/busybox echo iozone automatic measurements",
        //     home_dir.clone(),
        // )
        // .await;
        // command("/musl/iozone -a -r 1k -s 4m", home_dir.clone()).await;
        // command(
        //     "/musl/busybox echo #### OS COMP TEST GROUP END iozone-musl ####",
        //     home_dir.clone(),
        // )
        // .await;

        command("/musl/busybox sh busybox_testcode.sh", home_dir.clone()).await;

        command("/musl/busybox sh lua_testcode.sh", home_dir.clone()).await;
        command("/musl/busybox sh libctest_testcode.sh", home_dir.clone()).await;
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP END libctest-musl ####",
            home_dir.clone().clone(),
        )
        .await;

        //command("/musl/busybox sh run-dynamic-all.sh", home_dir.clone()).await;
        //command("/musl/busybox sh run-static-all.sh", home_dir.clone()).await;
        // command("/musl/busybox sh run-dynamic.sh", home_dir.clone()).await;
        // command("/musl/busybox sh run-static.sh", home_dir.clone()).await;
        //command("/musl/busybox sh cyclictest_testcode.sh", home_dir.clone()).await;

        // command("/musl/busybox sh unixbench_testcode.sh", home_dir.clone()).await;
        //command("/musl/busybox sh lmbench_testcode.sh", home_dir.clone()).await;
        // command("/musl/busybox sh iperf_testcode.sh", home_dir.clone()).await;
        // command("/musl/busybox sh multi.sh", home_dir.clone()).await;
        // command("/musl/busybox sh iozone_testcode.sh", home_dir.clone()).await;
        set_glibc_path("/glibc/lib/ld-linux-riscv64-lp64d.so.1".to_string());
        let glibc_home_dir = PathBuf::from("/glibc/basic");
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP START basic-glibc ####",
            glibc_home_dir.clone(),
        )
        .await;
        command(
            "/glibc/busybox sh /glibc/basic/run-all.sh",
            glibc_home_dir.clone(),
        )
        .await;
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP END basic-glibc ####",
            glibc_home_dir.clone().clone(),
        )
        .await;

        let glibc_home_dir = PathBuf::from("/glibc");
        command(
            "/glibc/busybox sh libcbench_testcode.sh",
            glibc_home_dir.clone(),
        )
        .await;
        command(
            "/glibc/busybox sh busybox_testcode.sh",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/busybox sh lua_testcode.sh", glibc_home_dir.clone()).await;

        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP START libctest-glibc ####",
            glibc_home_dir.clone(),
        )
        .await;
        let glibc_exclude = vec![
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_robust_detach",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "setvbuf_unget",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_cancel_points",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_cancel",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_cond",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_tsd",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_cancel_sem_wait",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_cond_smasher",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_condattr_setclock",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_exit_cancel",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_once_deadlock",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_rwlock_ebusy",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_robust_detach",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "setvbuf_unget",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel_points",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cond",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_tsd",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel_sem_wait",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cond_smasher",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_condattr_setclock",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_exit_cancel",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_once_deadlock",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-dynamic.exe",
                "pthread_rwlock_ebusy",
                glibc_home_dir.clone(),
            ),
        ];
        run_glibc_tests(glibc_home_dir.clone(), glibc_exclude).await;
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP END libctest-glibc ####",
            glibc_home_dir.clone().clone(),
        )
        .await;

        // command(
        //     "/glibc/busybox sh cyclictest_testcode.sh",
        //     glibc_home_dir.clone(),
        // )
        // .await;
    }

    #[cfg(target_arch = "loongarch64")]
    {
        set_libc_path("/musl/lib/libc.so".to_string());
        println!("start kernel tasks");
        let home_dir = PathBuf::from("/musl/basic");
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP START basic-musl ####",
            home_dir.clone(),
        )
        .await;
        command("/musl/busybox sh /musl/basic/run-all.sh", home_dir.clone()).await;
        /*  command("/musl/busybox echo Testing brk:", home_dir.clone()).await;
        command("/musl/basic/brk", home_dir.clone()).await;

        // chdir
        command("/musl/busybox echo Testing chdir:", home_dir.clone()).await;
        command("/musl/basic/chdir", home_dir.clone()).await;

        // clone
        command("/musl/busybox echo Testing clone:", home_dir.clone()).await;
        command("/musl/basic/clone", home_dir.clone()).await;

        // close
        command("/musl/busybox echo Testing close:", home_dir.clone()).await;
        command("/musl/basic/close", home_dir.clone()).await;

        // dup2
        command("/musl/busybox echo Testing dup2:", home_dir.clone()).await;
        command("/musl/basic/dup2", home_dir.clone()).await;

        // dup
        command("/musl/busybox echo Testing dup:", home_dir.clone()).await;
        command("/musl/basic/dup", home_dir.clone()).await;

        // execve
        command("/musl/busybox echo Testing execve:", home_dir.clone()).await;
        command("/musl/basic/execve", home_dir.clone()).await;

        // exit
        command("/musl/busybox echo Testing exit:", home_dir.clone()).await;
        command("/musl/basic/exit", home_dir.clone()).await;

        // fork
        command("/musl/busybox echo Testing fork:", home_dir.clone()).await;
        command("/musl/basic/fork", home_dir.clone()).await;

        // fstat
        command("/musl/busybox echo Testing fstat:", home_dir.clone()).await;
        command("/musl/basic/fstat", home_dir.clone()).await;

        // getcwd
        command("/musl/busybox echo Testing getcwd:", home_dir.clone()).await;
        command("/musl/basic/getcwd", home_dir.clone()).await;

        // getdents
        command("/musl/busybox echo Testing getdents:", home_dir.clone()).await;
        command("/musl/basic/getdents", home_dir.clone()).await;

        // getpid
        command("/musl/busybox echo Testing getpid:", home_dir.clone()).await;
        command("/musl/basic/getpid", home_dir.clone()).await;

        // getppid
        command("/musl/busybox echo Testing getppid:", home_dir.clone()).await;
        command("/musl/basic/getppid", home_dir.clone()).await;

        // gettimeofday
        command("/musl/busybox echo Testing gettimeofday:", home_dir.clone()).await;
        command("/musl/basic/gettimeofday", home_dir.clone()).await;

        // mkdir_
        command("/musl/busybox echo Testing mkdir_:", home_dir.clone()).await;
        command("/musl/basic/mkdir_", home_dir.clone()).await;

        // mmap
        command("/musl/busybox echo Testing mmap:", home_dir.clone()).await;
        //command("/musl/basic/mmap", home_dir.clone()).await;

        // mount
        command("/musl/busybox echo Testing mount:", home_dir.clone()).await;
        command("/musl/basic/mount", home_dir.clone()).await;

        // munmap
        command("/musl/busybox echo Testing munmap:", home_dir.clone()).await;
        command("/musl/basic/munmap", home_dir.clone()).await;

        // openat
        command("/musl/busybox echo Testing openat:", home_dir.clone()).await;
        command("/musl/basic/openat", home_dir.clone()).await;

        // open
        command("/musl/busybox echo Testing open:", home_dir.clone()).await;
        command("/musl/basic/open", home_dir.clone()).await;

        // pipe
        command("/musl/busybox echo Testing pipe:", home_dir.clone()).await;
        command("/musl/basic/pipe", home_dir.clone()).await;

        // read
        command("/musl/busybox echo Testing read:", home_dir.clone()).await;
        command("/musl/basic/read", home_dir.clone()).await;

        // times
        command("/musl/busybox echo Testing times:", home_dir.clone()).await;
        command("/musl/basic/times", home_dir.clone()).await;

        // umount
        command("/musl/busybox echo Testing umount:", home_dir.clone()).await;
        command("/musl/basic/umount", home_dir.clone()).await;

        // uname
        command("/musl/busybox echo Testing uname:", home_dir.clone()).await;
        command("/musl/basic/uname", home_dir.clone()).await;

        // unlink
        command("/musl/busybox echo Testing unlink:", home_dir.clone()).await;
        command("/musl/basic/unlink", home_dir.clone()).await;

        // wait
        command("/musl/busybox echo Testing wait:", home_dir.clone()).await;
        command("/musl/basic/wait", home_dir.clone()).await;

        // waitpid
        command("/musl/busybox echo Testing waitpid:", home_dir.clone()).await;
        command("/musl/basic/waitpid", home_dir.clone()).await;

        // write
        command("/musl/busybox echo Testing write:", home_dir.clone()).await;
        command("/musl/basic/write", home_dir.clone()).await;

        // yield
        command("/musl/busybox echo Testing yield:", home_dir.clone()).await;
        command("/musl/basic/yield", home_dir.clone()).await;*/
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP END basic-musl ####",
            home_dir.clone(),
        )
        .await;
        let home_dir = PathBuf::from("/musl");
        command("/musl/busybox sh libcbench_testcode.sh", home_dir.clone()).await;
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP END basic-musl ####",
            home_dir.clone(),
        )
        .await;
        let home_dir = PathBuf::from("/musl");
        // command(
        //     "/musl/busybox echo #### OS COMP TEST GROUP START iozone-musl ####",
        //     home_dir.clone(),
        // )
        // .await;
        // command(
        //     "/musl/busybox echo iozone automatic measurements",
        //     home_dir.clone(),
        // )
        // .await;
        // command("/musl/iozone -a -r 1k -s 4m", home_dir.clone()).await;
        // command(
        //     "/musl/busybox echo #### OS COMP TEST GROUP END iozone-musl ####",
        //     home_dir.clone(),
        // )
        // .await;

        command("/musl/busybox sh busybox_testcode.sh", home_dir.clone()).await;
        command("/musl/busybox sh lua_testcode.sh", home_dir.clone()).await;
        //command("/musl/busybox sh libctest_testcode.sh", home_dir.clone()).await;
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP START libctest-musl ####",
            home_dir.clone(),
        )
        .await;
        let musl_exclude = vec![
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_robust_detach",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "setvbuf_unget",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_cancel_points",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_cancel",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_cond",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_tsd",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_cancel_sem_wait",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_cond_smasher",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_condattr_setclock",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_exit_cancel",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_once_deadlock",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-static.exe",
                "pthread_rwlock_ebusy",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_robust_detach",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "setvbuf_unget",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel_points",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cond",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_tsd",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cancel_sem_wait",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_cond_smasher",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_condattr_setclock",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_exit_cancel",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_once_deadlock",
                home_dir.clone(),
            ),
            (
                "/musl/runtest.exe",
                "entry-dynamic.exe",
                "pthread_rwlock_ebusy",
                home_dir.clone(),
            ),
        ];
        run_musl_tests(home_dir.clone(), musl_exclude).await;
        command(
            "/musl/busybox echo #### OS COMP TEST GROUP END libctest-musl ####",
            home_dir.clone().clone(),
        )
        .await;

        //command("/musl/busybox sh run-dynamic-all.sh", home_dir.clone()).await;
        //command("/musl/busybox sh run-static-all.sh", home_dir.clone()).await;
        // command("/musl/busybox sh run-dynamic.sh", home_dir.clone()).await;
        // command("/musl/busybox sh run-static.sh", home_dir.clone()).await;
        //command("/musl/busybox sh cyclictest_testcode.sh", home_dir.clone()).await;

        // command("/musl/busybox sh unixbench_testcode.sh", home_dir.clone()).await;
        //command("/musl/busybox sh lmbench_testcode.sh", home_dir.clone()).await;
        // command("/musl/busybox sh iperf_testcode.sh", home_dir.clone()).await;
        // command("/musl/busybox sh multi.sh", home_dir.clone()).await;
        // command("/musl/busybox sh iozone_testcode.sh", home_dir.clone()).await;
        set_glibc_path("/glibc/lib/ld-linux-riscv64-lp64d.so.1".to_string());
        let glibc_home_dir = PathBuf::from("/glibc/basic");
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP START basic-glibc ####",
            glibc_home_dir.clone(),
        )
        .await;
        command(
            "/glibc/busybox sh /glibc/basic/run-all.sh",
            glibc_home_dir.clone(),
        )
        .await;
        // brk
        /*command("/glibc/busybox echo Testing brk:", glibc_home_dir.clone()).await;
        command("/glibc/basic/brk", glibc_home_dir.clone()).await;

        // chdir
        command("/glibc/busybox echo Testing chdir:", glibc_home_dir.clone()).await;
        command("/glibc/basic/chdir", glibc_home_dir.clone()).await;

        // clone
        command("/glibc/busybox echo Testing clone:", glibc_home_dir.clone()).await;
        command("/glibc/basic/clone", glibc_home_dir.clone()).await;

        // close
        command("/glibc/busybox echo Testing close:", glibc_home_dir.clone()).await;
        command("/glibc/basic/close", glibc_home_dir.clone()).await;

        // dup2
        command("/glibc/busybox echo Testing dup2:", glibc_home_dir.clone()).await;
        command("/glibc/basic/dup2", glibc_home_dir.clone()).await;

        // dup
        command("/glibc/busybox echo Testing dup:", glibc_home_dir.clone()).await;
        command("/glibc/basic/dup", glibc_home_dir.clone()).await;

        // execve
        command(
            "/glibc/busybox echo Testing execve:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/execve", glibc_home_dir.clone()).await;

        // exit
        command("/glibc/busybox echo Testing exit:", glibc_home_dir.clone()).await;
        command("/glibc/basic/exit", glibc_home_dir.clone()).await;

        // fork
        command("/glibc/busybox echo Testing fork:", glibc_home_dir.clone()).await;
        command("/glibc/basic/fork", glibc_home_dir.clone()).await;

        // fstat
        command("/glibc/busybox echo Testing fstat:", glibc_home_dir.clone()).await;
        command("/glibc/basic/fstat", glibc_home_dir.clone()).await;

        // getcwd
        command(
            "/glibc/busybox echo Testing getcwd:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/getcwd", glibc_home_dir.clone()).await;

        // getdents
        command(
            "/glibc/busybox echo Testing getdents:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/getdents", glibc_home_dir.clone()).await;

        // getpid
        command(
            "/glibc/busybox echo Testing getpid:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/getpid", glibc_home_dir.clone()).await;

        // getppid
        command(
            "/glibc/busybox echo Testing getppid:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/getppid", glibc_home_dir.clone()).await;

        // gettimeofday
        command(
            "/glibc/busybox echo Testing gettimeofday:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/gettimeofday", glibc_home_dir.clone()).await;

        // mkdir_
        command(
            "/glibc/busybox echo Testing mkdir_:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/mkdir_", glibc_home_dir.clone()).await;

        // mmap
        command("/glibc/busybox echo Testing mmap:", glibc_home_dir.clone()).await;
        //command("/glibc/basic/mmap", glibc_home_dir.clone()).await;

        // mount
        command("/glibc/busybox echo Testing mount:", glibc_home_dir.clone()).await;
        command("/glibc/basic/mount", glibc_home_dir.clone()).await;

        // munmap
        command(
            "/glibc/busybox echo Testing munmap:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/munmap", glibc_home_dir.clone()).await;

        // openat
        command(
            "/glibc/busybox echo Testing openat:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/openat", glibc_home_dir.clone()).await;

        // open
        command("/glibc/busybox echo Testing open:", glibc_home_dir.clone()).await;
        command("/glibc/basic/open", glibc_home_dir.clone()).await;

        // pipe
        command("/glibc/busybox echo Testing pipe:", glibc_home_dir.clone()).await;
        command("/glibc/basic/pipe", glibc_home_dir.clone()).await;

        // read
        command("/glibc/busybox echo Testing read:", glibc_home_dir.clone()).await;
        command("/glibc/basic/read", glibc_home_dir.clone()).await;

        // times
        command("/glibc/busybox echo Testing times:", glibc_home_dir.clone()).await;
        command("/glibc/basic/times", glibc_home_dir.clone()).await;

        // umount
        command(
            "/glibc/busybox echo Testing umount:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/umount", glibc_home_dir.clone()).await;

        // uname
        command("/glibc/busybox echo Testing uname:", glibc_home_dir.clone()).await;
        command("/glibc/basic/uname", glibc_home_dir.clone()).await;

        // unlink
        command(
            "/glibc/busybox echo Testing unlink:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/unlink", glibc_home_dir.clone()).await;

        // wait
        command("/glibc/busybox echo Testing wait:", glibc_home_dir.clone()).await;
        command("/glibc/basic/wait", glibc_home_dir.clone()).await;

        // waitpid
        command(
            "/glibc/busybox echo Testing waitpid:",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/basic/waitpid", glibc_home_dir.clone()).await;

        // write
        command("/glibc/busybox echo Testing write:", glibc_home_dir.clone()).await;
        command("/glibc/basic/write", glibc_home_dir.clone()).await;

        // yield
        command("/glibc/busybox echo Testing yield:", glibc_home_dir.clone()).await;
        command("/glibc/basic/yield", glibc_home_dir.clone()).await;*/
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP END basic-glibc ####",
            glibc_home_dir.clone().clone(),
        )
        .await;

        let glibc_home_dir = PathBuf::from("/glibc");
        command(
            "/glibc/busybox sh libcbench_testcode.sh",
            glibc_home_dir.clone(),
        )
        .await;
        command(
            "/glibc/busybox sh busybox_testcode.sh",
            glibc_home_dir.clone(),
        )
        .await;
        command("/glibc/busybox sh lua_testcode.sh", glibc_home_dir.clone()).await;
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP START libctest-glibc ####",
            glibc_home_dir.clone(),
        )
        .await;
        let glibc_exclude = vec![
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "pthread_robust_detach",
                glibc_home_dir.clone(),
            ),
            (
                "/glibc/runtest.exe",
                "entry-static.exe",
                "setvbuf_unget",
                glibc_home_dir.clone(),
            ),
        ];
        run_glibc_tests(glibc_home_dir.clone(), glibc_exclude).await;
        command(
            "/glibc/busybox echo #### OS COMP TEST GROUP END libctest-glibc ####",
            glibc_home_dir.clone().clone(),
        )
        .await;

        command(
            "/glibc/busybox sh cyclictest_testcode.sh",
            glibc_home_dir.clone(),
        )
        .await;
    }
    println!("!TEST FINISH!");
    shutdown();

    // Shutdown if there just have blankkernel task.
    if TASK_MAP
        .lock()
        .values()
        .find(|x| {
            x.upgrade()
                .map(|x| x.get_task_type() != TaskType::BlankKernel)
                .unwrap_or(false)
        })
        .is_none()
    {
        shutdown();
    }
}
async fn run_musl_tests(home_dir: PathBuf, exclude: Vec<(&str, &str, &str, PathBuf)>) {
    let musl_static_tests = vec![
        ("/musl/runtest.exe", "entry-static.exe", "argv"),
        ("/musl/runtest.exe", "entry-static.exe", "basename"),
        ("/musl/runtest.exe", "entry-static.exe", "clocale_mbfuncs"),
        ("/musl/runtest.exe", "entry-static.exe", "clock_gettime"),
        ("/musl/runtest.exe", "entry-static.exe", "dirname"),
        ("/musl/runtest.exe", "entry-static.exe", "env"),
        ("/musl/runtest.exe", "entry-static.exe", "fdopen"),
        ("/musl/runtest.exe", "entry-static.exe", "fnmatch"),
        ("/musl/runtest.exe", "entry-static.exe", "fscanf"),
        ("/musl/runtest.exe", "entry-static.exe", "fwscanf"),
        ("/musl/runtest.exe", "entry-static.exe", "iconv_open"),
        ("/musl/runtest.exe", "entry-static.exe", "inet_pton"),
        ("/musl/runtest.exe", "entry-static.exe", "mbc"),
        ("/musl/runtest.exe", "entry-static.exe", "memstream"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_cancel_points",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "pthread_cancel"),
        ("/musl/runtest.exe", "entry-static.exe", "pthread_cond"),
        ("/musl/runtest.exe", "entry-static.exe", "pthread_tsd"),
        ("/musl/runtest.exe", "entry-static.exe", "qsort"),
        ("/musl/runtest.exe", "entry-static.exe", "random"),
        ("/musl/runtest.exe", "entry-static.exe", "search_hsearch"),
        ("/musl/runtest.exe", "entry-static.exe", "search_insque"),
        ("/musl/runtest.exe", "entry-static.exe", "search_lsearch"),
        ("/musl/runtest.exe", "entry-static.exe", "search_tsearch"),
        ("/musl/runtest.exe", "entry-static.exe", "setjmp"),
        ("/musl/runtest.exe", "entry-static.exe", "snprintf"),
        ("/musl/runtest.exe", "entry-static.exe", "socket"),
        ("/musl/runtest.exe", "entry-static.exe", "sscanf"),
        ("/musl/runtest.exe", "entry-static.exe", "sscanf_long"),
        ("/musl/runtest.exe", "entry-static.exe", "stat"),
        ("/musl/runtest.exe", "entry-static.exe", "strftime"),
        ("/musl/runtest.exe", "entry-static.exe", "string"),
        ("/musl/runtest.exe", "entry-static.exe", "string_memcpy"),
        ("/musl/runtest.exe", "entry-static.exe", "string_memmem"),
        ("/musl/runtest.exe", "entry-static.exe", "string_memset"),
        ("/musl/runtest.exe", "entry-static.exe", "string_strchr"),
        ("/musl/runtest.exe", "entry-static.exe", "string_strcspn"),
        ("/musl/runtest.exe", "entry-static.exe", "string_strstr"),
        ("/musl/runtest.exe", "entry-static.exe", "strptime"),
        ("/musl/runtest.exe", "entry-static.exe", "strtod"),
        ("/musl/runtest.exe", "entry-static.exe", "strtod_simple"),
        ("/musl/runtest.exe", "entry-static.exe", "strtof"),
        ("/musl/runtest.exe", "entry-static.exe", "strtol"),
        ("/musl/runtest.exe", "entry-static.exe", "strtold"),
        ("/musl/runtest.exe", "entry-static.exe", "swprintf"),
        ("/musl/runtest.exe", "entry-static.exe", "tgmath"),
        ("/musl/runtest.exe", "entry-static.exe", "time"),
        ("/musl/runtest.exe", "entry-static.exe", "tls_align"),
        ("/musl/runtest.exe", "entry-static.exe", "udiv"),
        ("/musl/runtest.exe", "entry-static.exe", "ungetc"),
        ("/musl/runtest.exe", "entry-static.exe", "utime"),
        ("/musl/runtest.exe", "entry-static.exe", "wcsstr"),
        ("/musl/runtest.exe", "entry-static.exe", "wcstol"),
        ("/musl/runtest.exe", "entry-static.exe", "daemon_failure"),
        ("/musl/runtest.exe", "entry-static.exe", "dn_expand_empty"),
        ("/musl/runtest.exe", "entry-static.exe", "dn_expand_ptr_0"),
        ("/musl/runtest.exe", "entry-static.exe", "fflush_exit"),
        ("/musl/runtest.exe", "entry-static.exe", "fgets_eof"),
        ("/musl/runtest.exe", "entry-static.exe", "fgetwc_buffering"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "fpclassify_invalid_ld80",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "ftello_unflushed_append",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "getpwnam_r_crash"),
        ("/musl/runtest.exe", "entry-static.exe", "getpwnam_r_errno"),
        ("/musl/runtest.exe", "entry-static.exe", "iconv_roundtrips"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "inet_ntop_v4mapped",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "inet_pton_empty_last_field",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "iswspace_null"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "lrand48_signextend",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "lseek_large"),
        ("/musl/runtest.exe", "entry-static.exe", "malloc_0"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "mbsrtowcs_overflow",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "memmem_oob_read"),
        ("/musl/runtest.exe", "entry-static.exe", "memmem_oob"),
        ("/musl/runtest.exe", "entry-static.exe", "mkdtemp_failure"),
        ("/musl/runtest.exe", "entry-static.exe", "mkstemp_failure"),
        ("/musl/runtest.exe", "entry-static.exe", "printf_1e9_oob"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "printf_fmt_g_round",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "printf_fmt_g_zeros",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "printf_fmt_n"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_robust_detach",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_cancel_sem_wait",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_cond_smasher",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_condattr_setclock",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_exit_cancel",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_once_deadlock",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "pthread_rwlock_ebusy",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "putenv_doublefree"),
        ("/musl/runtest.exe", "entry-static.exe", "regex_backref_0"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "regex_bracket_icase",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "regex_ere_backref"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "regex_escaped_high_byte",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "regex_negated_range",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "regexec_nosub"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "rewind_clear_error",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "rlimit_open_files"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "scanf_bytes_consumed",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "scanf_match_literal_eof",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "scanf_nullbyte_char",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "setvbuf_unget"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "sigprocmask_internal",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "sscanf_eof"),
        ("/musl/runtest.exe", "entry-static.exe", "statvfs"),
        ("/musl/runtest.exe", "entry-static.exe", "strverscmp"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "syscall_sign_extend",
        ),
        ("/musl/runtest.exe", "entry-static.exe", "uselocale_0"),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "wcsncpy_read_overflow",
        ),
        (
            "/musl/runtest.exe",
            "entry-static.exe",
            "wcsstr_false_negative",
        ),
    ];

    let musl_dynamic_tests = vec![
        ("/musl/runtest.exe", "entry-dynamic.exe", "argv"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "basename"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "clocale_mbfuncs"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "clock_gettime"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "dirname"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "dlopen"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "env"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fdopen"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fnmatch"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fscanf"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fwscanf"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "iconv_open"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "inet_pton"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "mbc"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "memstream"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_cancel_points",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "pthread_cancel"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "pthread_cond"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "pthread_tsd"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "qsort"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "random"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "search_hsearch"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "search_insque"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "search_lsearch"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "search_tsearch"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "sem_init"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "setjmp"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "snprintf"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "socket"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "sscanf"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "sscanf_long"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "stat"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strftime"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_memcpy"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_memmem"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_memset"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_strchr"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_strcspn"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "string_strstr"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strptime"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strtod"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strtod_simple"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strtof"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strtol"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strtold"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "swprintf"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "tgmath"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "time"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "tls_init"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "tls_local_exec"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "udiv"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "ungetc"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "utime"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "wcsstr"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "wcstol"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "daemon_failure"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "dn_expand_empty"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "dn_expand_ptr_0"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fflush_exit"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fgets_eof"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "fgetwc_buffering"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "fpclassify_invalid_ld80",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "ftello_unflushed_append",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "getpwnam_r_crash"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "getpwnam_r_errno"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "iconv_roundtrips"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "inet_ntop_v4mapped",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "inet_pton_empty_last_field",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "iswspace_null"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "lrand48_signextend",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "lseek_large"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "malloc_0"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "mbsrtowcs_overflow",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "memmem_oob_read"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "memmem_oob"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "mkdtemp_failure"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "mkstemp_failure"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "printf_1e9_oob"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "printf_fmt_g_round",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "printf_fmt_g_zeros",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "printf_fmt_n"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_robust_detach",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_cond_smasher",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_condattr_setclock",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_exit_cancel",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_once_deadlock",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "pthread_rwlock_ebusy",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "putenv_doublefree",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "regex_backref_0"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "regex_bracket_icase",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "regex_ere_backref",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "regex_escaped_high_byte",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "regex_negated_range",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "regexec_nosub"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "rewind_clear_error",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "rlimit_open_files",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "scanf_bytes_consumed",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "scanf_match_literal_eof",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "scanf_nullbyte_char",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "setvbuf_unget"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "sigprocmask_internal",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "sscanf_eof"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "statvfs"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "strverscmp"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "syscall_sign_extend",
        ),
        ("/musl/runtest.exe", "entry-dynamic.exe", "tls_get_new_dtv"),
        ("/musl/runtest.exe", "entry-dynamic.exe", "uselocale_0"),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "wcsncpy_read_overflow",
        ),
        (
            "/musl/runtest.exe",
            "entry-dynamic.exe",
            "wcsstr_false_negative",
        ),
    ];

    for tests in vec![musl_static_tests, musl_dynamic_tests] {
        for (runtest, entry, test_name) in tests {
            let test_dir = home_dir.clone();
            if !exclude
                .iter()
                .any(|&(ex_runtest, ex_entry, ex_test_name, ref ex_dir)| {
                    ex_runtest == runtest
                        && ex_entry == entry
                        && ex_test_name == test_name
                        && ex_dir == &test_dir
                })
            {
                command(
                    &alloc::format!("{} -w {} {}", runtest, entry, test_name),
                    test_dir,
                )
                .await;
            }
        }
    }
}

async fn run_glibc_tests(glibc_home_dir: PathBuf, exclude: Vec<(&str, &str, &str, PathBuf)>) {
    let glibc_static_tests = vec![
        ("/glibc/runtest.exe", "entry-static.exe", "argv"),
        ("/glibc/runtest.exe", "entry-static.exe", "basename"),
        ("/glibc/runtest.exe", "entry-static.exe", "clocale_mbfuncs"),
        ("/glibc/runtest.exe", "entry-static.exe", "clock_gettime"),
        ("/glibc/runtest.exe", "entry-static.exe", "dirname"),
        ("/glibc/runtest.exe", "entry-static.exe", "env"),
        ("/glibc/runtest.exe", "entry-static.exe", "fdopen"),
        ("/glibc/runtest.exe", "entry-static.exe", "fnmatch"),
        ("/glibc/runtest.exe", "entry-static.exe", "fscanf"),
        ("/glibc/runtest.exe", "entry-static.exe", "fwscanf"),
        ("/glibc/runtest.exe", "entry-static.exe", "iconv_open"),
        ("/glibc/runtest.exe", "entry-static.exe", "inet_pton"),
        ("/glibc/runtest.exe", "entry-static.exe", "mbc"),
        ("/glibc/runtest.exe", "entry-static.exe", "memstream"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_cancel_points",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "pthread_cancel"),
        ("/glibc/runtest.exe", "entry-static.exe", "pthread_cond"),
        ("/glibc/runtest.exe", "entry-static.exe", "pthread_tsd"),
        ("/glibc/runtest.exe", "entry-static.exe", "qsort"),
        ("/glibc/runtest.exe", "entry-static.exe", "random"),
        ("/glibc/runtest.exe", "entry-static.exe", "search_hsearch"),
        ("/glibc/runtest.exe", "entry-static.exe", "search_insque"),
        ("/glibc/runtest.exe", "entry-static.exe", "search_lsearch"),
        ("/glibc/runtest.exe", "entry-static.exe", "search_tsearch"),
        ("/glibc/runtest.exe", "entry-static.exe", "setjmp"),
        ("/glibc/runtest.exe", "entry-static.exe", "snprintf"),
        ("/glibc/runtest.exe", "entry-static.exe", "socket"),
        ("/glibc/runtest.exe", "entry-static.exe", "sscanf"),
        ("/glibc/runtest.exe", "entry-static.exe", "sscanf_long"),
        ("/glibc/runtest.exe", "entry-static.exe", "stat"),
        ("/glibc/runtest.exe", "entry-static.exe", "strftime"),
        ("/glibc/runtest.exe", "entry-static.exe", "string"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_memcpy"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_memmem"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_memset"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_strchr"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_strcspn"),
        ("/glibc/runtest.exe", "entry-static.exe", "string_strstr"),
        ("/glibc/runtest.exe", "entry-static.exe", "strptime"),
        ("/glibc/runtest.exe", "entry-static.exe", "strtod"),
        ("/glibc/runtest.exe", "entry-static.exe", "strtod_simple"),
        ("/glibc/runtest.exe", "entry-static.exe", "strtof"),
        ("/glibc/runtest.exe", "entry-static.exe", "strtol"),
        ("/glibc/runtest.exe", "entry-static.exe", "strtold"),
        ("/glibc/runtest.exe", "entry-static.exe", "swprintf"),
        ("/glibc/runtest.exe", "entry-static.exe", "tgmath"),
        ("/glibc/runtest.exe", "entry-static.exe", "time"),
        ("/glibc/runtest.exe", "entry-static.exe", "tls_align"),
        ("/glibc/runtest.exe", "entry-static.exe", "udiv"),
        ("/glibc/runtest.exe", "entry-static.exe", "ungetc"),
        ("/glibc/runtest.exe", "entry-static.exe", "utime"),
        ("/glibc/runtest.exe", "entry-static.exe", "wcsstr"),
        ("/glibc/runtest.exe", "entry-static.exe", "wcstol"),
        ("/glibc/runtest.exe", "entry-static.exe", "daemon_failure"),
        ("/glibc/runtest.exe", "entry-static.exe", "dn_expand_empty"),
        ("/glibc/runtest.exe", "entry-static.exe", "dn_expand_ptr_0"),
        ("/glibc/runtest.exe", "entry-static.exe", "fflush_exit"),
        ("/glibc/runtest.exe", "entry-static.exe", "fgets_eof"),
        ("/glibc/runtest.exe", "entry-static.exe", "fgetwc_buffering"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "fpclassify_invalid_ld80",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "ftello_unflushed_append",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "getpwnam_r_crash"),
        ("/glibc/runtest.exe", "entry-static.exe", "getpwnam_r_errno"),
        ("/glibc/runtest.exe", "entry-static.exe", "iconv_roundtrips"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "inet_ntop_v4mapped",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "inet_pton_empty_last_field",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "iswspace_null"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "lrand48_signextend",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "lseek_large"),
        ("/glibc/runtest.exe", "entry-static.exe", "malloc_0"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "mbsrtowcs_overflow",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "memmem_oob_read"),
        ("/glibc/runtest.exe", "entry-static.exe", "memmem_oob"),
        ("/glibc/runtest.exe", "entry-static.exe", "mkdtemp_failure"),
        ("/glibc/runtest.exe", "entry-static.exe", "mkstemp_failure"),
        ("/glibc/runtest.exe", "entry-static.exe", "printf_1e9_oob"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "printf_fmt_g_round",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "printf_fmt_g_zeros",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "printf_fmt_n"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_robust_detach",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_cancel_sem_wait",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_cond_smasher",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_condattr_setclock",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_exit_cancel",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_once_deadlock",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "pthread_rwlock_ebusy",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "putenv_doublefree",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "regex_backref_0"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "regex_bracket_icase",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "regex_ere_backref",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "regex_escaped_high_byte",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "regex_negated_range",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "regexec_nosub"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "rewind_clear_error",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "rlimit_open_files",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "scanf_bytes_consumed",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "scanf_match_literal_eof",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "scanf_nullbyte_char",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "setvbuf_unget"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "sigprocmask_internal",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "sscanf_eof"),
        ("/glibc/runtest.exe", "entry-static.exe", "statvfs"),
        ("/glibc/runtest.exe", "entry-static.exe", "strverscmp"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "syscall_sign_extend",
        ),
        ("/glibc/runtest.exe", "entry-static.exe", "uselocale_0"),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "wcsncpy_read_overflow",
        ),
        (
            "/glibc/runtest.exe",
            "entry-static.exe",
            "wcsstr_false_negative",
        ),
    ];

    let glibc_dynamic_tests = vec![
        ("/glibc/runtest.exe", "entry-dynamic.exe", "argv"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "basename"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "clocale_mbfuncs"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "clock_gettime"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "dirname"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "dlopen"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "env"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fdopen"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fnmatch"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fscanf"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fwscanf"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "iconv_open"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "inet_pton"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "mbc"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "memstream"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_cancel_points",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "pthread_cancel"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "pthread_cond"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "pthread_tsd"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "qsort"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "random"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "search_hsearch"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "search_insque"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "search_lsearch"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "search_tsearch"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "sem_init"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "setjmp"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "snprintf"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "socket"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "sscanf"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "sscanf_long"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "stat"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strftime"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_memcpy"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_memmem"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_memset"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_strchr"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_strcspn"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "string_strstr"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strptime"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strtod"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strtod_simple"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strtof"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strtol"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strtold"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "swprintf"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "tgmath"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "time"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "tls_init"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "tls_local_exec"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "udiv"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "ungetc"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "utime"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "wcsstr"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "wcstol"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "daemon_failure"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "dn_expand_empty"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "dn_expand_ptr_0"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fflush_exit"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "fgets_eof"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "fgetwc_buffering",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "fpclassify_invalid_ld80",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "ftello_unflushed_append",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "getpwnam_r_crash",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "getpwnam_r_errno",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "iconv_roundtrips",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "inet_ntop_v4mapped",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "inet_pton_empty_last_field",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "iswspace_null"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "lrand48_signextend",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "lseek_large"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "malloc_0"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "mbsrtowcs_overflow",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "memmem_oob_read"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "memmem_oob"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "mkdtemp_failure"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "mkstemp_failure"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "printf_1e9_oob"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "printf_fmt_g_round",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "printf_fmt_g_zeros",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "printf_fmt_n"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_robust_detach",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_cond_smasher",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_condattr_setclock",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_exit_cancel",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_once_deadlock",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "pthread_rwlock_ebusy",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "putenv_doublefree",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "regex_backref_0"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "regex_bracket_icase",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "regex_ere_backref",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "regex_escaped_high_byte",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "regex_negated_range",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "regexec_nosub"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "rewind_clear_error",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "rlimit_open_files",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "scanf_bytes_consumed",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "scanf_match_literal_eof",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "scanf_nullbyte_char",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "setvbuf_unget"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "sigprocmask_internal",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "sscanf_eof"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "statvfs"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "strverscmp"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "syscall_sign_extend",
        ),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "tls_get_new_dtv"),
        ("/glibc/runtest.exe", "entry-dynamic.exe", "uselocale_0"),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "wcsncpy_read_overflow",
        ),
        (
            "/glibc/runtest.exe",
            "entry-dynamic.exe",
            "wcsstr_false_negative",
        ),
    ];

    for tests in vec![glibc_static_tests, glibc_dynamic_tests] {
        for (runtest, entry, test_name) in tests {
            let test_dir = glibc_home_dir.clone();
            if !exclude
                .iter()
                .any(|&(ex_runtest, ex_entry, ex_test_name, ref ex_dir)| {
                    ex_runtest == runtest
                        && ex_entry == entry
                        && ex_test_name == test_name
                        && ex_dir == &test_dir
                })
            {
                command(
                    &alloc::format!("{} -w {} {}", runtest, entry, test_name),
                    test_dir,
                )
                .await;
            }
        }
    }
}
