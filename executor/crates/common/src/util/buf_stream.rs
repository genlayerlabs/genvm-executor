use std::io::{self, BufWriter, Read, Write};

const CAPACITY: usize = 8 * 1024;

/// Buffers both directions of a duplex stream. A read first flushes pending
/// writes, and read-ahead survives later writes. Dropping flushes pending
/// writes and ignores errors, like [`BufWriter`].
pub struct BufStream<S: Write> {
    inner: BufWriter<S>,
    read_buf: Box<[u8]>,
    read_pos: usize,
    read_end: usize,
}

impl<S: Write> BufStream<S> {
    pub fn new(stream: S) -> Self {
        Self {
            inner: BufWriter::with_capacity(CAPACITY, stream),
            read_buf: vec![0; CAPACITY].into_boxed_slice(),
            read_pos: 0,
            read_end: 0,
        }
    }
}

impl<S: Read + Write> Read for BufStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.inner.buffer().is_empty() {
            self.inner.flush()?;
        }
        if self.read_pos == self.read_end {
            if buf.len() >= self.read_buf.len() {
                return self.inner.get_mut().read(buf);
            }
            self.read_end = self.inner.get_mut().read(&mut self.read_buf)?;
            self.read_pos = 0;
        }
        let n = buf.len().min(self.read_end - self.read_pos);
        buf[..n].copy_from_slice(&self.read_buf[self.read_pos..self.read_pos + n]);
        self.read_pos += n;
        Ok(n)
    }
}

impl<S: Write> Write for BufStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
