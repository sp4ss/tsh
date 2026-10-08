use anyhow::{bail, Context, Result};
use quinn::{RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAGIC: &[u8; 4] = b"TSH1";
const MAX_PATH: u32 = 16 * 1024;
const MAX_ERROR: u32 = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Put = 1,
    Get = 2,
}

impl Op {
    fn from_u8(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Put),
            2 => Ok(Self::Get),
            _ => bail!("unknown operation: {value}"),
        }
    }
}

#[derive(Debug)]
pub struct Request {
    pub op: Op,
    pub size: u64,
    pub path: String,
}

#[derive(Debug)]
pub enum Response {
    Ok { size: u64 },
    Err { message: String },
}

pub async fn write_request(send: &mut SendStream, request: &Request) -> Result<()> {
    let path = request.path.as_bytes();

    if path.is_empty() {
        bail!("remote path is empty");
    }

    if path.len() > MAX_PATH as usize {
        bail!("remote path exceeds {} bytes", MAX_PATH);
    }

    send.write_all(MAGIC).await?;
    send.write_u8(request.op as u8).await?;
    send.write_u32(path.len() as u32).await?;
    send.write_u64(request.size).await?;
    send.write_all(path).await?;
    Ok(())
}

pub async fn read_request(recv: &mut RecvStream) -> Result<Request> {
    read_magic(recv).await?;

    let op = Op::from_u8(recv.read_u8().await?)?;
    let path_len = recv.read_u32().await?;

    if path_len == 0 {
        bail!("empty remote path");
    }

    if path_len > MAX_PATH {
        bail!("remote path exceeds {} bytes", MAX_PATH);
    }

    let size = recv.read_u64().await?;

    let mut path = vec![0u8; path_len as usize];
    recv.read_exact(&mut path).await?;

    let path = String::from_utf8(path).context("remote path is not valid UTF-8")?;

    Ok(Request { op, size, path })
}

pub async fn write_ok(send: &mut SendStream, size: u64) -> Result<()> {
    send.write_u8(0).await?;
    send.write_u64(size).await?;
    Ok(())
}

pub async fn write_error(send: &mut SendStream, message: &str) -> Result<()> {
    let bytes = message.as_bytes();
    let bytes = &bytes[..bytes.len().min(MAX_ERROR as usize)];

    send.write_u8(1).await?;
    send.write_u32(bytes.len() as u32).await?;
    send.write_all(bytes).await?;
    Ok(())
}

pub async fn read_response(recv: &mut RecvStream) -> Result<Response> {
    let status = recv.read_u8().await?;

    match status {
        0 => {
            let size = recv.read_u64().await?;
            Ok(Response::Ok { size })
        }
        1 => {
            let len = recv.read_u32().await?;

            if len > MAX_ERROR {
                bail!("server error message too large");
            }

            let mut bytes = vec![0u8; len as usize];
            recv.read_exact(&mut bytes).await?;

            let message = String::from_utf8(bytes).context("server error is not valid UTF-8")?;

            Ok(Response::Err { message })
        }
        _ => bail!("unknown response status: {status}"),
    }
}

async fn read_magic(recv: &mut RecvStream) -> Result<()> {
    let mut magic = [0u8; 4];
    recv.read_exact(&mut magic).await?;

    if &magic != MAGIC {
        bail!("invalid tsh protocol magic");
    }

    Ok(())
}
