//! PTY transport for connecting to ELM327 emulators on macOS/Linux.
//!
//! Unlike `SerialTransport`, this opens the PTY as a raw file descriptor
//! without serial-port-specific ioctls (tcsetattr/tcgetattr), which
//! macOS PTYs don't support. Used when `--emu` flag is set.

#[cfg(unix)]
mod inner {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::io::unix::AsyncFd;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};

    use crate::obd2::transport::Transport;
    use crate::obd2::types::Obd2Error;

    const READ_TIMEOUT: Duration = Duration::from_secs(5);

    /// Async wrapper around a raw PTY file descriptor.
    struct AsyncPty {
        inner: AsyncFd<OwnedFd>,
    }

    impl AsyncPty {
        fn new(fd: OwnedFd) -> io::Result<Self> {
            // Ensure non-blocking mode
            let raw = fd.as_raw_fd();
            unsafe {
                let flags = libc::fcntl(raw, libc::F_GETFL);
                if flags < 0 {
                    return Err(io::Error::last_os_error());
                }
                if flags & libc::O_NONBLOCK == 0 {
                    if libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
            Ok(Self {
                inner: AsyncFd::new(fd)?,
            })
        }
    }

    impl AsyncRead for AsyncPty {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            loop {
                let mut guard = match self.inner.poll_read_ready(cx) {
                    Poll::Ready(Ok(g)) => g,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                };

                let fd = self.inner.get_ref().as_raw_fd();
                let unfilled = buf.initialize_unfilled();
                let ret = unsafe {
                    libc::read(
                        fd,
                        unfilled.as_mut_ptr() as *mut libc::c_void,
                        unfilled.len(),
                    )
                };

                if ret < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        continue;
                    }
                    return Poll::Ready(Err(err));
                }

                buf.advance(ret as usize);
                return Poll::Ready(Ok(()));
            }
        }
    }

    impl AsyncWrite for AsyncPty {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            loop {
                let mut guard = match self.inner.poll_write_ready(cx) {
                    Poll::Ready(Ok(g)) => g,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                };

                let fd = self.inner.get_ref().as_raw_fd();
                let ret = unsafe {
                    libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len())
                };

                if ret < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        continue;
                    }
                    return Poll::Ready(Err(err));
                }

                return Poll::Ready(Ok(ret as usize));
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// Transport that opens a PTY device path without serial-port ioctls.
    pub struct PtyTransport {
        reader: BufReader<AsyncPty>,
    }

    impl PtyTransport {
        /// Open a PTY device path (e.g. `/dev/ttys003`) for async read/write.
        pub fn open(path: &str) -> Result<Self, Obd2Error> {
            use std::fs::OpenOptions;
            use std::os::fd::IntoRawFd;
            use std::os::unix::fs::OpenOptionsExt;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
                .open(path)
                .map_err(Obd2Error::Io)?;

            let fd = unsafe { OwnedFd::from_raw_fd(file.into_raw_fd()) };
            let pty = AsyncPty::new(fd).map_err(Obd2Error::Io)?;

            Ok(Self {
                reader: BufReader::new(pty),
            })
        }
    }

    #[async_trait]
    impl Transport for PtyTransport {
        async fn send(&mut self, data: &[u8]) -> Result<(), Obd2Error> {
            let data_str = String::from_utf8_lossy(data);
            tracing::debug!(target: "obd2::pty", "TX {} bytes: {:?}", data.len(), data_str.trim());
            let writer = self.reader.get_mut();
            writer.write_all(data).await.map_err(Obd2Error::Io)?;
            writer.flush().await.map_err(Obd2Error::Io)?;
            Ok(())
        }

        async fn read_line(&mut self) -> Result<String, Obd2Error> {
            let mut buf = Vec::new();
            loop {
                let byte = match tokio::time::timeout(READ_TIMEOUT, self.reader.read_u8()).await {
                    Ok(result) => result.map_err(Obd2Error::Io)?,
                    Err(_) => {
                        tracing::warn!(target: "obd2::pty", "Read timed out after {}s", READ_TIMEOUT.as_secs());
                        return Err(Obd2Error::Timeout);
                    }
                };
                match byte {
                    b'\r' => break,
                    b'\n' => continue,
                    b'>' => {
                        buf.push(b'>');
                        break;
                    }
                    other => buf.push(other),
                }
            }
            let line = String::from_utf8_lossy(&buf).to_string();
            if !line.is_empty() {
                tracing::debug!(target: "obd2::pty", "RX: {:?}", line);
            }
            Ok(line)
        }
    }
}

#[cfg(unix)]
pub use inner::PtyTransport;
