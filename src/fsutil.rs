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
mod tests {
    use super::*;

    #[test]
    fn read_capped_reports_overflow_without_returning_it() {
        assert_eq!(
            read_capped(&b"abc"[..], 3).unwrap(),
            (b"abc".to_vec(), false)
        );
        assert_eq!(
            read_capped(&b"abcd"[..], 3).unwrap(),
            (b"abc".to_vec(), true)
        );
        assert_eq!(read_capped(&b""[..], 3).unwrap(), (Vec::new(), false));
    }

    #[test]
    fn symlinks_are_refused_and_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(open_no_follow(&target).is_ok());
        assert!(is_symlink_error(&open_no_follow(&link).unwrap_err()));
        assert!(!is_symlink_error(
            &open_no_follow(&dir.path().join("missing")).unwrap_err()
        ));
    }
}
