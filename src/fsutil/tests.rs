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
