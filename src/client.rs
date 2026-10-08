use crate::{
    protocol::{self, Op, Request, Response},
    tls, transfer,
};
use anyhow::{bail, Context, Result};
use quinn::{Connection, Endpoint};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

const REMOTE_PREFIX: &str = "tsh://";

pub fn is_remote(path: &str) -> bool {
    path.starts_with(REMOTE_PREFIX)
}

pub async fn connect(ca_path: &Path, target: &str) -> Result<(Endpoint, Connection)> {
    let endpoint = tls::client_endpoint(ca_path)?;

    let addr = resolve_target(target).await?;

    let connection = endpoint
        .connect(addr, "tsh")
        .context("start QUIC connection")?
        .await
        .context("complete QUIC connection")?;

    Ok((endpoint, connection))
}

pub async fn upload(ca_path: &Path, local: &Path, remote: &str) -> Result<u64> {
    let (target, remote_path) = parse_remote(remote)?;

    let (endpoint, connection) = connect(ca_path, &target).await?;

    let result = upload_on(&connection, local, &remote_path).await;

    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    result
}

pub async fn download(ca_path: &Path, remote: &str, local: &Path) -> Result<u64> {
    let (target, remote_path) = parse_remote(remote)?;

    let (endpoint, connection) = connect(ca_path, &target).await?;

    let result = download_on(&connection, &remote_path, local).await;

    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    result
}

pub(crate) async fn upload_on(connection: &Connection, local: &Path, remote: &Path) -> Result<u64> {
    let size = transfer::file_size(local).await?;

    let (mut send, mut recv) = connection.open_bi().await?;

    let path = remote.to_str().context("remote path is not UTF-8")?;

    protocol::write_request(
        &mut send,
        &Request {
            op: Op::Put,
            size,
            path: path.to_owned(),
        },
    )
    .await?;

    transfer::send_file(&mut send, local, size).await?;
    send.finish()?;

    match protocol::read_response(&mut recv).await? {
        Response::Ok { size: remote_size } if remote_size == size => {
            println!(
                "uploaded {} -> {} ({} bytes)",
                local.display(),
                remote.display(),
                size
            );

            Ok(size)
        }

        Response::Ok { size: remote_size } => {
            bail!("server reported {} bytes, expected {}", remote_size, size)
        }

        Response::Err { message } => {
            bail!("server: {message}")
        }
    }
}

pub(crate) async fn download_on(
    connection: &Connection,
    remote: &Path,
    local: &Path,
) -> Result<u64> {
    let (mut send, mut recv) = connection.open_bi().await?;

    let path = remote.to_str().context("remote path is not UTF-8")?;

    protocol::write_request(
        &mut send,
        &Request {
            op: Op::Get,
            size: 0,
            path: path.to_owned(),
        },
    )
    .await?;

    send.finish()?;

    let size = match protocol::read_response(&mut recv).await? {
        Response::Ok { size } => size,
        Response::Err { message } => {
            bail!("server: {message}")
        }
    };

    transfer::receive_file(&mut recv, size, local).await?;

    println!(
        "downloaded {} -> {} ({} bytes)",
        remote.display(),
        local.display(),
        size
    );

    Ok(size)
}

async fn resolve_target(target: &str) -> Result<SocketAddr> {
    let mut addresses = tokio::net::lookup_host(target)
        .await
        .with_context(|| format!("resolve {target}"))?;

    addresses
        .next()
        .with_context(|| format!("no addresses for {target}"))
}

fn parse_remote(spec: &str) -> Result<(String, PathBuf)> {
    let rest = spec
        .strip_prefix(REMOTE_PREFIX)
        .context("not a tsh remote path")?;

    let slash = rest
        .find('/')
        .context("remote path must be tsh://HOST:PORT/PATH")?;

    let target = &rest[..slash];
    let path = &rest[slash..];

    if target.is_empty() {
        bail!("remote target is empty");
    }

    if path == "/" {
        bail!("remote path must name a file");
    }

    if !path.starts_with('/') {
        bail!("remote path must be absolute");
    }

    Ok((target.to_owned(), PathBuf::from(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_path() {
        let (target, path) = parse_remote("tsh://127.0.0.1:8080/tmp/a.txt").unwrap();

        assert_eq!(target, "127.0.0.1:8080");
        assert_eq!(path, PathBuf::from("/tmp/a.txt"));
    }

    #[test]
    fn rejects_relative_remote_path() {
        assert!(parse_remote("tsh://127.0.0.1:8080/a").is_ok());

        assert!(parse_remote("tsh://127.0.0.1:8080").is_err());
    }
}
