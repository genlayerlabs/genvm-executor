use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

use genvm_common::util::buf_stream::BufStream;

const CAPACITY: usize = 8 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Read(Vec<u8>),
    Write(Vec<u8>),
    Flush,
}

#[derive(Default)]
struct Peer {
    incoming: VecDeque<u8>,
    events: Vec<Event>,
    failing_writes: usize,
    interrupted_reads: usize,
    write_attempts: usize,
    /// Most bytes a single write accepts; `None` accepts everything.
    write_limit: Option<usize>,
}

#[derive(Clone, Default)]
struct Mock(Rc<RefCell<Peer>>);

impl Mock {
    fn with_incoming(data: &[u8]) -> Self {
        let mock = Self::default();
        mock.0.borrow_mut().incoming.extend(data);
        mock
    }

    fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }
}

impl Read for Mock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut peer = self.0.borrow_mut();
        if peer.interrupted_reads > 0 {
            peer.interrupted_reads -= 1;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = buf.len().min(peer.incoming.len());
        let chunk: Vec<u8> = peer.incoming.drain(..n).collect();
        buf[..n].copy_from_slice(&chunk);
        peer.events.push(Event::Read(chunk));
        Ok(n)
    }
}

impl Write for Mock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut peer = self.0.borrow_mut();
        peer.write_attempts += 1;
        if peer.failing_writes > 0 {
            peer.failing_writes -= 1;
            return Err(io::Error::other("injected"));
        }
        let n = peer
            .write_limit
            .map_or(buf.len(), |limit| limit.min(buf.len()));
        peer.events.push(Event::Write(buf[..n].to_vec()));
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().events.push(Event::Flush);
        Ok(())
    }
}

#[test]
fn writes_wait_for_flush() {
    let mock = Mock::default();
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"ab").unwrap();
    stream.write_all(b"cd").unwrap();
    assert_eq!(mock.take_events(), []);
    stream.flush().unwrap();
    assert_eq!(
        mock.take_events(),
        [Event::Write(b"abcd".to_vec()), Event::Flush]
    );
}

#[test]
fn read_flushes_pending_writes_first() {
    let mock = Mock::with_incoming(b"xyz");
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"req").unwrap();
    let mut buf = [0; 3];
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"xyz");
    assert_eq!(
        mock.take_events(),
        [
            Event::Write(b"req".to_vec()),
            Event::Flush,
            Event::Read(b"xyz".to_vec()),
        ]
    );
}

#[test]
fn read_ahead_survives_writes() {
    let mock = Mock::with_incoming(b"0123456789");
    let mut stream = BufStream::new(mock.clone());
    let mut buf = [0; 4];
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"0123");
    stream.write_all(b"w").unwrap();
    let mut rest = [0; 6];
    stream.read_exact(&mut rest).unwrap();
    assert_eq!(&rest, b"456789");
    assert_eq!(
        mock.take_events(),
        [
            Event::Read(b"0123456789".to_vec()),
            Event::Write(b"w".to_vec()),
            Event::Flush,
        ]
    );
}

#[test]
fn drop_flushes_pending_writes() {
    let mock = Mock::default();
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"bye").unwrap();
    drop(stream);
    assert_eq!(mock.take_events(), [Event::Write(b"bye".to_vec())]);
}

#[test]
fn drop_ignores_write_errors() {
    let mock = Mock::default();
    mock.0.borrow_mut().failing_writes = 1;
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"lost").unwrap();
    drop(stream);
    assert_eq!(mock.0.borrow().write_attempts, 1);
    assert_eq!(mock.take_events(), []);
}

#[test]
fn drop_without_pending_writes_writes_nothing() {
    let mock = Mock::with_incoming(b"abc");
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"q").unwrap();
    let mut buf = [0; 1];
    stream.read_exact(&mut buf).unwrap();
    mock.take_events();
    drop(stream);
    assert_eq!(mock.0.borrow().write_attempts, 1);
    assert_eq!(mock.take_events(), []);
}

#[test]
fn partial_writes_are_resumed_by_flush() {
    let mock = Mock::default();
    mock.0.borrow_mut().write_limit = Some(2);
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"abcde").unwrap();
    stream.flush().unwrap();
    assert_eq!(
        mock.take_events(),
        [
            Event::Write(b"ab".to_vec()),
            Event::Write(b"cd".to_vec()),
            Event::Write(b"e".to_vec()),
            Event::Flush,
        ]
    );
}

#[test]
fn zero_length_write_surfaces_on_read() {
    let mock = Mock::with_incoming(b"r");
    mock.0.borrow_mut().write_limit = Some(0);
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"req").unwrap();
    let mut buf = [0; 1];
    let err = stream.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::WriteZero);
}

#[test]
fn read_error_after_flush_does_not_resend_writes() {
    let mock = Mock::with_incoming(b"r");
    mock.0.borrow_mut().interrupted_reads = 1;
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"req").unwrap();
    let mut buf = [0; 1];
    let err = stream.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    assert_eq!(stream.read(&mut buf).unwrap(), 1);
    assert_eq!(&buf, b"r");
    assert_eq!(
        mock.take_events(),
        [
            Event::Write(b"req".to_vec()),
            Event::Flush,
            Event::Read(b"r".to_vec()),
        ]
    );
}

#[test]
fn read_exact_retries_interrupted_reads() {
    let mock = Mock::with_incoming(b"abcd");
    mock.0.borrow_mut().interrupted_reads = 2;
    let mut stream = BufStream::new(mock.clone());
    let mut buf = [0; 4];
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"abcd");
}

#[test]
fn read_exact_hitting_eof_fails() {
    let mock = Mock::with_incoming(b"ab");
    let mut stream = BufStream::new(mock);
    let mut buf = [0; 4];
    let err = stream.read_exact(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn zero_length_read_and_write_from_read_ahead() {
    let mock = Mock::with_incoming(b"ab");
    let mut stream = BufStream::new(mock.clone());
    let mut one = [0; 1];
    stream.read_exact(&mut one).unwrap();
    mock.take_events();
    assert_eq!(stream.write(&[]).unwrap(), 0);
    assert_eq!(stream.read(&mut []).unwrap(), 0);
    stream.read_exact(&mut one).unwrap();
    assert_eq!(&one, b"b");
    assert_eq!(mock.take_events(), []);
}

#[test]
fn alternating_reads_and_writes_keep_order() {
    let incoming: Vec<u8> = (0..=255).collect();
    let mock = Mock::with_incoming(&incoming);
    let mut stream = BufStream::new(mock.clone());
    let mut got = Vec::new();
    for (i, len) in [3, 10, 1, 50, 192].into_iter().enumerate() {
        stream.write_all(&[i as u8]).unwrap();
        let mut buf = vec![0; len];
        stream.read_exact(&mut buf).unwrap();
        got.extend(buf);
    }
    assert_eq!(got, incoming);
    let writes: Vec<Event> = mock
        .take_events()
        .into_iter()
        .filter(|e| matches!(e, Event::Write(_)))
        .collect();
    let expected: Vec<Event> = (0..5).map(|i| Event::Write(vec![i])).collect();
    assert_eq!(writes, expected);
}

#[test]
fn write_of_capacity_bypasses_buffer() {
    let mock = Mock::default();
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"a").unwrap();
    let big = vec![7; CAPACITY];
    stream.write_all(&big).unwrap();
    assert_eq!(
        mock.take_events(),
        [Event::Write(b"a".to_vec()), Event::Write(big)]
    );
}

#[test]
fn write_overflowing_buffer_flushes_it() {
    let mock = Mock::default();
    let mut stream = BufStream::new(mock.clone());
    let first = vec![1; CAPACITY - 1];
    stream.write_all(&first).unwrap();
    stream.write_all(b"23").unwrap();
    assert_eq!(mock.take_events(), [Event::Write(first)]);
    stream.flush().unwrap();
    assert_eq!(
        mock.take_events(),
        [Event::Write(b"23".to_vec()), Event::Flush]
    );
}

#[test]
fn failed_flush_on_read_keeps_pending_writes() {
    let mock = Mock::with_incoming(b"r");
    mock.0.borrow_mut().failing_writes = 1;
    let mut stream = BufStream::new(mock.clone());
    stream.write_all(b"req").unwrap();
    let mut buf = [0; 1];
    let err = stream.read(&mut buf).unwrap_err();
    assert_eq!(err.to_string(), "injected");
    assert_eq!(mock.take_events(), []);
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"r");
    assert_eq!(
        mock.take_events(),
        [
            Event::Write(b"req".to_vec()),
            Event::Flush,
            Event::Read(b"r".to_vec()),
        ]
    );
}

#[test]
fn read_of_capacity_bypasses_empty_buffer() {
    let data = vec![3; CAPACITY + 1];
    let mock = Mock::with_incoming(&data);
    let mut stream = BufStream::new(mock.clone());
    let mut buf = vec![0; CAPACITY];
    assert_eq!(stream.read(&mut buf).unwrap(), CAPACITY);
    assert_eq!(mock.take_events(), [Event::Read(data[..CAPACITY].to_vec())]);
}

#[test]
fn read_below_capacity_fills_buffer() {
    let data = vec![3; CAPACITY + 1];
    let mock = Mock::with_incoming(&data);
    let mut stream = BufStream::new(mock.clone());
    let mut buf = vec![0; CAPACITY - 1];
    assert_eq!(stream.read(&mut buf).unwrap(), CAPACITY - 1);
    assert_eq!(mock.take_events(), [Event::Read(data[..CAPACITY].to_vec())]);
}

#[test]
fn large_read_drains_read_ahead_first() {
    let data: Vec<u8> = (0..CAPACITY * 2).map(|i| i as u8).collect();
    let mock = Mock::with_incoming(&data);
    let mut stream = BufStream::new(mock.clone());
    let mut one = [0; 1];
    stream.read_exact(&mut one).unwrap();
    mock.take_events();
    let mut buf = vec![0; CAPACITY];
    assert_eq!(stream.read(&mut buf).unwrap(), CAPACITY - 1);
    assert_eq!(buf[..CAPACITY - 1], data[1..CAPACITY]);
    assert_eq!(mock.take_events(), []);
}

#[test]
fn read_returns_buffered_bytes_without_reading_stream() {
    let mock = Mock::with_incoming(b"abcdefgh");
    let mut stream = BufStream::new(mock.clone());
    let mut one = [0; 1];
    stream.read_exact(&mut one).unwrap();
    mock.take_events();
    mock.0.borrow_mut().incoming.extend(b"more");
    stream.write_all(b"w").unwrap();
    let mut buf = [0; 16];
    assert_eq!(stream.read(&mut buf).unwrap(), 7);
    assert_eq!(&buf[..7], b"bcdefgh");
    assert_eq!(
        mock.take_events(),
        [Event::Write(b"w".to_vec()), Event::Flush]
    );
}

#[test]
fn read_at_eof_returns_zero() {
    let mock = Mock::default();
    let mut stream = BufStream::new(mock);
    let mut buf = [0; 4];
    assert_eq!(stream.read(&mut buf).unwrap(), 0);
}

#[test]
fn unix_socket_round_trip() {
    let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
    let peer = std::thread::spawn(move || {
        let mut theirs = BufStream::new(theirs);
        let mut req = [0; 4];
        theirs.read_exact(&mut req).unwrap();
        theirs
            .write_all(&u32::from_le_bytes(req).wrapping_add(1).to_le_bytes())
            .unwrap();
    });
    let mut ours = BufStream::new(ours);
    ours.write_all(&41u32.to_le_bytes()).unwrap();
    let mut resp = [0; 4];
    ours.read_exact(&mut resp).unwrap();
    assert_eq!(u32::from_le_bytes(resp), 42);
    peer.join().unwrap();
}

#[test]
fn unix_socket_sees_nothing_before_flush() {
    let (ours, mut theirs) = std::os::unix::net::UnixStream::pair().unwrap();
    theirs.set_nonblocking(true).unwrap();
    let mut ours = BufStream::new(ours);
    ours.write_all(b"held").unwrap();
    let mut buf = [0; 4];
    let err = theirs.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
    ours.flush().unwrap();
    theirs.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"held");
}
