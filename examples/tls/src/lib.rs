//! `Get-RustTlsInfo`: one HTTPS request the module makes itself through
//! rustls, whose aws-lc-rs provider offers the X25519MLKEM768 hybrid key
//! exchange first, and a report of what the handshake negotiated. The
//! host's own web cmdlets and every other module keep the host's TLS stack;
//! only this module's connections use this one.

use pwrs::prelude::*;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// What one handshake negotiated, and the response it carried.
#[psclass(name = "Tls.Handshake")]
#[derive(Default, Clone)]
pub struct Handshake {
    /// The host connected to.
    pub host: String,
    /// The protocol version, such as TLSv1_3.
    pub protocol: String,
    /// The key exchange group, such as X25519MLKEM768.
    pub key_exchange: String,
    /// The cipher suite.
    pub cipher_suite: String,
    /// The response's status line.
    pub status: String,
    /// The response body, as text.
    pub body: String,
}

/// Fetches `Uri` over HTTPS through rustls inside the module, not the
/// host's TLS stack, and writes what the handshake negotiated with the
/// response.
///
/// # Examples
/// Get-RustTlsInfo https://pq.cloudflareresearch.com/cdn-cgi/trace
#[cmdlet(verb = "Get", noun = "RustTlsInfo", output = ["Tls.Handshake"])]
pub struct GetRustTlsInfo {
    /// An https:// address.
    #[param(mandatory, position = 0, value_from_pipeline)]
    pub uri: String,
    /// Seconds to wait for the connection and for each read or write; 30 when not given.
    #[param(validate_range(1, 300))]
    pub timeout_seconds: u64,
}

/// An unbound parameter keeps its field's value, so this is the default timeout.
impl Default for GetRustTlsInfo {
    fn default() -> Self {
        GetRustTlsInfo { uri: String::new(), timeout_seconds: 30 }
    }
}

impl Cmdlet for GetRustTlsInfo {
    fn process(&mut self, ps: &Pipeline<'_>) -> PsResult<()> {
        let (host, port, path) = split_https(&self.uri)?;
        let wait = Duration::from_secs(self.timeout_seconds);
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let config = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| PsError::new(ErrorCategory::InvalidArgument, "TlsBadHost", format!("{host} is not a server name: {e}")))?;
        let mut conn = rustls::ClientConnection::new(Arc::new(config), name)
            .map_err(|e| PsError::new(ErrorCategory::SecurityError, "TlsHandshake", e.to_string()))?;
        let mut sock = connect(&host, port, wait)?;
        let mut tls = rustls::Stream::new(&mut conn, &mut sock);
        // HTTP/1.0, so the server neither chunks the body nor keeps the connection open.
        write!(tls, "GET {path} HTTP/1.0\r\nHost: {host}\r\nAccept-Encoding: identity\r\nUser-Agent: pwrs-example-tls\r\n\r\n").map_err(io_error)?;
        let response = read_all(&mut tls)?;
        let text = String::from_utf8_lossy(&response);
        let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| {
            PsError::new(ErrorCategory::InvalidResult, "TlsResponse", format!("the response from {host} has no end of headers"))
        })?;
        let status = head.lines().next().ok_or_else(|| {
            PsError::new(ErrorCategory::InvalidResult, "TlsResponse", format!("the response from {host} has no status line"))
        })?;
        let unnegotiated = |what: &str| PsError::new(ErrorCategory::SecurityError, "TlsHandshake", format!("the exchange with {host} finished without a {what}"));
        let protocol = conn.protocol_version().ok_or_else(|| unnegotiated("protocol version"))?;
        let group = conn.negotiated_key_exchange_group().ok_or_else(|| unnegotiated("key exchange group"))?;
        let suite = conn.negotiated_cipher_suite().ok_or_else(|| unnegotiated("cipher suite"))?;
        ps.write(Handshake {
            host: host.clone(),
            protocol: format!("{protocol:?}"),
            key_exchange: format!("{:?}", group.name()),
            cipher_suite: format!("{:?}", suite.suite()),
            status: status.to_string(),
            body: body.to_string(),
        })
    }
}

/// Splits `https://host[:port][/path]` into its parts; anything else is refused.
fn split_https(uri: &str) -> PsResult<(String, u16, String)> {
    let rest = uri
        .strip_prefix("https://")
        .ok_or_else(|| PsError::new(ErrorCategory::InvalidArgument, "TlsNotHttps", format!("{uri} is not an https:// address")))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => {
            let port = port
                .parse::<u16>()
                .map_err(|e| PsError::new(ErrorCategory::InvalidArgument, "TlsBadPort", format!("{port} is not a port: {e}")))?;
            (host, port)
        }
        None => (authority, 443),
    };
    if host.is_empty() {
        return Err(PsError::new(ErrorCategory::InvalidArgument, "TlsBadHost", format!("{uri} names no host")));
    }
    Ok((host.to_string(), port, path.to_string()))
}

/// Connects to the first address `host` resolves to that accepts within
/// `wait`, and bounds every later read and write by `wait` as well.
fn connect(host: &str, port: u16, wait: Duration) -> PsResult<TcpStream> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs().map_err(io_error)? {
        match TcpStream::connect_timeout(&addr, wait) {
            Ok(sock) => {
                sock.set_read_timeout(Some(wait)).map_err(io_error)?;
                sock.set_write_timeout(Some(wait)).map_err(io_error)?;
                return Ok(sock);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) => io_error(e),
        None => PsError::new(ErrorCategory::ConnectionError, "TlsNoAddress", format!("{host} resolved to no address")),
    })
}

/// Reads to the end of the stream, reserving before each chunk is kept, so
/// a response too large for the allocator is an error record and not the
/// end of the session. A server that closes without a close_notify alert
/// ends the response the same way as one that sends it.
fn read_all(stream: &mut impl Read) -> PsResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => return Ok(out),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(out),
            Err(e) => return Err(io_error(e)),
        };
        out.try_reserve(n)?;
        out.extend_from_slice(&chunk[..n]);
    }
}

fn io_error(e: std::io::Error) -> PsError {
    PsError::new(ErrorCategory::ConnectionError, "TlsConnection", e.to_string())
}

pwrs::export_module! {
    name: "Tls",
    cmdlets: [GetRustTlsInfo],
    classes: [Handshake],
}
