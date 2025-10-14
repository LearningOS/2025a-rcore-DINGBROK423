//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A constant for a small stride, used for scheduling.
const BIG_STRIDE: usize = 1 << 20; // 适中的大常数

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Get the number of processes in the ready queue
    pub fn get_mut(&self, pid: usize) -> Option<&Arc<TaskControlBlock>> {
        self.ready_queue.get(pid)
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
use crate::task::TaskStatus;
/// Fetch a task from the ready queue using stride scheduling
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    let mut task_manager = TASK_MANAGER.exclusive_access();
    let mut min_stride_task: Option<(usize, usize)> = None; // (index, stride)
        
    for (idx, task) in task_manager.ready_queue.iter().enumerate() {
        let inner = task.inner_exclusive_access();
        if inner.task_status == TaskStatus::Ready {
            let stride = inner.stride;
            if min_stride_task.is_none() || stride < min_stride_task.unwrap().1 {
                min_stride_task = Some((idx, stride));
            }
        }
    }
    
    if let Some((idx, _)) = min_stride_task {
        // 使用remove方法获取任务
        let task = task_manager.ready_queue.remove(idx).unwrap();
        // 更新stride值，但不设置状态为Running（这由processor处理）
        let mut inner = task.inner_exclusive_access();
        let pass = BIG_STRIDE / inner.priority;
        inner.stride += pass;
        drop(inner);
        
        Some(task)
    } else {
        // 如果没有找到合适的任务，使用原来的FIFO调度
        task_manager.fetch()
    }
}