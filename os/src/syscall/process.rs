//! Process management syscalls

use crate::mm::{frame_alloc, MapArea, MapType, PTEFlags, VirtPageNum};
#[allow(unused_imports)]
use crate::{
    config::{MAX_SYSCALL_NUM, PAGE_SIZE},
    mm::{translated_byte_buffer, MapPermission, VirtAddr},
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskStatus, TASK_MANAGER,
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
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
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
    let (syscall_times, time, task_status) = TASK_MANAGER.current_task();

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

    // if start % PAGE_SIZE != 0 {
    //     return -1;
    // }

    // if port & !0x7 != 0 {
    //     return -1;
    // }

    // if port & 0x7 == 0 {
    //     return -1;
    // }

    // let map_perm = MapPermission::from_bits_truncate((port as u8) << 1);

    // let page_count = (len + PAGE_SIZE - 1) / PAGE_SIZE;

    // let block = TASK_MANAGER.get_current_task_control_block();

    // let inner = TASK_MANAGER.inner.exclusive_access();

    // let mem_set = &mut block.memory_set;

    // for i in 0..page_count {
    //     let start_va = VirtAddr::from(start + i * PAGE_SIZE);
    //     let end_va = VirtAddr::from(start + (i + 1) * PAGE_SIZE);

    //     info!(
    //         "Mapping page: start_va = {:#x},end_va = {:#x}",
    //         start_va.0, end_va.0
    //     );
    //     mem_set.insert_framed_area(start_va, end_va, map_perm);
    // }

    // drop(inner);
    // 0

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

    let block = TASK_MANAGER.get_current_task_control_block();
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
        let next =  VirtPageNum(va_start.0 + 1);

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

    let block = TASK_MANAGER.get_current_task_control_block();
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
        mem_set.remove_area(start_page_num.into(), VirtPageNum(start_page_num.0 + 1).into());
        start_page_num = VirtPageNum(start_page_num.0 + 1);
    }
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
