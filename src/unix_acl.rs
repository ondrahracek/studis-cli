//! Descriptor-based extended ACL check shared by private local credential stores.

use std::fs::File;

#[cfg(target_os = "macos")]
pub(crate) fn descriptor_has_no_extended_acl(file: &File) -> Result<bool, ()> {
    descriptor_acl_is_safe(file, false)
}

#[cfg(target_os = "macos")]
pub(crate) fn descriptor_has_no_allow_acl(file: &File) -> Result<bool, ()> {
    descriptor_acl_is_safe(file, true)
}

#[cfg(target_os = "macos")]
fn descriptor_acl_is_safe(file: &File, allow_deny_entries: bool) -> Result<bool, ()> {
    use std::{ffi::c_void, os::fd::AsRawFd, ptr};

    const ACL_FIRST_ENTRY: i32 = 0;
    const ACL_NEXT_ENTRY: i32 = -1;
    const ACL_TYPE_EXTENDED: i32 = 0x100;
    const ACL_EXTENDED_DENY: i32 = 2;

    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, acl_type: i32) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, entry_id: i32, entry: *mut *mut c_void) -> i32;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut i32) -> i32;
        fn acl_free(object: *mut c_void) -> i32;
    }

    // SAFETY: `file` owns a live descriptor, and the type value comes from macOS sys/acl.h.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(errno) if errno == rustix::io::Errno::NOENT.raw_os_error() => Ok(true),
            _ => Err(()),
        };
    }

    let mut entry = ptr::null_mut();
    let mut entry_id = ACL_FIRST_ENTRY;
    let mut safe = true;
    loop {
        // SAFETY: `acl` is live, and `entry` points to writable output storage.
        let status = unsafe { acl_get_entry(acl, entry_id, &mut entry) };
        if status == -1
            && entry_id == ACL_NEXT_ENTRY
            && std::io::Error::last_os_error().raw_os_error()
                == Some(rustix::io::Errno::INVAL.raw_os_error())
        {
            break;
        }
        if status != 0 || !allow_deny_entries {
            safe = false;
            break;
        }
        let mut tag = 0;
        // SAFETY: `entry` was returned by `acl_get_entry` while `acl` is live.
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != ACL_EXTENDED_DENY {
            safe = false;
            break;
        }
        entry_id = ACL_NEXT_ENTRY;
    }
    // SAFETY: macOS requires each non-null ACL returned by `acl_get_fd_np` to be freed once.
    let free_status = unsafe { acl_free(acl) };
    if free_status != 0 {
        return Err(());
    }
    Ok(safe)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn descriptor_has_no_extended_acl(_file: &File) -> Result<bool, ()> {
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn descriptor_has_no_allow_acl(_file: &File) -> Result<bool, ()> {
    Ok(true)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{fs, process::Command, time::SystemTime};

    #[test]
    fn deny_only_acl_is_safe_but_allow_acl_is_not() {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("studis-browser-acl-{}-{nonce}", std::process::id()));
        fs::create_dir(&directory).expect("create synthetic directory");
        let deny = Command::new("/bin/chmod")
            .args(["+a", "everyone deny delete"])
            .arg(&directory)
            .status()
            .expect("add deny ACL");
        assert!(deny.success());
        let file = File::open(&directory).expect("open directory");
        assert_eq!(descriptor_has_no_allow_acl(&file), Ok(true));
        assert_eq!(descriptor_has_no_extended_acl(&file), Ok(false));

        let allow = Command::new("/bin/chmod")
            .args(["+a", "everyone allow add_file"])
            .arg(&directory)
            .status()
            .expect("add allow ACL");
        assert!(allow.success());
        assert_eq!(descriptor_has_no_allow_acl(&file), Ok(false));
        let clear = Command::new("/bin/chmod")
            .arg("-N")
            .arg(&directory)
            .status()
            .expect("clear synthetic ACL");
        assert!(clear.success());
        fs::remove_dir(&directory).expect("remove synthetic directory");
    }
}
