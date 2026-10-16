use std::ffi::CString;
use std::io;
use std::ptr::NonNull;

use assets_manager::{
    source::{DirEntry, FileContent, Source},
    BoxedError,
};

pub struct AndroidAssets {
    manager: NonNull<ndk_sys::AAssetManager>,
}

unsafe impl Send for AndroidAssets {}
unsafe impl Sync for AndroidAssets {}

impl AndroidAssets {
    pub fn from_raw(ptr: *mut ndk_sys::AAssetManager) -> io::Result<Self> {
        let manager = NonNull::new(ptr)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "AAssetManager is null"))?;
        Ok(Self { manager })
    }

    fn read_asset_bytes(&self, id: &str, ext: &str) -> io::Result<Vec<u8>> {
        let filename = format!("{}.{}", id, ext);
        let c_filename = match CString::new(filename.clone()) {
            Ok(s) => s,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Asset path contains null byte: {filename}"),
                ))
            }
        };

        let asset_ptr = unsafe {
            ndk_sys::AAssetManager_open(
                self.manager.as_ptr(),
                c_filename.as_ptr(),
                ndk_sys::AASSET_MODE_UNKNOWN as i32,
            )
        };

        if asset_ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Asset not found: {filename}"),
            ));
        }

        let len = unsafe { ndk_sys::AAsset_getLength(asset_ptr) } as usize;
        let mut buf = vec![0u8; len];
        let bytes_read = unsafe {
            ndk_sys::AAsset_read(
                asset_ptr,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                len,
            )
        };

        unsafe { ndk_sys::AAsset_close(asset_ptr) };

        if bytes_read < 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to read asset: {filename}"),
            ));
        }

        buf.truncate(bytes_read as usize);
        Ok(buf)
    }
}

impl Source for AndroidAssets {
    fn read(&self, id: &str, ext: &str) -> io::Result<FileContent<'_>> {
        let bytes = self.read_asset_bytes(id, ext)?;
        Ok(FileContent::Owned(Box::new(bytes) as Box<dyn AsRef<[u8]>>))
    }

    fn read_dir(&self, id: &str, f: &mut dyn FnMut(DirEntry)) -> io::Result<()> {
        let c_id = match CString::new(id) {
            Ok(s) => s,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Directory path contains null byte: {id}"),
                ))
            }
        };

        let dir_ptr = unsafe {
            ndk_sys::AAssetManager_openDir(self.manager.as_ptr(), c_id.as_ptr())
        };

        if dir_ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Asset directory not found: {id}"),
            ));
        }

        loop {
            let file_name = unsafe { ndk_sys::AAssetDir_getNextFileName(dir_ptr) };
            if file_name.is_null() {
                break;
            }

            let name = unsafe { std::ffi::CStr::from_ptr(file_name) }
                .to_string_lossy()
                .into_owned();

            let path = format!("{}/{}", id, name);
            let dot_pos = name.rfind('.');
            let ext = dot_pos.map(|p| &name[p + 1..]).unwrap_or("");

            f(DirEntry::File(&path, ext));
        }

        unsafe { ndk_sys::AAssetDir_close(dir_ptr) };

        Ok(())
    }

    fn exists(&self, entry: DirEntry) -> bool {
        match entry {
            DirEntry::File(id, ext) => {
                let filename = format!("{}.{}", id, ext);
                let c_filename = match CString::new(filename) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                let ptr = unsafe {
                    ndk_sys::AAssetManager_open(
                        self.manager.as_ptr(),
                        c_filename.as_ptr(),
                        ndk_sys::AASSET_MODE_UNKNOWN as i32,
                    )
                };
                if !ptr.is_null() {
                    unsafe { ndk_sys::AAsset_close(ptr) };
                    true
                } else {
                    false
                }
            }
            DirEntry::Directory(id) => {
                let c_id = match CString::new(id) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                let ptr = unsafe {
                    ndk_sys::AAssetManager_openDir(self.manager.as_ptr(), c_id.as_ptr())
                };
                if !ptr.is_null() {
                    unsafe { ndk_sys::AAssetDir_close(ptr) };
                    true
                } else {
                    false
                }
            }
        }
    }

    fn configure_hot_reloading(&self, _events: assets_manager::hot_reloading::EventSender) -> Result<(), BoxedError> {
        Ok(())
    }
}
