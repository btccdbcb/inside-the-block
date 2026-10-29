//! QUIC client for the TPU: ALPN `solana-tpu`, self-signed ed25519 client certificate, one unidirectional
//! stream per transaction. Writing bytes needs a `LiveAuthorization`; a dry run only connects.

use crate::auth::LiveAuthorization;
use crate::util::mono_ns;
use anyhow::{anyhow, Context, Result};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub const ALPN: &[u8] = b"solana-tpu";

/// Validators present self-signed certificates; TPU traffic is authenticated by the transactions' own signatures.
#[derive(Debug)]
struct AcceptAnyServerCert(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub struct QuicClient {
    endpoint: quinn::Endpoint,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SendTiming {
    pub planned_ns: u64,
    pub open_ns: u64,
    pub write_ns: u64,
    pub finish_ns: u64,
    pub rtt_us: u64,
    /// true once any byte may have left the process (the reservation must stay charged)
    pub bytes_may_have_left: bool,
    pub error: Option<String>,
}

impl QuicClient {
    pub fn new() -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let cert = rcgen::CertificateParams::new(vec!["solana".to_string()])?.self_signed(&key)?;
        let cert_der = CertificateDer::from(cert.der().to_vec());
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert(provider)))
            .with_client_auth_cert(vec![cert_der], key_der)?;
        tls.alpn_protocols = vec![ALPN.to_vec()];
        tls.enable_early_data = false;
        let mut transport = quinn::TransportConfig::default();
        transport.max_idle_timeout(Some(Duration::from_secs(10).try_into()?));
        transport.keep_alive_interval(Some(Duration::from_millis(500)));
        transport.initial_mtu(1_232);
        transport.min_mtu(1_232);
        transport.mtu_discovery_config(None);
        let mut cfg = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls)?));
        cfg.transport_config(Arc::new(transport));
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(cfg);
        Ok(QuicClient { endpoint })
    }

    /// Handshake with a TPU address. Returns the connection and the handshake duration in µs.
    pub async fn connect(&self, addr: &str, timeout: Duration) -> Result<(quinn::Connection, u64)> {
        let sa: SocketAddr = addr.parse().with_context(|| format!("bad TPU address {addr}"))?;
        let t0 = mono_ns();
        let conn = tokio::time::timeout(timeout, self.endpoint.connect(sa, "connect")?)
            .await
            .map_err(|_| anyhow!("QUIC handshake timeout"))??;
        Ok((conn, (mono_ns() - t0) / 1_000))
    }
}

pub fn is_open(conn: &quinn::Connection) -> bool {
    conn.close_reason().is_none()
}

/// Open `n` unidirectional streams ahead of the fire time. Nothing goes on the wire until a stream is written; this
/// only takes stream credit from the peer, so the timed writes later never wait for it.
pub async fn open_streams(
    conn: &quinn::Connection,
    n: usize,
    timeout: Duration,
) -> Result<Vec<(quinn::SendStream, u64)>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let s =
            tokio::time::timeout(timeout, conn.open_uni()).await.map_err(|_| anyhow!("stream credit timeout"))??;
        out.push((s, mono_ns()));
    }
    Ok(out)
}

/// Write one transaction on a pre-opened stream and finish it. `write_ns` is the send time. Only callable with a
/// live authorization.
pub async fn send_tx(
    _auth: &LiveAuthorization,
    conn: &quinn::Connection,
    stream: (quinn::SendStream, u64),
    bytes: &[u8],
    planned_ns: u64,
) -> (SendTiming, Option<quinn::SendStream>) {
    let (mut s, open_ns) = stream;
    let mut t = SendTiming { planned_ns, open_ns, rtt_us: conn.rtt().as_micros() as u64, ..Default::default() };
    t.write_ns = mono_ns();
    t.bytes_may_have_left = true;
    if let Err(e) = s.write_all(bytes).await {
        t.error = Some(format!("write: {e}"));
        return (t, None);
    }
    if let Err(e) = s.finish() {
        t.error = Some(format!("finish: {e}"));
    }
    t.finish_ns = mono_ns();
    (t, Some(s))
}

/// Wait (bounded) until the peer has acknowledged the whole stream. true = acknowledged.
pub async fn wait_ack(s: quinn::SendStream, timeout: Duration) -> Option<bool> {
    match tokio::time::timeout(timeout, s.stopped()).await {
        Ok(Ok(None)) => Some(true),
        Ok(Ok(Some(_code))) => Some(false),
        Ok(Err(_)) => Some(false),
        Err(_) => None,
    }
}
