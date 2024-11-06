//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.
use core::borrow::BorrowMut;
use crate::config::MAX_SYSCALL_NUM;
use crate::syscall::SYSCALL_WRITE;
use crate::timer::get_time_ms;
use alloc::collections::btree_map::BTreeMap;
use alloc::vec::Vec;


use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{TaskContext, TaskControlBlock};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// Processor management structure
pub struct Processor {
    ///The task currently executing on the current processor
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    ///Create an empty Processor
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }

    
    pub fn current_process(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }

    /**
     * 记录系统第一次调用时间
     */
    pub fn update_syscall_times(&mut self, call_id: usize) {

        // assert!(3 <= info.syscall_times[SYSCALL_GETTIMEOFDAY]);
        // assert_eq!(1, info.syscall_times[SYSCALL_TASK_INFO]);
        // assert_eq!(0, info.syscall_times[SYSCALL_WRITE]);
        // assert!(0 < info.syscall_times[SYSCALL_YIELD]);
        // assert_eq!(0, info.syscall_times[SYSCALL_EXIT]);



        let current_block = self.current().unwrap();
        let mut current = current_block.inner_exclusive_access();
        let syscall_times = current.get_syscall_times();
        syscall_times[call_id] = syscall_times[call_id] + 1;

        let mut cmd;
        match call_id {
           // SYSCALL_GETTIMEOFDAY=>cmd="SYSCALL_GETTIMEOFDAY",
          //  SYSCALL_TASK_INFO=>cmd="SYSCALL_TASK_INFO",
            SYSCALL_WRITE=>cmd="SYSCALL_WRITE",
           // SYSCALL_YIELD=>cmd="SYSCALL_YIELD",
           // SYSCALL_EXIT=>cmd="SYSCALL_EXIT",
            _=>cmd="unknown",
        }

        if cmd!="unknown" {
           // println!("------update_syscall_times cmd:{} pid: {}, call_id: {} ,times: {}---------",cmd, current_block.pid.0, call_id, syscall_times[call_id])
        }
       
 
    }

    /**
     *  Get the current task id, context, and status.
     */
    pub fn current_task(&self) -> ([u32; MAX_SYSCALL_NUM], usize, TaskStatus) {
        let current = self.current().unwrap();
        let mut current = current.inner_exclusive_access();

        (
            current.syscall_times.clone(),
            get_time_ms() - *current.get_tasks_first(),
            TaskStatus::Running,
        )
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;
      

            
            // else {

            //     let first = processor.tasks_first.get(&pid).unwrap().clone();
            //     let used = processor.tasks_use_time.get_mut(&pid).unwrap();

            //     *used = *used + (get_time_ms() - first) as usize;
            // }

            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            if let Some (current) = processor.current() {
                let mut current = current.inner_exclusive_access();

                if *current.get_tasks_first() == 0 {
                    *current.get_tasks_first() = get_time_ms();
                }
            }

            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

///Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

///Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}
