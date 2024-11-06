//!Implementation of [`TaskManager`]
use core::cell::RefMut;

use super::task::{Stride, TaskControlBlockInner};
use super::{id, TaskControlBlock, TaskStatus};
 
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // {
        //     if self.ready_queue.is_empty() {
        //         return None;
        //     }

        //     if self.ready_queue.len() == 1 {
        //         return Some(self.ready_queue.pop_front().unwrap());
        //     }
        // }

        let mut remove_idx = None;
        {
            let mut option = None;
            let mut ret: Option<Arc<TaskControlBlock>> = None;
            let mut option_inner: Option<RefMut<'_, TaskControlBlockInner>> = None;

            {
                for (idx, ele) in self.ready_queue.iter().enumerate() {
                    if ele.inner_exclusive_access_borrow().task_status == TaskStatus::Ready {
                        if option.is_none() {
                            option = Some(ele);
                            option = Some(ele);
                            option_inner = Some(option.as_ref().unwrap().inner_exclusive_access());
                            remove_idx = Some(idx);
                        } else {
                            let ele_inner = ele.inner_exclusive_access();

                            if option_inner.as_ref().unwrap().stride.get_stride()
                                > ele_inner.stride.get_stride()
                            {
                                drop(option_inner);
                                option = Some(ele);
                                option_inner = Some(ele_inner);
                                remove_idx = Some(idx);
                            }
                        }
                    }
                }

                if let Some(task) = option {
                    ret = Some(task.clone());
                }
            }
        }
        let quere = &mut self.ready_queue;
        if let Some(idx) = remove_idx {
            return quere.remove(idx);
        } else {
            return None;
        }
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
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    trace!("kernel: TaskManager::fetch_task");
    //TASK_MANAGER.exclusive_access().ready_queue.pop_front()

    let item = TASK_MANAGER.exclusive_access().fetch();
    if let Some(task) = item.as_ref() {
        task.update_stride();
    }
    // println!(
    //     "fetch_task: {:?}, stride: {},status:{:?}",
    //     item.as_ref().unwrap().pid.0,
    //     item.as_ref()
    //         .unwrap()
    //         .inner_exclusive_access_borrow()
    //         .stride
    //         .get_stride(),
    //     item.as_ref()
    //         .unwrap()
    //         .inner_exclusive_access_borrow()
    //         .task_status,
    // );
    item
}