use super::BandwidthLimiter;
use crate::transport::{boxed_stream, DynStream};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

type BoxWait = Pin<Box<dyn Future<Output = ()> + Send>>;

const MAX_IO_CHUNK: usize = 256 * 1024;

enum ReadPhase {
    Idle,
    Wait {
        data: Vec<u8>,
        off: usize,
        wait: BoxWait,
    },
    Deliver {
        data: Vec<u8>,
        off: usize,
    },
}

enum WritePhase {
    Idle,
    Wait { wait: BoxWait, chunk: usize },
    Write { chunk: usize, written: usize },
}

pub struct LimitedStream<S> {
    inner: S,
    limiter: Arc<BandwidthLimiter>,
    read_phase: ReadPhase,
    write_pending: Vec<u8>,
    write_report: usize,
    write_phase: WritePhase,
}

impl<S> LimitedStream<S> {
    pub fn new(inner: S, limiter: Arc<BandwidthLimiter>) -> Self {
        Self {
            inner,
            limiter,
            read_phase: ReadPhase::Idle,
            write_pending: Vec::new(),
            write_report: 0,
            write_phase: WritePhase::Idle,
        }
    }

    fn io_chunk_cap(&self) -> usize {
        self.limiter.burst().clamp(1, MAX_IO_CHUNK)
    }
}

pub fn maybe_limit(stream: DynStream, limiter: Option<Arc<BandwidthLimiter>>) -> DynStream {
    match limiter {
        Some(l) => boxed_stream(LimitedStream::new(stream, l)),
        None => stream,
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for LimitedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            match self.read_phase {
                ReadPhase::Idle => {
                    let want = buf.remaining().min(self.io_chunk_cap());
                    if want == 0 {
                        return Poll::Ready(Ok(()));
                    }

                    if self.limiter.try_wait_n(want) {
                        let unfilled = buf.initialize_unfilled();
                        let ncap = want.min(unfilled.len());
                        let mut tmp = ReadBuf::new(&mut unfilled[..ncap]);
                        match Pin::new(&mut self.inner).poll_read(cx, &mut tmp) {
                            Poll::Pending => {
                                self.limiter.return_n(want);
                                return Poll::Pending;
                            }
                            Poll::Ready(Err(e)) => {
                                self.limiter.return_n(want);
                                return Poll::Ready(Err(e));
                            }
                            Poll::Ready(Ok(())) => {
                                let n = tmp.filled().len();
                                if n < want {
                                    self.limiter.return_n(want - n);
                                }
                                buf.advance(n);
                                return Poll::Ready(Ok(()));
                            }
                        }
                    }

                    let mut tmp_storage = vec![0u8; want];
                    let mut tmp = ReadBuf::new(&mut tmp_storage);
                    match Pin::new(&mut self.inner).poll_read(cx, &mut tmp) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Ready(Ok(())) => {
                            let n = tmp.filled().len();
                            if n == 0 {
                                return Poll::Ready(Ok(()));
                            }
                            tmp_storage.truncate(n);
                            let limiter = Arc::clone(&self.limiter);
                            if limiter.try_wait_n(n) {
                                buf.put_slice(&tmp_storage);
                                return Poll::Ready(Ok(()));
                            }
                            let wait = Box::pin(async move { limiter.wait_n(n).await });
                            self.read_phase = ReadPhase::Wait {
                                data: tmp_storage,
                                off: 0,
                                wait,
                            };
                        }
                    }
                }
                ReadPhase::Wait { .. } => {
                    let phase = std::mem::replace(&mut self.read_phase, ReadPhase::Idle);
                    let ReadPhase::Wait {
                        data,
                        off,
                        mut wait,
                    } = phase
                    else {
                        unreachable!()
                    };
                    match wait.as_mut().poll(cx) {
                        Poll::Pending => {
                            self.read_phase = ReadPhase::Wait { data, off, wait };
                            return Poll::Pending;
                        }
                        Poll::Ready(()) => {
                            self.read_phase = ReadPhase::Deliver { data, off };
                        }
                    }
                }
                ReadPhase::Deliver { .. } => {
                    let phase = std::mem::replace(&mut self.read_phase, ReadPhase::Idle);
                    let ReadPhase::Deliver { data, mut off } = phase else {
                        unreachable!()
                    };
                    let avail = &data[off..];
                    let n = avail.len().min(buf.remaining());
                    buf.put_slice(&avail[..n]);
                    off += n;
                    if off < data.len() {
                        self.read_phase = ReadPhase::Deliver { data, off };
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for LimitedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if matches!(self.write_phase, WritePhase::Idle) && self.write_pending.is_empty() {
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }

            let take = buf.len().min(self.io_chunk_cap());
            self.write_report = take;
            self.write_pending.clear();
            let need = take.saturating_sub(self.write_pending.capacity());
            if need > 0 {
                self.write_pending.reserve(need);
            }
            self.write_pending.extend_from_slice(&buf[..take]);
        }

        loop {
            if self.write_pending.is_empty() && matches!(self.write_phase, WritePhase::Idle) {
                let n = self.write_report;
                self.write_report = 0;
                return Poll::Ready(Ok(n));
            }

            match self.write_phase {
                WritePhase::Idle => {
                    let chunk = self.write_pending.len().min(self.io_chunk_cap());
                    let limiter = Arc::clone(&self.limiter);
                    if limiter.try_wait_n(chunk) {
                        self.write_phase = WritePhase::Write { chunk, written: 0 };
                    } else {
                        let wait = Box::pin(async move { limiter.wait_n(chunk).await });
                        self.write_phase = WritePhase::Wait { wait, chunk };
                    }
                }
                WritePhase::Wait { .. } => {
                    let phase = std::mem::replace(&mut self.write_phase, WritePhase::Idle);
                    let WritePhase::Wait { mut wait, chunk } = phase else {
                        unreachable!()
                    };
                    match wait.as_mut().poll(cx) {
                        Poll::Pending => {
                            self.write_phase = WritePhase::Wait { wait, chunk };
                            return Poll::Pending;
                        }
                        Poll::Ready(()) => {
                            self.write_phase = WritePhase::Write { chunk, written: 0 };
                        }
                    }
                }
                WritePhase::Write { chunk, written } => {
                    let this = &mut *self;
                    let chunk = chunk.min(this.write_pending.len());
                    if written >= chunk {
                        this.write_pending.drain(..chunk);
                        this.write_phase = WritePhase::Idle;
                        continue;
                    }
                    match Pin::new(&mut this.inner)
                        .poll_write(cx, &this.write_pending[written..chunk])
                    {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => {
                            this.write_pending.clear();
                            this.write_phase = WritePhase::Idle;
                            this.write_report = 0;
                            return Poll::Ready(Err(e));
                        }
                        Poll::Ready(Ok(0)) => {
                            this.write_pending.clear();
                            this.write_phase = WritePhase::Idle;
                            this.write_report = 0;
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "write zero in limited stream",
                            )));
                        }
                        Poll::Ready(Ok(n)) => {
                            let written = written + n;
                            if written >= chunk {
                                this.write_pending.drain(..chunk);
                                this.write_phase = WritePhase::Idle;
                            } else {
                                this.write_phase = WritePhase::Write { chunk, written };
                            }
                        }
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        while !self.write_pending.is_empty() || !matches!(self.write_phase, WritePhase::Idle) {
            match self.as_mut().poll_write(cx, &[]) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(_)) => {}
            }
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match self.as_mut().poll_flush(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(())) => Pin::new(&mut self.inner).poll_shutdown(cx),
        }
    }
}
