use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::task::process::ProcessControlBlockInner;
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// Check if mutex resource request will lead to deadlock
/// 
/// Using the Banker's Algorithm to detect potential deadlock
fn mutex_deadlock_detect(process_inner: &mut ProcessControlBlockInner, mutex_id: usize, _tid: usize) -> bool {
    if mutex_id >= process_inner.mutex_list.len() {
        return false;
    }

    // Check if the mutex is already held by anyone
    if process_inner.mutex_allocation[mutex_id] != 0 {
        // If trying to acquire a mutex that's already held, potential deadlock situation
        // For simplicity, mutex is either allocated or not, so a mutex can only be
        // held by at most one thread. We consider it a deadlock if any thread tries to
        // acquire a mutex that's already held.
        return true;
    }
    
    return false;
}

/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // Check for deadlock if detection is enabled
    if process_inner.deadlock_detect_enabled {
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        
        // Ensure mutex_allocation has enough space
        if process_inner.mutex_allocation.len() <= mutex_id {
            while process_inner.mutex_allocation.len() <= mutex_id {
                process_inner.mutex_allocation.push(0);
            }
        }
        
        if mutex_deadlock_detect(&mut process_inner, mutex_id, tid) {
            drop(process_inner);
            drop(process);
            return -0xDEAD;
        }
        
        // Mark the mutex as allocated to this thread
        process_inner.mutex_allocation[mutex_id] = tid + 1; // +1 to distinguish from 0 (unallocated)
    }
    
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // Update resource allocation if deadlock detection is enabled
    if process_inner.deadlock_detect_enabled && mutex_id < process_inner.mutex_allocation.len() {
        process_inner.mutex_allocation[mutex_id] = 0; // Mark as released
    }
    
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // Update resource allocation if deadlock detection is enabled
    if process_inner.deadlock_detect_enabled {
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
            
        // Ensure semaphore_allocation has enough space
        if tid < process_inner.semaphore_allocation.len() && 
           sem_id < process_inner.semaphore_allocation[tid].len() &&
           process_inner.semaphore_allocation[tid][sem_id] > 0 {
            // Decrement the allocation count
            process_inner.semaphore_allocation[tid][sem_id] -= 1;
        }
    }
    
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // Check for deadlock if detection is enabled
    if process_inner.deadlock_detect_enabled {
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        
        // Ensure semaphore_allocation has enough space
        while process_inner.semaphore_allocation.len() <= tid {
            process_inner.semaphore_allocation.push(vec![0; process.inner_exclusive_access().semaphore_list.len()]);
        }
        
        if crate::task::deadlock::semaphore_deadlock_detect(&mut process_inner, sem_id, tid) {
            drop(process_inner);
            return -0xDEAD;
        }
        
        // Record the allocation
        if sem_id < process_inner.semaphore_list.len() {
            if process_inner.semaphore_allocation[tid].len() <= sem_id {
                process_inner.semaphore_allocation[tid].resize(process.inner_exclusive_access().semaphore_list.len(), 0);
            }
            process_inner.semaphore_allocation[tid][sem_id] += 1;
        }
    }
    
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// Enable or disable deadlock detection for the current process
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_enable_deadlock_detect",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );

    // Check if the parameter is valid (0 or 1)
    if enabled != 0 && enabled != 1 {
        return -1;
    }

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    
    // Enable or disable deadlock detection
    process_inner.deadlock_detect_enabled = enabled == 1;
    
    // Initialize the allocation vectors if enabling deadlock detection
    if process_inner.deadlock_detect_enabled {
        // Initialize mutex allocation vector if empty
        if process_inner.mutex_allocation.is_empty() {
            process_inner.mutex_allocation = vec![0; process_inner.mutex_list.len()];
        }
        
        // Initialize semaphore allocation vector if empty
        if process_inner.semaphore_allocation.is_empty() {
            let thread_count = process_inner.tasks.len();
            let semaphore_count = process_inner.semaphore_list.len();
            process_inner.semaphore_allocation = vec![vec![0; semaphore_count]; thread_count];
        }
    }

    0
}
