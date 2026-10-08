use anyhow::{Context, Result};
use quinn::{RecvStream, SendStream};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

const BUF_SIZE: usize = 1024 * 1024;

pub async fn file_size(path: &Path) -> Result<u64> {
    let metadata = fs::metadata(path)
        .await
        .with_context(|| format!("stat {}", path.display()))?;

    if !metadata.is_file() {
        anyhow::bail!("not a regular file: {}", path.display());
    }

    Ok(metadata.len())
}

pub async fn send_file(send: &mut SendStream, path: &Path, size: u64) -> Result<()> {
    let mut file = fs::File::open(path)
        .await
        .with_context(|| format!("open {}", path.display()))?;

    let mut buf = vec![0u8; BUF_SIZE];
    let mut remaining = size;

    while remaining > 0 {
        let want = remaining.min(BUF_SIZE as u64) as usize;
        let n = file.read(&mut buf[..want]).await?;

        if n == 0 {
            anyhow::bail!("file changed while reading: {}", path.display());
        }

        send.write_all(&buf[..n]).await?;
        remaining -= n as u64;
    }

    Ok(())
}

pub async fn receive_file(recv: &mut RecvStream, size: u64, destination: &Path) -> Result<()> {
    let temporary = temporary_path(destination);

    let result = receive_to_temp(recv, size, &temporary).await;

    match result {
        Ok(()) => {
            fs::rename(&temporary, destination)
                .await
                .with_context(|| format!("replace destination {}", destination.display()))?;

            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary).await;
            Err(error)
        }
    }
}

async fn receive_to_temp(recv: &mut RecvStream, size: u64, temporary: &Path) -> Result<()> {
    let mut file = fs::File::create(temporary)
        .await
        .with_context(|| format!("create {}", temporary.display()))?;

    let mut buf = vec![0u8; BUF_SIZE];
    let mut remaining = size;

    while remaining > 0 {
        let want = remaining.min(BUF_SIZE as u64) as usize;
        let n = match recv.read(&mut buf[..want]).await? {
            Some(n) if n > 0 => n,
            _ => anyhow::bail!("unexpected end of file transfer"),
        };

        file.write_all(&buf[..n]).await?;
        remaining -= n as u64;
    }

    if recv.read(&mut [0u8; 1]).await?.is_some() {
        anyhow::bail!("unexpected bytes after file payload");
    }

    file.sync_all().await?;

    Ok(())
}

fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let name = destination
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("file");

    let temporary_name = format!(".{name}.tsh-part-{}-{nonce}", std::process::id());

    destination.with_file_name(temporary_name)
}
