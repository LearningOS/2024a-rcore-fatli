//! File and filesystem-related syscalls
use core::any::{self, Any};
use core::borrow::BorrowMut;

use alloc::sync::Arc;

use crate::fs::{create_file_link, get_sys_fstate, open_file, remove_link, OpenFlags, Stat};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::syscall::copy_to_virt;
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let node_id = inode.clone().get_inode_id() as usize;
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();

        let mut contains = false;

        for ele in &mut inner.file_fd_maps {
            if ele.as_ref().unwrap().0 == fd {
                *ele = Some(Arc::new((fd, node_id)));
                contains = true;
                break;
            }
        }

        if !contains {
            inner.file_fd_maps.push(Some(Arc::new((fd, node_id))));
        }

        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap().clone();

    let mut nodeid = 3;

    let inner = task.inner_exclusive_access_borrow();

    let mut nodeid = 0;

    for ele in &inner.file_fd_maps {
        let ele = ele.as_ref().unwrap();
        if ele.0 == _fd {
            nodeid = ele.1;
            break;
        }
    }

    drop(inner);

    if nodeid == 0 {
        return -1;
    }
    else {
      let state =   get_sys_fstate(nodeid);
      //let mut state = Stat::default();
      //state.nlink = 3;

      copy_to_virt(&state, _st);
    }

    0
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let old_path = translated_str(token, _old_name);
    let new_path = translated_str(token, _new_name);

    if old_path == new_path {
        return -1;
    }
    println!("create link from {} to {}", old_path, new_path);
    create_file_link(old_path.as_str(), new_path.as_str());
    0
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );

    let token = current_user_token();
    let old_path = translated_str(token, _name);
    return remove_link(old_path.as_str());
}
