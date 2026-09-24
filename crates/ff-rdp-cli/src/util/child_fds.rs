//! Darwin's non-atomic pipe/CLOEXEC setup can leak a caller's capture pipe into
//! Firefox. Keep only the configured standard streams across the browser exec.

use std::{io, os::unix::process::CommandExt, process::Command};

#[allow(unsafe_code)]
pub(crate) fn exclude_inherited(command: &mut Command) {
    // SAFETY: the callback uses stack storage and Darwin syscalls only. Marking
    // rather than closing leaves std's private exec-error writer usable on error.
    unsafe { command.pre_exec(mark_extra_fds) };
}

#[allow(unsafe_code)]
fn mark_extra_fds() -> io::Result<()> {
    // SAFETY: static terminated path, valid open flags, no allocation/stdio lock.
    let directory = unsafe {
        libc::open(
            c"/dev/fd".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if directory < 0 {
        return Err(io::Error::last_os_error());
    }
    let result = mark_directory(directory);
    // SAFETY: this descriptor was opened above and is owned only by this child.
    let closed = unsafe { libc::close(directory) };
    result?;
    if closed < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code)]
fn mark_directory(directory: libc::c_int) -> io::Result<()> {
    // XNU bsd/kern/syscalls.master: getdirentries64 is syscall344 on Darwin.
    // Unlike readdir, this syscall needs no DIR allocation or libc directory lock.
    const GETDIRENTRIES64: libc::c_int = 344;
    const NAME: usize = std::mem::offset_of!(libc::dirent, d_name);
    const RECORD_LEN: usize = std::mem::offset_of!(libc::dirent, d_reclen);
    const NAME_LEN: usize = std::mem::offset_of!(libc::dirent, d_namlen);
    let invalid = || io::Error::from_raw_os_error(libc::EIO);
    let mut buffer = [0_u8; 4096];
    let mut base: libc::off_t = 0;
    loop {
        // SAFETY: writable stack buffer and offset, owned open directory fd.
        let count = unsafe {
            libc::syscall(
                GETDIRENTRIES64,
                directory,
                buffer.as_mut_ptr(),
                buffer.len(),
                &raw mut base,
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        if count == 0 {
            return Ok(());
        }
        let count = usize::try_from(count).map_err(|_| invalid())?;
        let bytes = buffer.get(..count).ok_or_else(invalid)?;
        let mut cursor = 0;
        while cursor < bytes.len() {
            let entry = &bytes[cursor..];
            if entry.len() < NAME {
                return Err(invalid());
            }
            let length = usize::from(u16::from_ne_bytes([
                entry[RECORD_LEN],
                entry[RECORD_LEN + 1],
            ]));
            let name_length =
                usize::from(u16::from_ne_bytes([entry[NAME_LEN], entry[NAME_LEN + 1]]));
            if length <= NAME || length > entry.len() || name_length >= length - NAME {
                return Err(invalid());
            }
            let name = &entry[NAME..NAME + name_length];
            if entry[NAME + name_length] != 0 {
                return Err(invalid());
            }
            if name != b"." && name != b".." {
                let fd = name
                    .iter()
                    .try_fold(0_i32, |number, byte| {
                        if byte.is_ascii_digit() {
                            number.checked_mul(10)?.checked_add(i32::from(*byte - b'0'))
                        } else {
                            None
                        }
                    })
                    .filter(|_| !name.is_empty())
                    .ok_or_else(invalid)?;
                if fd >= 3 && fd != directory {
                    // SAFETY: fcntl acts on an enumerated child fd; no fd is closed
                    // during enumeration. Errors abort spawn via std's error pipe.
                    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
                    if flags < 0
                        || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
            cursor += length;
        }
    }
}

#[cfg(test)]
mod tests;
