use core::fmt::{write, Display};

use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::{format, string::ToString};
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
///

pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
    node_id: u32,
    link_node: u32,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
        node_id: u32,
        link_node: u32,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
            node_id,
            link_node: link_node,
        }
    }

    /// Get the info of the inode
    pub fn get_info(&self) -> String {
        return format!(
            "block_id {}  block_offset {}    node_id {} link_node_id {}",
            self.block_id, self.block_offset, self.node_id, self.link_node
        );
    }

    /// Get the link node id of the inode
    pub fn get_link_node(&self) -> u32 {
        self.link_node
    }

    /// Get the link node id of the inode
    pub fn get_node(&self) -> u32 {
        self.node_id
    }

    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }

    /// Find inode under a disk inode by name and return the dirent
    fn find_inode_direntry(&self, name: &str, disk_inode: &DiskInode) -> Option<DirEntry> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent);
            }
        }
        None
    }

    /// Get the linked inode of the current inode
    pub fn get_linked_inode(&self) -> Arc<Inode> {
        let fs = self.fs.lock();
        let (block_id, block_offset) = fs.get_disk_inode_pos(self.link_node);
        
        return Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            self.link_node,
            0,
        ));
    }

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let item = self.find_inode_direntry(name, disk_inode);

            if let Some(dir) = item {
                if dir.is_deleted() {
                    return None;
                } else {
                    let mut inode_id = dir.inode_id();
                    if dir.link_node_id() != 0 {
                        inode_id = dir.link_node_id();
                    }

                    let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                    return Some(Arc::new(Self::new(
                        block_id,
                        block_offset,
                        self.fs.clone(),
                        self.block_device.clone(),
                        dir.inode_id(),
                        dir.link_node_id(),
                    )));
                }
            } else {
                return None;
            }
        })
    }

    /// Find inode by node id
    pub fn find_by_nodeid(&self, inode_id: u32) -> Arc<Inode> {
        let fs = self.fs.lock();
        let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
        Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            inode_id,
            inode_id,
        ))
    }

    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }

    /// Create a hard link to a inode under current inode by name
    pub fn create_link(&self, name: &str, target: Arc<Inode>) -> Option<Arc<Inode>> {
        let mut fs: MutexGuard<'_, EasyFileSystem> = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new_link(name, new_inode_id, target.node_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            new_inode_id,
            target.node_id,
        )))
        // release efs lock automatically by compiler
    }

    // /// Delete a hard link to a inode under current inode by name
    // pub fn delete(&self, name: &str, new_inode_id: u32) -> usize {
    //     let mut file_count = 0;
    //     self.modify_disk_inode(|root_inode| {
    //         // append file in the dirent
    //         file_count = (root_inode.size as usize) / DIRENT_SZ;

    //         let dirent = DirEntry::new_delete(, new_inode_id);
    //         root_inode.write_at(
    //             file_count * DIRENT_SZ,
    //             dirent.as_bytes(),
    //             &self.block_device,
    //         );
    //     });
    //     file_count
    // }

    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs: MutexGuard<'_, EasyFileSystem> = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            new_inode_id,
            0,
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }

    /// List inodes under current inode
    // pub fn ls_link_list(&self) -> Vec<String> {
    //     let mut vec = Vec::new();
    //     let _fs = self.fs.lock();
    //     self.read_disk_inode(|disk_inode| {
    //         let file_count = (disk_inode.size as usize) / DIRENT_SZ;

    //         for i in 0..file_count {
    //             let mut dirent = DirEntry::empty();
    //             assert_eq!(
    //                 disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
    //                 DIRENT_SZ,
    //             );
    //             let msg = format!(
    //                 "name:{}  link_node_id:{}  inode_id:{}",
    //                 dirent.name(),
    //                 dirent.link_node_id(),
    //                 dirent.inode_id()
    //             );
    //             vec.push(msg);
    //         }
    //     });
    //     vec
    // }

    // /// List inodes under current inode
    // pub fn ls_link(&self, node: u32) -> Vec<String> {
    //     let mut vec = Vec::new();
    //     let mut size = 0;
    //     let change = &mut size;
    //     let _fs = self.fs.lock();
    //     self.read_disk_inode(|disk_inode| {
    //         let file_count = (disk_inode.size as usize) / DIRENT_SZ;

    //         for i in 0..file_count {
    //             let mut dirent = DirEntry::empty();

    //             assert_eq!(
    //                 disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
    //                 DIRENT_SZ,
    //             );
    //             if dirent.inode_id() == node {
    //                 if !dirent.name().is_empty() {
    //                     *change = *change + 1;
    //                 }
    //                 let msg = format!(
    //                     "       ADD   name:{}  link_node_id:{}  inode_id:{} eq node_id:{}",
    //                     dirent.name(),
    //                     dirent.link_node_id(),
    //                     dirent.inode_id(),
    //                     node
    //                 );
    //                 vec.push(msg);
    //             }

    //             if dirent.link_node_id() == node {
    //                 *change = *change + 1;
    //                 let msg = format!(
    //                     "       ADD   name:{}  link_node_id:{}  inode_id:{} eq node_id:{}",
    //                     dirent.name(),
    //                     dirent.link_node_id(),
    //                     dirent.inode_id(),
    //                     node
    //                 );
    //                 vec.push(msg);
    //             } else {
    //                 let msg = format!(
    //                     "       NOT CONTAINS   name:{}  link_node_id:{}  inode_id:{} not eq node_id:{}",
    //                     dirent.name(),
    //                     dirent.link_node_id(),
    //                     dirent.inode_id(),
    //                     node
    //                 );
    //                 vec.push(msg);
    //             }
    //         }
    //     });
    //     vec
    // }

    /// List inodes under current inode
    pub fn ls_link(&self, node: u32) -> usize {
        let mut size = 0;
        let change = &mut size;
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;

            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if dirent.inode_id() == node {
                    if !dirent.name().is_empty() {
                        *change = *change + 1;
                    }
                }
                if dirent.link_node_id() == node {
                    *change = *change + 1;
                }
            }
        });
        size
    }

    /// List inodes under current inode
    pub fn ls_delete(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if dirent.is_deleted() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }

    /// List inodes under current inode
    pub fn write_delete(&self, name: &str) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let _fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;

            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                // assert_eq!(
                //     disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                //     DIRENT_SZ,
                // );
                disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device);
                if name == dirent.name() {
                    dirent.set_deleted();
                    disk_inode.write_at(i * DIRENT_SZ, dirent.as_bytes(), &self.block_device);
                    v.push(name.to_string());
                    break;
                }
            }
        });
        v
    }

    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
