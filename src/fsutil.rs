//! Small filesystem helpers shared by credential and mutation-file handling.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use rustix::fs::OFlags;
use rustix::io::Errno;

/// Open for reading without following a symbolic link at the final component.
/// Opening never blocks, so a FIFO is returned (and can then be rejected as
/// not a regular file) instead of hanging until someone writes to it.
pub fn open_no_follow(path: &Path) -> io::Result<File> {
    let flags = OFlags::NOFOLLOW | OFlags::NONBLOCK;
    OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
}

/// Whether an `open_no_follow` failure means the path is a symbolic link.
pub fn is_symlink_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(Errno::LOOP.raw_os_error())
}

/// Read at most `limit` bytes. The flag reports whether more data followed,
/// without ever buffering more than one byte past the limit.
pub fn read_capped(reader: impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let more = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, more))
}

#[cfg(test)]
mod tests;
