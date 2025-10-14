//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time",
        current_task().unwrap().pid.0
    );
    let us=get_time_us();
    let sec =us / 1_000_000;
    let usec = us % 1_000_000;
    let time_val = TimeVal { sec, usec };
    let token = current_user_token();
    let time_val_size = core::mem::size_of::<TimeVal>();
    let byte_array = unsafe{
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            time_val_size
        )
    };
    let buffers = translated_byte_buffer(token, _ts as *const u8, time_val_size);
    let mut total_len=0;
    for buffer in buffers{
        let len = buffer.len().min(time_val_size - total_len);
        buffer[..len].copy_from_slice(&byte_array[total_len..total_len + len]);
        total_len += len;
        if total_len == time_val_size {
           break;
        }
    }
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _start % PAGE_SIZE != 0 { // 如果虚拟地址没有按页对齐直接失败
        return -1;
    }
    if _port & !0x7 != 0 { // _prot 其余位必须为 0
        return -1;
    }
    if _port & 0x7 == 0 { // 这样的内存无意义
        return -1;
    }
    let num_pages = (_len + PAGE_SIZE - 1) / PAGE_SIZE; // page 数向上取整
    let mut map_perm: MapPermission = MapPermission::U; // MapPermission::V 会在 page_table 的 map 中被加上
    if _port & 0x1 != 0 { // read
        map_perm |= MapPermission::R;
    }
    if _port & 0x2 != 0 { // write
        map_perm |= MapPermission::W;
    }
    if _port & 0x4 != 0 { // execute
        map_perm |= MapPermission::X;
    }
    let vpn = VirtAddr::from(_start).floor();
    match map_for_current_task(vpn, num_pages, map_perm) {
        0 => {
            return 0;
        },
        _ => {
            return -1;
        }
    };
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if _start % PAGE_SIZE != 0 { // 如果虚拟地址没有按页对齐直接失败
        return -1;
    }
    let num_pages = (_len + PAGE_SIZE - 1) / PAGE_SIZE; // page 数向上取整
    let vpn = VirtAddr::from(_start).floor();
    match unmap_for_current_task(vpn, num_pages) {
        0 => {
            return 0;
        },
        _ => {
            return -1;
        },
    };
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
use crate::task::TaskControlBlock;
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        // 创建新进程
        let new_task = Arc::new(TaskControlBlock::new(data));
        let new_pid = new_task.pid.0;
        // 将新进程设置为当前进程的子进程
        let current = current_task().unwrap();
        let mut current_inner = current.inner_exclusive_access();
        let mut new_inner = new_task.inner_exclusive_access();
        new_inner.parent = Some(Arc::downgrade(&current));
        current_inner.children.push(new_task.clone());
        drop(new_inner);
        drop(current_inner);
        // 添加新进程到调度器
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _prio < 2 {
        return -1;
    }
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    inner.priority = _prio as usize;
    _prio
}
