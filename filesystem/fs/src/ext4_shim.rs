use alloc::{
    ffi::CString,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::iter::zip;
use devices::get_blk_device;
use lwext4_rust::{
    bindings::{ext4_fsymlink, ext4_readlink, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY},
    Ext4BlockWrapper, Ext4File, InodeTypes, KernelDevOp,
};
use sync::Mutex;
use syscalls::Errno;
use vfscore::{
    DirEntry, FileSystem, FileType, INodeInterface, StatFS, StatMode, TimeSpec, VfsResult,
};

// 保持底层块设备扇区大小 512 字节
const BLOCK_SIZE: usize = 0x200;

pub struct Ext4DiskWrapper {
    block_id: usize,
    offset: usize,
    blk_id: usize,
}

impl Ext4DiskWrapper {
    /// Create a new disk.
    pub const fn new(blk_id: usize) -> Self {
        Self {
            block_id: 0,
            offset: 0,
            blk_id,
        }
    }

    /// Get the position of the cursor.
    #[inline]
    pub fn position(&self) -> u64 {
        (self.block_id * BLOCK_SIZE + self.offset) as u64
    }

    /// Set the position of the cursor.
    #[inline]
    pub fn set_position(&mut self, pos: u64) {
        self.block_id = pos as usize / BLOCK_SIZE;
        self.offset = pos as usize % BLOCK_SIZE;
    }
}

impl KernelDevOp for Ext4DiskWrapper {
    type DevType = Self;

    fn write(dev: &mut Self::DevType, buf: &[u8]) -> Result<usize, i32> {
        let bdev = get_blk_device(dev.blk_id).expect("can't find block device");
        let mut written = 0;
        let mut off_in_block = dev.offset; // 0..BLOCK_SIZE-1
        let mut blk_id = dev.block_id;

        // 先处理起始非对齐部分
        if off_in_block != 0 {
            let mut block_buf = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut block_buf);
            let can = core::cmp::min(BLOCK_SIZE - off_in_block, buf.len());
            block_buf[off_in_block..off_in_block + can].copy_from_slice(&buf[..can]);
            bdev.write_blocks(blk_id, &block_buf);
            // verify
            let mut verify = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut verify);
            if verify[off_in_block..off_in_block + can] != buf[..can] {
                // retry once
                bdev.write_blocks(blk_id, &block_buf);
                bdev.read_blocks(blk_id, &mut verify);
            }
            written += can;
            off_in_block += can;
            if off_in_block == BLOCK_SIZE {
                off_in_block = 0;
                blk_id += 1;
            }
        }

        // 再处理完整块
        let remain = buf.len() - written;
        if remain >= BLOCK_SIZE {
            let full_bytes = remain - (remain % BLOCK_SIZE);
            if full_bytes > 0 {
                bdev.write_blocks(blk_id, &buf[written..written + full_bytes]);
                // verify full blocks
                let mut verify = vec![0u8; full_bytes];
                bdev.read_blocks(blk_id, &mut verify);
                if verify[..] != buf[written..written + full_bytes] {
                    // retry once
                    bdev.write_blocks(blk_id, &buf[written..written + full_bytes]);
                    bdev.read_blocks(blk_id, &mut verify);
                }
                blk_id += full_bytes / BLOCK_SIZE;
                written += full_bytes;
            }
        }

        // 最后处理尾部非对齐
        let tail = buf.len() - written;
        if tail > 0 {
            let mut block_buf = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut block_buf);
            block_buf[..tail].copy_from_slice(&buf[written..]);
            bdev.write_blocks(blk_id, &block_buf);
            // verify tail
            let mut verify = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut verify);
            if verify[..tail] != buf[written..] {
                // retry once
                bdev.write_blocks(blk_id, &block_buf);
                bdev.read_blocks(blk_id, &mut verify);
            }
            off_in_block = tail;
        }

        // 更新位置
        dev.block_id = blk_id;
        dev.offset = off_in_block % BLOCK_SIZE;
        Ok(written + tail)
    }

    fn read(dev: &mut Self::DevType, buf: &mut [u8]) -> Result<usize, i32> {
        let bdev = get_blk_device(dev.blk_id).expect("can't find block device");
        let mut readn = 0;
        let mut off_in_block = dev.offset;
        let mut blk_id = dev.block_id;

        // 起始非对齐
        if off_in_block != 0 {
            let mut block_buf = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut block_buf);
            let can = core::cmp::min(BLOCK_SIZE - off_in_block, buf.len());
            buf[..can].copy_from_slice(&block_buf[off_in_block..off_in_block + can]);
            readn += can;
            off_in_block += can;
            if off_in_block == BLOCK_SIZE {
                off_in_block = 0;
                blk_id += 1;
            }
        }

        // 完整块
        let remain = buf.len() - readn;
        if remain >= BLOCK_SIZE {
            let full_bytes = remain - (remain % BLOCK_SIZE);
            if full_bytes > 0 {
                bdev.read_blocks(blk_id, &mut buf[readn..readn + full_bytes]);
                blk_id += full_bytes / BLOCK_SIZE;
                readn += full_bytes;
            }
        }

        // 尾部非对齐
        let tail = buf.len() - readn;
        if tail > 0 {
            let mut block_buf = [0u8; BLOCK_SIZE];
            bdev.read_blocks(blk_id, &mut block_buf);
            buf[readn..].copy_from_slice(&block_buf[..tail]);
            off_in_block = tail;
        }

        dev.block_id = blk_id;
        dev.offset = off_in_block % BLOCK_SIZE;
        Ok(readn + tail)
    }

    fn seek(dev: &mut Self::DevType, off: i64, whence: i32) -> Result<i64, i32> {
        let size = get_blk_device(dev.blk_id)
            .expect("can't seek to device")
            .capacity();
        let new_pos = match whence as u32 {
            lwext4_rust::bindings::SEEK_SET => Some(off),
            lwext4_rust::bindings::SEEK_CUR => {
                dev.position().checked_add_signed(off).map(|v| v as i64)
            }
            lwext4_rust::bindings::SEEK_END => size.checked_add_signed(off as _).map(|v| v as i64),
            _ => Some(off),
        }
        .ok_or(-1)?;

        if new_pos as u64 > (size as _) {
            log::warn!("Seek beyond the end of the block device");
        }
        dev.set_position(new_pos as u64);
        Ok(new_pos)
    }

    fn flush(_dev: &mut Self::DevType) -> Result<usize, i32> {
        todo!()
    }
}

pub struct Ext4FileSystem {
    _inner: Ext4BlockWrapper<Ext4DiskWrapper>,
    root: Arc<dyn INodeInterface>,
}

unsafe impl Sync for Ext4FileSystem {}
unsafe impl Send for Ext4FileSystem {}

impl Ext4FileSystem {
    pub fn new(blk_id: usize) -> Arc<Self> {
        let disk = Ext4DiskWrapper::new(blk_id);
        info!("Got position:{}", disk.position());
        let inner = Ext4BlockWrapper::<Ext4DiskWrapper>::new(disk)
            .expect("failed to initialize EXT4 filesystem");
        let root = Arc::new(Ext4FileWrapper::new("/", InodeTypes::EXT4_DE_DIR));
        Arc::new(Self {
            _inner: inner,
            root,
        })
    }
}

#[inline(always)]
fn map_ext4_err(err: i32) -> Errno {
    Errno::new(err)
}

impl FileSystem for Ext4FileSystem {
    fn root_dir(&self) -> Arc<dyn INodeInterface> {
        self.root.clone()
    }

    fn name(&self) -> &str {
        "ext4"
    }
}

pub struct Ext4FileWrapper {
    file_type: FileType,
    inner: Mutex<Ext4File>,
}

impl Ext4FileWrapper {
    fn new(path: &str, types: InodeTypes) -> Self {
        let file: Ext4File = Ext4File::new(path, types);
        let file_type = map_ext4_type(file.get_type());
        Self {
            file_type,
            inner: Mutex::new(file),
        }
    }
}

unsafe impl Send for Ext4FileWrapper {}
unsafe impl Sync for Ext4FileWrapper {}

pub fn map_ext4_type(value: InodeTypes) -> FileType {
    match value {
        InodeTypes::EXT4_DE_UNKNOWN => FileType::File,
        InodeTypes::EXT4_DE_REG_FILE => FileType::File,
        InodeTypes::EXT4_DE_DIR => FileType::Directory,
        InodeTypes::EXT4_DE_CHRDEV => FileType::Device,
        InodeTypes::EXT4_DE_BLKDEV => FileType::Device,
        InodeTypes::EXT4_DE_FIFO => FileType::Device,
        InodeTypes::EXT4_DE_SOCK => FileType::Socket,
        InodeTypes::EXT4_DE_SYMLINK => FileType::Link,
        InodeTypes::EXT4_INODE_MODE_FIFO => todo!(),
        InodeTypes::EXT4_INODE_MODE_CHARDEV => todo!(),
        InodeTypes::EXT4_INODE_MODE_DIRECTORY => todo!(),
        InodeTypes::EXT4_INODE_MODE_BLOCKDEV => todo!(),
        InodeTypes::EXT4_INODE_MODE_FILE => todo!(),
        InodeTypes::EXT4_INODE_MODE_SOFTLINK => todo!(),
        InodeTypes::EXT4_INODE_MODE_SOCKET => todo!(),
        InodeTypes::EXT4_INODE_MODE_TYPE_MASK => todo!(),
    }
}

impl Ext4FileWrapper {
    fn path_deal_with(&self, path: &str) -> String {
        if path.starts_with('/') {
            log::warn!("path_deal_with: {}", path);
        }
        let p = path.trim_matches('/'); // 首尾去除
        if p.is_empty() || p == "." {
            return String::new();
        }

        if let Some(rest) = p.strip_prefix("./") {
            //if starts with "./"
            return self.path_deal_with(rest);
        }
        let rest_p = p.replace("//", "/");
        if p != rest_p {
            return self.path_deal_with(&rest_p);
        }

        //Todo ? ../
        //注：lwext4创建文件必须提供文件path的绝对路径
        let file = self.inner.lock();
        let path = file.get_path();
        let fpath = String::from(path.to_str().unwrap().trim_end_matches('/')) + "/" + p;
        fpath
    }
}

impl INodeInterface for Ext4FileWrapper {
    fn readat(&self, offset: usize, buffer: &mut [u8]) -> VfsResult<usize> {
        const MAX_RETRIES: usize = 3;

        for attempt in 0..MAX_RETRIES {
            let mut file = self.inner.lock();
            let path = file.get_path();
            let path = path.to_str().unwrap();

            // 读取前先确保设备/缓存已同步，避免读到旧数据
            let _ = file.file_cache_flush();

            match file.file_open(path, O_RDONLY) {
                Ok(_) => match file.file_seek(offset as _, 0) {
                    Ok(_) => match file.file_read(buffer) {
                        Ok(rsize) => {
                            let _ = file.file_close();
                            return Ok(rsize);
                        }
                        Err(e) => {
                            let _ = file.file_close();
                            if attempt == MAX_RETRIES - 1 {
                                return Err(map_ext4_err(e));
                            }
                        }
                    },
                    Err(e) => {
                        let _ = file.file_close();
                        if attempt == MAX_RETRIES - 1 {
                            return Err(map_ext4_err(e));
                        }
                    }
                },
                Err(e) => {
                    if attempt == MAX_RETRIES - 1 {
                        return Err(map_ext4_err(e));
                    }
                }
            }
        }

        // Fallback: return EIO error using map_ext4_err
        Err(map_ext4_err(5)) // 5 is EIO as defined in ext4_errno.h
    }

    fn writeat(&self, offset: usize, buffer: &[u8]) -> VfsResult<usize> {
        let mut file = self.inner.lock();
        let path = file.get_path();
        let path = path.to_str().unwrap();
        file.file_open(path, O_RDWR).map_err(map_ext4_err)?;

        // 若写入位置超过当前文件大小，优先对小间隙进行零填充，
        // 对大间隙使用 truncate 扩展，避免大块 0 的实际写入
        let current_size = file.file_size() as usize;
        if offset > current_size {
            let gap = offset - current_size;
            const ZERO_FILL_THRESHOLD: usize = 1 << 20; // 1 MiB
            if gap <= ZERO_FILL_THRESHOLD {
                // 小间隙：物理写入零，保证读回为0
                file.file_seek(current_size as i64, 0)
                    .map_err(map_ext4_err)?;
                // 分块写入，避免一次性分配过大缓冲
                const CHUNK: usize = 64 * 1024;
                let mut remain = gap;
                let mut zero_chunk = [0u8; CHUNK];
                while remain > 0 {
                    let to_write = core::cmp::min(CHUNK, remain);
                    file.file_write(&zero_chunk[..to_write])
                        .map_err(map_ext4_err)?;
                    remain -= to_write;
                }
                // 确保零填充数据落盘
                let _ = file.file_cache_flush();
            } else {
                // 大间隙：使用 truncate 逻辑扩展为稀疏区
                file.file_truncate(offset as u64).map_err(map_ext4_err)?;
            }
        }

        // 定位到目标偏移写入
        file.file_seek(offset as _, 0).map_err(map_ext4_err)?;

        // 写入：切分为页粒度，处理短写并提高持久化可靠性
        const PAGE: usize = 4096;
        let mut written = 0;
        while written < buffer.len() {
            let remain = buffer.len() - written;
            let chunk = core::cmp::min(PAGE, remain);
            // log::error!("ext4_shim::writeat page off={} len={}", offset + written, chunk);
            let mut done = 0;
            while done < chunk {
                let w = file
                    .file_write(&buffer[written + done..written + chunk])
                    .map_err(map_ext4_err)?;
                if w == 0 {
                    break;
                }
                done += w;
            }
            // 每页刷一次缓存，避免后续读到旧数据
            let _ = file.file_cache_flush();
            written += done;
            if done < chunk {
                break;
            }
        }
        // 写入后确保缓存刷新
        let _ = file.file_cache_flush();
        let _ = file.file_close();
        Ok(written)
    }

    fn mkdir(&self, name: &str) -> VfsResult<()> {
        log::warn!("mkdir name: {}", name);
        let fpath = self.path_deal_with(&name);
        self.inner.lock().dir_mk(&fpath).map_err(map_ext4_err)?;
        Ok(())
    }

    fn rmdir(&self, name: &str) -> VfsResult<()> {
        self.unlink(name)
    }

    fn remove(&self, name: &str) -> VfsResult<()> {
        self.unlink(name)
    }

    fn symlink(&self, name: &str, src: &str) -> VfsResult<()> {
        let fpath = self.path_deal_with(name);
        let fpath = fpath.as_str();
        if fpath.is_empty() {
            return Ok(());
        }
        let mut file = Ext4File::new(fpath, InodeTypes::EXT4_DE_SYMLINK);
        if file.check_inode_exist(fpath, InodeTypes::EXT4_DE_SYMLINK) {
            return Err(Errno::EEXIST);
        }
        let c_fpath = CString::new(fpath).unwrap();
        let c_src = CString::new(src).unwrap();
        unsafe {
            Errno::from_ret(ext4_fsymlink(c_src.into_raw(), c_fpath.into_raw()) as _)?;
        }
        Ok(())
    }

    fn resolve_link(&self) -> VfsResult<String> {
        let file = self.inner.lock();
        let path = file.get_path();
        let path = path.to_str().unwrap();
        let mut buffer = [0u8; 100];
        let mut rsize = 0;
        unsafe {
            Errno::from_ret(ext4_readlink(
                path.as_ptr() as _,
                buffer.as_mut_ptr() as _,
                buffer.len() as _,
                &mut rsize,
            ) as _)?;
        }
        let str = String::from_utf8_lossy(&buffer[..rsize]);
        Ok(str.to_string())
    }

    fn read_dir(&self) -> VfsResult<Vec<DirEntry>> {
        let iters = self
            .inner
            .lock()
            .lwext4_dir_entries()
            .map_err(map_ext4_err)?;
        let mut ans = Vec::new();
        for (name, file_type) in zip(iters.0, iters.1) {
            ans.push(DirEntry {
                filename: CString::from_vec_with_nul(name)
                    .map_err(|_| Errno::EINVAL)?
                    .to_str()
                    .map_err(|_| Errno::EINVAL)?
                    .to_string(),
                len: 0,
                file_type: map_ext4_type(file_type),
            })
        }
        Ok(ans)
    }

    fn lookup(&self, name: &str) -> VfsResult<Arc<dyn INodeInterface>> {
        let fpath = self.path_deal_with(name);
        let fpath = fpath.as_str();
        if fpath.is_empty() {
            return Ok(Arc::new(Ext4FileWrapper::new("/", InodeTypes::EXT4_DE_DIR)));
        }

        let mut file = self.inner.lock();

        if file.check_inode_exist(&fpath, InodeTypes::EXT4_DE_DIR) {
            Ok(Arc::new(Ext4FileWrapper::new(
                fpath,
                InodeTypes::EXT4_DE_DIR,
            )))
        } else if file.check_inode_exist(&fpath, InodeTypes::EXT4_DE_REG_FILE) {
            Ok(Arc::new(Ext4FileWrapper::new(
                fpath,
                InodeTypes::EXT4_DE_REG_FILE,
            )))
        } else if file.check_inode_exist(&fpath, InodeTypes::EXT4_DE_SYMLINK) {
            Ok(Arc::new(Ext4FileWrapper::new(
                fpath,
                InodeTypes::EXT4_DE_SYMLINK,
            )))
        } else {
            Err(Errno::ENOENT)
        }
    }

    fn create(&self, name: &str, ty: FileType) -> VfsResult<()> {
        let ext4_type = match ty {
            FileType::Directory => InodeTypes::EXT4_DE_DIR,
            FileType::File => InodeTypes::EXT4_DE_REG_FILE,
            _ => unimplemented!(),
        };

        let fpath = self.path_deal_with(name);
        let fpath = fpath.as_str();
        if fpath.is_empty() {
            return Ok(());
        }
        let mut file = self.inner.lock();
        if file.check_inode_exist(fpath, ext4_type.clone()) {
            Ok(())
        } else {
            if ext4_type == InodeTypes::EXT4_DE_DIR {
                file.dir_mk(fpath).map_err(map_ext4_err)?;
            } else {
                file.file_open(fpath, O_WRONLY | O_CREAT | O_TRUNC)
                    .map_err(map_ext4_err)?;
                file.file_close().map_err(map_ext4_err)?;
            }
            Ok(())
        }
    }

    fn truncate(&self, size: usize) -> VfsResult<()> {
        let mut file = self.inner.lock();
        let path = file.get_path();
        let path = path.to_str().unwrap();
        file.file_open(path, O_RDWR).map_err(map_ext4_err)?;

        file.file_truncate(size as _).map_err(map_ext4_err)?;
        let _ = file.file_close();
        Ok(())
    }

    fn unlink(&self, name: &str) -> VfsResult<()> {
        let fpath = self.path_deal_with(name);
        let mut file = self.inner.lock();
        if file.check_inode_exist(&fpath, InodeTypes::EXT4_DE_DIR) {
            // Recursive directory remove
            file.dir_rm(&fpath)
        } else {
            file.file_remove(&fpath)
        }
        .map_err(map_ext4_err)?;
        Ok(())
    }

    fn stat(&self, stat: &mut vfscore::Stat) -> VfsResult<()> {
        let mut file = self.inner.lock();

        if self.file_type == FileType::File {
            let path = file.get_path();
            let path = path.to_str().unwrap();
            file.file_open(path, O_RDONLY).map_err(map_ext4_err)?;

            // 获取真实的inode号
            let path_cstr = CString::new(path).unwrap();
            let mut inode_num: u32 = 0;
            let mut inode = core::mem::MaybeUninit::uninit();

            unsafe {
                if lwext4_rust::bindings::ext4_raw_inode_fill(
                    path_cstr.as_ptr(),
                    &mut inode_num,
                    inode.as_mut_ptr(),
                ) == 0
                {
                    stat.ino = inode_num as u64;
                } else {
                    stat.ino = 1; // fallback
                }
            }
        } else {
            stat.ino = 1; // 对于非文件类型，使用默认值
        }

        stat.mode = match file.get_type() {
            InodeTypes::EXT4_DE_REG_FILE => StatMode::FILE,
            InodeTypes::EXT4_DE_DIR => StatMode::DIR,
            InodeTypes::EXT4_DE_BLKDEV => StatMode::BLOCK,
            InodeTypes::EXT4_DE_SOCK => StatMode::SOCKET,
            InodeTypes::EXT4_DE_SYMLINK => StatMode::LINK,
            _ => unreachable!(),
        };
        stat.nlink = 1;
        stat.uid = 0;
        stat.gid = 0;
        stat.size = file.file_size();
        stat.blksize = 512;
        stat.blocks = 0;
        stat.rdev = 0;
        stat.atime.nsec = 0;
        stat.atime.sec = 0;
        stat.ctime.nsec = 0;
        stat.ctime.sec = 0;
        stat.mtime.nsec = 0;
        stat.mtime.sec = 0;

        if self.file_type == FileType::File {
            let _ = file.file_close();
        }
        Ok(())
        // Err(vfscore::VfsError::NotSupported)
    }

    fn statfs(&self, statfs: &mut StatFS) -> VfsResult<()> {
        statfs.ftype = 32;
        statfs.bsize = 512;
        statfs.blocks = 80;
        statfs.bfree = 40;
        statfs.bavail = 0;
        statfs.files = 32;
        statfs.ffree = 0;
        statfs.fsid = 32;
        statfs.namelen = 20;
        Ok(())
    }

    fn utimes(&self, _times: &mut [TimeSpec]) -> VfsResult<()> {
        log::warn!("not support utimes for utimes now");
        // Err(vfscore::VfsError::NotSupported)
        Ok(())
    }
}
