use std::fs;

use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::infrastructure::filesystem::ensure_dir_writable;
use tempfile::TempDir;

#[test]
fn ensure_dir_writable_succeeds_on_fresh_tempdir() {
    let tmp = TempDir::new().expect("tempdir");
    let target = tmp.path().join("rustory-storage");

    ensure_dir_writable(&target).expect("fresh writable path must succeed");
    assert!(target.is_dir(), "target directory must exist after call");
}

#[test]
fn ensure_dir_writable_fails_when_parent_is_a_regular_file() {
    // Exercises the public API from outside the crate: the Rust core must
    // refuse to fabricate storage under a path that cannot become a directory
    // and must surface a normalized [`AppError`].
    //
    // This scenario is OS- and user-agnostic: even root cannot create a
    // subdirectory under a regular file, so the test stays deterministic in
    // CI containers that happen to run as root.
    let tmp = TempDir::new().expect("tempdir");
    let blocker = tmp.path().join("blocker");
    fs::write(&blocker, b"not-a-directory").expect("write blocker file");

    let target = blocker.join("storage");
    let err = ensure_dir_writable(&target).expect_err("must fail on non-directory parent");

    assert_eq!(err.code, AppErrorCode::LocalStorageUnavailable);
    assert!(
        err.user_action.is_some(),
        "user_action must be populated so the UI can render the next step"
    );
}
