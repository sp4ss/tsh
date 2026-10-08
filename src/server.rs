use crate::{
    protocol::{self, Op},
    tls, transfer,
};
use anyhow::{Context, Result};
use quinn::{Connection, Endpoint};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
};

pub async fn run(port: u16, state_dir: &Path) -> Result<()> {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);

    let (endpoint, fingerprint) = tls::server_endpoint(bind_addr, state_dir)?;

    println!("tsh server listening on {}", endpoint.local_addr()?);
    println!("certificate sha256:{}", fingerprint);

    tokio::select! {
        result = accept_loop(endpoint) => result?,
        _ = tokio::signal::ctrl_c() => {
            println!("shutting down");
        }
    }

    Ok(())
}

async fn accept_loop(endpoint: Endpoint) -> Result<()> {
    while let Some(connecting) = endpoint.accept().await {
        tokio::spawn(async move {
            let result = async {
                let connection = connecting.await.context("QUIC handshake")?;

                println!("connection from {}", connection.remote_address());

                serve_connection(connection).await
            }
            .await;

            if let Err(error) = result {
                eprintln!("connection error: {error:#}");
            }
        });
    }

    Ok(())
}

pub(crate) async fn serve_connection(connection: Connection) -> Result<()> {
    loop {
        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(streams) => streams,
            Err(_) => return Ok(()),
        };

        if let Err(error) = handle_stream(&mut send, &mut recv).await {
            let _ = protocol::write_error(&mut send, &error.to_string()).await;

            let _ = send.finish();

            eprintln!("request error: {error:#}");
        } else {
            let _ = send.finish();
        }
    }
}

async fn handle_stream(send: &mut quinn::SendStream, recv: &mut quinn::RecvStream) -> Result<()> {
    let request = protocol::read_request(recv).await?;

    let path = Path::new(&request.path);

    match request.op {
        Op::Put => {
            transfer::receive_file(recv, request.size, path).await?;

            protocol::write_ok(send, request.size).await?;

            println!("received {} bytes -> {}", request.size, path.display());
        }

        Op::Get => {
            let size = transfer::file_size(path)
                .await
                .with_context(|| format!("stat {}", path.display()))?;

            protocol::write_ok(send, size).await?;

            transfer::send_file(send, path, size).await?;

            println!("sent {} bytes <- {}", size, path.display());
        }
    }

    Ok(())
}
