use std::os::fd::AsRawFd;

use genvm_common::io::FdPairStream;

#[test]
fn fd_pair_stream_rejects_the_same_fd_twice() {
    // owned by the `File`, so a regression cannot double-close a fd the harness needs
    let file = std::fs::File::open("/dev/null").unwrap();
    let fd = file.as_raw_fd();

    let err = match unsafe { FdPairStream::from_raw_fds(fd, fd) } {
        Ok(_) => panic!("the same fd was accepted for both directions"),
        Err(e) => e,
    };
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::InvalidInput,
        "unexpected error: {err}"
    );

    // ownership was not taken: the fd is still usable
    assert!(file.metadata().is_ok());
}
