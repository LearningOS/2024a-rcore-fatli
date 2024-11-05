//! Process management syscalls
use core::ops::DerefMut;

use alloc::sync::Arc;

use crate::mm::{frame_alloc, MapArea, MapType, PTEFlags, VirtPageNum};
#[allow(unused_imports)]
use crate::{
    config::{MAX_SYSCALL_NUM, PAGE_SIZE},
    loader::get_app_data_by_name,
    mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission, VirtAddr},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskStatus, PROCESSOR,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
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
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
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

pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();

    let ts1 = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };

    copy_to_virt(&ts1, ts);

    0
}

fn copy_to_virt<T>(src: &T, dst: *mut T) {
    let src_buf_ptr: *const u8 = unsafe { core::mem::transmute(src) };
    let dst_buf_ptr: *const u8 = unsafe { core::mem::transmute(dst) };
    let len = core::mem::size_of::<T>();
    let dst_frames = translated_byte_buffer(current_user_token(), dst_buf_ptr, len);

    let mut offset = 0;
    for dst_frame in dst_frames {
        dst_frame.copy_from_slice(unsafe {
            core::slice::from_raw_parts(src_buf_ptr.add(offset), dst_frame.len())
        });

        offset += dst_frame.len();
    }
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let (syscall_times, time, task_status) = PROCESSOR.exclusive_access().current_task();

    let task_info = TaskInfo {
        status: task_status,
        syscall_times: syscall_times,
        time: time,
    };
    copy_to_virt(&task_info, ti);

    0
}

// YOUR JOB: Implement mmap.
#[allow(unused_variables)]
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");

    let va_start: VirtAddr = start.into();
    if !va_start.aligned() {
        debug!("unmap fail don't aligned");
        return -1;
    }

    if port == 0 || port & 0b0000_1000 != 0 {
        return -1;
    }

    let mut va_start: VirtPageNum = va_start.into();

    let mut map_perm: MapPermission = MapPermission::U;

    let mut flags = PTEFlags::from_bits(port as u8).unwrap();

    if port & 0b0000_0001 != 0 {
        flags |= PTEFlags::R;
        map_perm |= MapPermission::R;
    }

    if port & 0b0000_0010 != 0 {
        flags |= PTEFlags::W;
        map_perm |= MapPermission::W;
    }

    if port & 0b0000_0100 != 0 {
        flags |= PTEFlags::X;
        map_perm |= MapPermission::X;
    }

    flags |= PTEFlags::U;
    flags |= PTEFlags::V;

    let va_end: VirtAddr = (start + len).into();
    let va_end: VirtPageNum = va_end.ceil();

    let block = PROCESSOR.exclusive_access().current().unwrap();
    let mut block = block.inner_exclusive_access();

    let block = block.deref_mut();
    let mem_set = &mut block.memory_set;

    println!(
        "start = {:x} && va_star = {} && va_end = {}",
        start, va_start.0, va_end.0
    );

    while va_start != va_end {
        println!("map va_start = {}", va_start.0);
        if let Some(pte) = mem_set.translate(va_start) {
            if pte.is_valid() {
                // println!("mmap found exit va_start {}", va_start.0);
                return -1;
            }
        }

        let map_type = MapType::Framed;
        let next = VirtPageNum(va_start.0 + 1);

        let mut map = Some(MapArea::new(
            va_start.into(),
            next.into(),
            map_type,
            map_perm,
        ));
        map.as_mut()
            .unwrap()
            .map_one(&mut mem_set.page_table, va_start);
        mem_set.areas.push(map.unwrap());

        va_start = VirtPageNum(va_start.0 + 1);
    }

    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");

    let va_start: VirtAddr = start.into();
    if !va_start.aligned() {
        debug!("unmap fail don't aligned");
        return -1;
    }
    let mut start_page_num: VirtPageNum = va_start.into();

    let va_end: VirtAddr = (start + len).into();
    let end_page_num: VirtPageNum = va_end.ceil();

    let block = PROCESSOR.exclusive_access().current().unwrap();
    let mut block = block.inner_exclusive_access();

    let block = block.deref_mut();
    let mem_set = &mut block.memory_set;

    while start_page_num != end_page_num {
        // println!("unmap va_start = {}", va_start.0);
        if let Some(item) = mem_set.page_table.translate(start_page_num) {
            if !item.is_valid() {
                debug!("unmap on no map vpn");
                return -1;
            }
        } else {
            return -1;
        }
        //mem_set.page_table.unmap(start_page_num);
        mem_set.remove_area(
            start_page_num.into(),
            VirtPageNum(start_page_num.0 + 1).into(),
        );
        start_page_num = VirtPageNum(start_page_num.0 + 1);
    }
    0
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
pub fn sys_spawn(path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(&data);
        let new_pid = new_task.pid.0;
        // modify trap context of new_task, because it returns immediately after switching
        let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
        // we do not have to move to next instruction since we have done it before
        // for child process, fork returns 0
        trap_cx.x[10] = 0;
        // add new task to scheduler
        add_task(new_task);
        return new_pid as isize;
    } else {
        -1
    }
}

// syscall ID：140
// 设置当前进程优先级为 prio
// 参数：prio 进程优先级，要求 prio >= 2
// 返回值：如果输入合法则返回 prio，否则返回 -1

// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    if prio < 2 {
        return -1;
    }
    let current_task = current_task().unwrap();
  
    current_task.update_prority(prio as u32);

    0
}
