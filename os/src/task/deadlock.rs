// os/src/task/deadlock.rs
use crate::task::process::ProcessControlBlockInner;
use alloc::vec;

/// Semaphore deadlock detection using Banker's algorithm
pub fn semaphore_deadlock_detect(
    process_inner: &mut ProcessControlBlockInner, 
    sem_id: usize,
    tid: usize
) -> bool {
    let thread_count = process_inner.tasks.len();
    let semaphore_count = process_inner.semaphore_list.len();
    
    if sem_id >= semaphore_count {
        return false;
    }
    
    // Get the semaphore and check available resources
    let sem = process_inner.semaphore_list[sem_id].as_ref().unwrap();
    let available = sem.inner.exclusive_access().count;
    
    // If there are still resources available, no deadlock
    if available > 0 {
        return false;
    }
    
    // Initialize work vector to current available resources
    let mut work = vec![0; semaphore_count];
    for i in 0..semaphore_count {
        if let Some(sem) = &process_inner.semaphore_list[i] {
            work[i] = sem.inner.exclusive_access().count as usize;
        }
    }
    
    // Initialize finish vector
    let mut finish = vec![false; thread_count];
    
    // Simulate allocation for the current request
    let mut need = vec![vec![0; semaphore_count]; thread_count];
    let allocation = process_inner.semaphore_allocation.clone();
    
    // Update for the current request - mark as needed
    need[tid][sem_id] = 1;
    
    // Try to find a safe execution sequence
    let mut found = true;
    while found {
        found = false;
        
        for i in 0..thread_count {
            if !finish[i] {
                // Check if this thread's needs can be satisfied
                let mut can_allocate = true;
                for j in 0..semaphore_count {
                    if need[i][j] > work[j] {
                        can_allocate = false;
                        break;
                    }
                }
                
                if can_allocate {
                    // Simulate execution completion and resource release
                    for j in 0..semaphore_count {
                        work[j] += allocation[i][j];
                    }
                    finish[i] = true;
                    found = true;
                }
            }
        }
    }
    
    // Check if all threads can finish
    for i in 0..thread_count {
        if !finish[i] {
            // Deadlock detected
            return true;
        }
    }
    
    // No deadlock
    false
}