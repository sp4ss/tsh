use anyhow::{Context, Result};
use quinn::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        RootCertStore,
    },
    ClientConfig, Endpoint, ServerConfig,
};
use sha2::{Digest, Sha256};
use std::{fs, net::SocketAddr, path::Path, sync::Arc};

const CERT_FILE: &str = "server.der";
const KEY_FILE: &str = "server.key";

pub fn server_endpoint(bind_addr: SocketAddr, state_dir: &Path) -> Result<(Endpoint, String)> {
    fs::create_dir_all(state_dir)?;

    let cert_path = state_dir.join(CERT_FILE);
    let key_path = state_dir.join(KEY_FILE);

    let (cert_der, key_der): (Vec<u8>, Vec<u8>) = match (cert_path.exists(), key_path.exists()) {
        (true, true) => (
            fs::read(&cert_path).context("read server cert ")?,
            fs::read(&key_path).context("read server private key ")?,
        ),
        (false, false) => {
            let key = rcgen::generate_simple_self_signed(vec!["tsh".into()])
                .context("generate server certificate")?;

            let cert_der = key.cert.der().to_vec();
            let key_der = key.key_pair.serialize_der();

            fs::write(&cert_path, &cert_der).context("write server cert")?;
            write_private_key(&key_path, &key_der).context("write server private keys ")?;
            (cert_der, key_der)
        }
        _ => anyhow::bail!(
            "server certificate/key state is incomplete: {}",
            state_dir.display()
        ),
    };

    let fingerprint = fingerprint(&cert_der);
    let cert = CertificateDer::from(cert_der);
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

    let server_config =
        ServerConfig::with_single_cert(vec![cert], key).context("build QUIC server TLS confgis")?;

    let endpoint = Endpoint::server(server_config, bind_addr).context("bind QUIC endpoint")?;

    Ok((endpoint, fingerprint))
}

pub fn client_endpoint(ca_path: &Path) -> Result<Endpoint> {
    let cert = CertificateDer::from(
        fs::read(ca_path)
            .with_context(|| format!("Reading trusted Certificate {}", ca_path.display()))?,
    );
    let mut roots = RootCertStore::empty();
    roots
        .add(cert)
        .context("add server certificate to trust store")?;

    let config = ClientConfig::with_root_certificates(Arc::new(roots))
        .context("Build QUIC client TLS Config")?;

    let mut endpoint = Endpoint::client("[::]:0".parse()?).context("creating client")?;

    endpoint.set_default_client_config(config);
    Ok(endpoint)
}

pub fn fingerprint(cert_der: &[u8]) -> String {
    hex::encode(Sha256::digest(cert_der))
}

#[cfg(unix)]
fn write_private_key(path: &Path, data: &[u8]) -> Result<()> {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;

    file.write_all(data)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_key(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data)?;
    Ok(())
}
