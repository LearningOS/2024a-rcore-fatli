//! `Arc<Inode>` -> `OSInodeInner`: In order to open files concurrently
//! we need to wrap `Inode` into `Arc`,but `Mutex` in `Inode` prevents
//! file systems from being accessed simultaneously
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: for static `ROOT_INODE`,we
//! need to wrap `OSInodeInner` into `UPSafeCell`
use super::{File, Stat, StatMode};
use crate::drivers::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::vec::Vec;
use alloc::{fmt, sync::Arc};
use bitflags::*;
use easy_fs::{EasyFileSystem, Inode};
use lazy_static::*;

/// inode in memory
/// A wrapper around a filesystem inode
/// to implement File trait atop
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}
/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    /// create a new inode in memory
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
        }
    }

    /// get the inode id
    pub fn get_inode_id(&self) -> u32 {
        return self.inner.exclusive_access().inode.get_node();
    }

    /// read all data from the inode
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

/// List all apps in the root directory
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    ///  The flags argument to the open() system call is constructed by ORing together zero or more of the following values:
    pub struct OpenFlags: u32 {
        /// readyonly
        const RDONLY = 0;
        /// writeonly
        const WRONLY = 1 << 0;
        /// read and write
        const RDWR = 1 << 1;
        /// create new file
        const CREATE = 1 << 9;
        /// truncate file size to 0
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// Open a file
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            println!(
                "open file: {} success {}   CREATE  ",
                name,
                inode.get_info()
            );

            if inode.get_link_node() != 0 {
                let inode = inode.get_linked_inode();
                inode.clear();
                Some(Arc::new(OSInode::new(readable, writable, inode)))
            } else {
                // clear size
                inode.clear();
                Some(Arc::new(OSInode::new(readable, writable, inode)))
            }
        } else {
            // create file
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
        }
    } else {
        ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            println!(
                "open file: {} success {}   FIND      ",
                name,
                inode.get_info()
            );

            if inode.get_link_node() != 0 {
                println!("node info  : {}", inode.get_info());
                let inode = inode.get_linked_inode();
                println!("link node info  : {}", inode.get_info());
                Arc::new(OSInode::new(readable, writable, inode))
            } else {
                // clear size
                Arc::new(OSInode::new(readable, writable, inode))
            }


        })
    }
}

pub fn create_file_link(name: &str, link_name: &str) -> Option<Arc<OSInode>> {
    if let Some(inode) = ROOT_INODE.find(name) {
        ROOT_INODE.create_link(link_name, inode);
    }
    None
}

pub fn get_sys_fstate(fd: usize) -> Stat {
    let mut stat = Stat::default();
    stat.mode = StatMode::FILE;

    // let vec = ROOT_INODE.ls_link_list();
    // println!("file infos {:#?}", vec);
    let vec = ROOT_INODE.ls_link(fd as u32);
    println!("file infos {:#?}", vec);
    stat.nlink = vec as u32;
    stat
}

/// Remove a file
pub fn remove_link(name: &str) -> isize {
    if let Some(inode) = ROOT_INODE.find(name) {
        if inode.get_link_node() != 0 {
            println!(
                "remove_link file: {} success {}   FIND      ",
                name,
                inode.get_info()
            );
            let link_node = ROOT_INODE.find_by_nodeid(inode.get_link_node());
            println!("delete   node: {:?}", ROOT_INODE.write_delete(name));
            println!("delete line  node: {:?}", ROOT_INODE.write_delete(name));

            link_node.clear();
            inode.clear();
            return 0;
        } else {
            println!(
                "remove_link file: {} success {}   FIND      ",
                name,
                inode.get_info()
            );

            println!("delete   node: {:?}", ROOT_INODE.write_delete(name));

            for ele in ROOT_INODE.ls_delete() {
                println!("delete line  node: {}", ele);
            }

            ROOT_INODE.find(name).map(|inode| {
                println!(
                    "delete not ok  node: {} link_node: {}",
                    inode.get_node(),
                    inode.get_link_node()
                );
            });

            inode.clear();
            return 0;
        }
    }
    -1
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
}
