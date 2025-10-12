//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next, COUNTS, TASK_MANAGER},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// TODO: implement the syscall
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    match _trace_request {
        0 => {
            let target_addr = _id as *const u8;
            println!("trace request: 0, read value {} from address 0x{:x}", byte_value, _id);
            target_addr as isize
        }
        1 => {
            let target_addr = _id as *mut u8;
            unsafe{*target_addr= (_data & 0xff) as u8};
            println!("trace request: 1, wrote value {} to address 0x{:x}", byte_value, _id);
            0
        }
        2 => {
            let current = TASK_MANAGER.get_current_task_id();
            let syscall_id=_id as usize;
            println!("syscall_id: {}, count: {}", syscall_id, COUNTS.exclusive_access()[current][syscall_id]);
            COUNTS.exclusive_access()[current][syscall_id] as isize
        }
        _ => {
            trace!("kernel: sys_trace");
            -1
        }
    }
}
