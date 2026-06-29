use crate::{Args, Error};
use rkvm_input::windows::writer_simple::WriterWindowsSimple;
use crate::stream::RkvmStream;

use std::marker::PhantomPinned;
use std::pin::{Pin, pin};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufStream};
use tokio::net::windows::named_pipe::{ ClientOptions, NamedPipeClient };
use tokio::time::{Duration, sleep};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

pub async fn init_stream(args: &Args) -> Result<PipeStream,Error> {
    let pipe = loop {
        match ClientOptions::new().open(args.config_path.clone())  {
            Ok(client) => break client,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => (),
            Err(e) => return Err(Error::Io(e)),
        }

        sleep(Duration::from_millis(50)).await;
    };
    Ok(PipeStream::new(pipe))
}

pub fn init_writers() -> WriterWindowsSimple {
    WriterWindowsSimple::new()
}

pub struct PipeStream {
    pipe: BufStream<NamedPipeClient>,
}

impl PipeStream {
    pub fn new(pipe: NamedPipeClient) -> Self {
        PipeStream { pipe: BufStream::with_capacity(1024, 1024, pipe), }
    }
}

impl AsyncRead for PipeStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.pipe).poll_read(cx, buf)
    }
}

impl AsyncWrite for PipeStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut this.pipe).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.pipe).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.pipe).poll_shutdown(cx)
    }
}

