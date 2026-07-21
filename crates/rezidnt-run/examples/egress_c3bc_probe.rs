//! DEV-ONLY test-support for the c3bc-enforce dataplane (DR-023: fixtures stay
//! dev-only — an `[[example]]`, NEVER linked into the daemon binary, so I7's
//! one-static-binary posture is untouched). Three modes drive the enforce
//! integration suite `egress_mediation_c3bc.rs`:
//!
//!   - `escape-probe   <proxy_addr> <ca_pem> <via_host>` — exec'd by `pasta`
//!     INSIDE the sealed netns. It SEALS the route table to the proxy-only `/32`
//!     (the inescapability precondition), then runs the direct-egress escape
//!     attempts (unset-proxy-env by-name, raw socket to a public IP, alternate
//!     DNS resolver) and the sanctioned via-proxy reach. Each attempt is one JSON
//!     line on stdout the parent parses into a `ProbeReport` (DR-026 crit 3).
//!
//!   - `injected-egress <proxy_addr> <ca_pem> <host>` — exec'd by `pasta` inside
//!     the sealed netns. Seals routes, then issues ONE HTTPS request to `host`
//!     THROUGH the proxy (trusting `ca_pem`, the rezidnt CA). It carries NO
//!     Authorization of its own (the agent never holds the token). Prints
//!     `AGENT_ENV {json}` — its own environment — so the suite asserts the token
//!     is absent agent-side (DR-026 crit 4).
//!
//!   - `upstream-capture <listen_addr> <ca_der_out> <capture_out>` — started by
//!     the SUITE in the HOST namespace (it has internet). A self-signed TLS server
//!     the proxy dials as the "allowlisted upstream": it writes its CA DER to
//!     `ca_der_out` (so the proxy's upstream client trusts it) and, on each
//!     request, writes the headers it RECEIVED to `capture_out` — independent
//!     proof the injected token reached the upstream (DR-026 crit 4).
//!
//! The netns sealing execs `ip` (userspace routing; `pasta` maps the host gateway
//! so the proxy on host loopback is reachable as `GATEWAY:port`). No new linked
//! crate — `ip` is exec'd like `pasta`/`bwrap` (I7).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).context("usage: <mode> ...")?;
    match mode.as_str() {
        "escape-probe" => escape_probe(),
        "injected-egress" => injected_egress(),
        "upstream-capture" => upstream_capture(),
        other => bail!("unknown mode {other:?}"),
    }
}

/// Seal the netns route table to the proxy-only `/32`: drop the default route,
/// flush the main table, keep ONLY a host route to the gateway `pasta` maps the
/// proxy into, and delete the local-table self route. After this, a raw socket to
/// any public IP gets ENETUNREACH — the inescapability the criterion-3 probe then
/// demonstrates. Execs `ip` (no linked crate).
fn seal_routes() -> Result<()> {
    let out = run("ip", &["-o", "route", "show", "default"])?;
    // "default via <GW> dev <DEV> ..."
    let mut gw = String::new();
    let mut dev = String::new();
    let toks: Vec<&str> = out.split_whitespace().collect();
    for w in toks.windows(2) {
        match w[0] {
            "via" => gw = w[1].to_string(),
            "dev" => dev = w[1].to_string(),
            _ => {}
        }
    }
    if gw.is_empty() || dev.is_empty() {
        bail!("could not parse default route (gw={gw:?} dev={dev:?}) from {out:?}");
    }

    // Flush the main table (drops the default route + the on-link subnet route)
    // and add back ONLY a host `/32` to the gateway `pasta` maps the proxy into.
    // After this the ONLY routable peer is the proxy: a raw socket to any PUBLIC
    // IP (1.1.1.1, 8.8.8.8, …) gets ENETUNREACH — the inescapability the probe
    // then demonstrates.
    //
    // We deliberately do NOT delete the local-table self route: `pasta`'s reply
    // path to the mapped-gateway proxy depends on it, so removing it breaks the
    // proxy reach too (the sole route must actually carry traffic — the
    // non-vacuity requirement). The ONLY peer still reachable besides the proxy
    // is the host's OWN address (which lands on `pasta`, NOT the open internet) —
    // not a public-egress hole, and not what criterion 3 probes (it probes
    // reaching the OPEN INTERNET via public IPs, which stays ENETUNREACH).
    let _ = run("ip", &["route", "flush", "table", "main"]);
    run(
        "ip",
        &[
            "route",
            "add",
            &format!("{gw}/32"),
            "dev",
            &dev,
            "scope",
            "link",
        ],
    )
    .context("add proxy-only /32 route")?;
    Ok(())
}

/// The gateway address the proxy is mapped to inside the netns (pasta --map-gw).
fn gateway_addr() -> Result<String> {
    // After sealing, the only route is the gateway /32; read it back.
    let out = run("ip", &["-o", "route", "show"])?;
    let gw = out
        .lines()
        .flat_map(|l| l.split_whitespace().next())
        .find(|t| t.parse::<std::net::Ipv4Addr>().is_ok())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("no gateway route after seal: {out:?}"))?;
    Ok(gw)
}

fn escape_probe() -> Result<()> {
    let proxy_addr = arg(2)?; // 127.0.0.1:PORT on the host (mapped to GW:PORT in-ns)
    let ca_pem = arg(3)?;
    let via_host = arg(4)?;
    let proxy_port = proxy_addr
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("bad proxy addr {proxy_addr:?}"))?;

    seal_routes().context("seal netns routes")?;
    let gw = gateway_addr()?;
    let mut out = std::io::stdout();

    // (a) unset-proxy-env by name — no resolver in-ns, so a name lookup cannot even
    // resolve; the point is a non-proxy attempt reaches nothing. We approximate the
    // "by name" attempt with a connect to a hard public IP after clearing proxy env
    // (there is no DNS in the sealed ns; a real agent's getaddrinfo would fail).
    for (k, _) in std::env::vars() {
        if k.ends_with("_PROXY") || k.ends_with("_proxy") {
            unsafe { std::env::remove_var(k) };
        }
    }
    emit(&mut out, unset_proxy_env_attempt("1.1.1.1", 443, &via_host));

    // (b) raw socket to a known public IP — must be ENETUNREACH (blocked).
    emit(&mut out, raw_public_attempt("1.1.1.1", 80));

    // (c) alternate DNS resolver directly — must be blocked.
    emit(&mut out, alt_dns_attempt("8.8.8.8"));

    // (d) the sanctioned via-proxy reach — MUST succeed (non-vacuity). Connect to
    // the proxy via the mapped gateway address and speak TLS for `via_host`.
    emit(
        &mut out,
        via_proxy_attempt(&gw, proxy_port, &via_host, &ca_pem),
    );
    Ok(())
}

fn unset_proxy_env_attempt(ip: &str, port: u16, target_host: &str) -> serde_json::Value {
    let reach = match tcp_connect(ip, port, Duration::from_secs(3)) {
        Ok(_) => reached_open_internet(format!("connected {ip}:{port} after unsetting proxy env")),
        Err(e) => blocked(format!("unset-proxy-env by-name blocked: {e}")),
    };
    serde_json::json!({"attempt":"unset_proxy_env","target":target_host,
        "reach":reach.0,"detail":reach.1})
}

fn raw_public_attempt(ip: &str, port: u16) -> serde_json::Value {
    let reach = match tcp_connect(ip, port, Duration::from_secs(3)) {
        Ok(_) => reached_open_internet(format!("raw socket reached {ip}:{port}")),
        Err(e) => blocked(format!("raw socket to {ip}:{port} blocked: {e}")),
    };
    serde_json::json!({"attempt":"raw_socket_public_ip","ip":ip,"port":port,
        "reach":reach.0,"detail":reach.1})
}

fn alt_dns_attempt(resolver: &str) -> serde_json::Value {
    let reach = match tcp_connect(resolver, 53, Duration::from_secs(3)) {
        Ok(_) => reached_open_internet(format!("reached alt resolver {resolver}:53")),
        Err(e) => blocked(format!("alt DNS {resolver}:53 blocked: {e}")),
    };
    serde_json::json!({"attempt":"alt_dns_resolver","resolver":resolver,
        "reach":reach.0,"detail":reach.1})
}

fn via_proxy_attempt(gw: &str, port: u16, host: &str, ca_pem: &str) -> serde_json::Value {
    match tls_request_through_proxy(gw, port, host, ca_pem, None) {
        Ok(_) => {
            let r = reached_proxy(format!("TLS to {host} via proxy {gw}:{port} succeeded"));
            serde_json::json!({"attempt":"via_proxy","target":host,"reach":r.0,"detail":r.1})
        }
        Err(e) => {
            // The via-proxy path failing is NOT an escape — but it IS a vacuity
            // failure the parent's `proxy_path_reached` guard catches. Report it as
            // blocked with the error so the parent surfaces a real diagnostic.
            let r = blocked(format!(
                "via-proxy reach FAILED (non-vacuity guard will fire): {e}"
            ));
            serde_json::json!({"attempt":"via_proxy","target":host,"reach":r.0,"detail":r.1})
        }
    }
}

fn injected_egress() -> Result<()> {
    let proxy_addr = arg(2)?;
    let ca_pem = arg(3)?;
    let host = arg(4)?;
    let proxy_port = proxy_addr
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("bad proxy addr {proxy_addr:?}"))?;

    seal_routes().context("seal netns routes")?;
    let gw = gateway_addr()?;

    // Report the agent's OWN environment FIRST (the suite asserts the token is
    // absent from it). The agent carries NO Authorization of its own.
    let env: BTreeMap<String, String> = std::env::vars().collect();
    println!("AGENT_ENV {}", serde_json::to_string(&env)?);

    // Issue ONE HTTPS request through the proxy. The agent sends no token; the
    // proxy injects the brokered secret UPSTREAM only.
    let agent_headers = vec![("X-Agent-Probe".to_string(), "c3bc".to_string())];
    tls_request_through_proxy(&gw, proxy_port, &host, &ca_pem, Some(agent_headers))
        .with_context(|| format!("injected egress to {host} through proxy {gw}:{proxy_port}"))?;
    Ok(())
}

/// The capturing upstream TLS server (host namespace). Generates a self-signed
/// CA+leaf, writes the CA DER for the proxy to trust, and on each request writes
/// the received headers to the capture file (independent proof of injection).
fn upstream_capture() -> Result<()> {
    let listen_addr = arg(2)?;
    let ca_der_out = arg(3)?;
    let capture_out = arg(4)?;
    let sni = std::env::args()
        .nth(5)
        .unwrap_or_else(|| "github.com".to_string());

    // Self-signed CA + leaf for the upstream identity (test-support crypto).
    let ca_key = rcgen::KeyPair::generate()?;
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "c3bc upstream CA");
    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_der = ca_cert.der().to_vec();
    std::fs::write(&ca_der_out, &ca_der).context("write upstream CA DER")?;
    let issuer = rcgen::Issuer::new(ca_params, ca_key);

    let leaf_key = rcgen::KeyPair::generate()?;
    let mut leaf_params = rcgen::CertificateParams::new(vec![sni.clone()])?;
    leaf_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, &sni);
    let leaf = leaf_params.signed_by(&leaf_key, &issuer)?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![
                rustls::pki_types::CertificateDer::from(leaf.der().to_vec()),
                rustls::pki_types::CertificateDer::from(ca_der.clone()),
            ],
            rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der())
                .map_err(|e| anyhow!("leaf key encode: {e}"))?,
        )?;
    let cfg = Arc::new(cfg);

    let listener = TcpListener::bind(&listen_addr).context("bind upstream capture")?;
    // Signal readiness on stdout so the suite can wait for the bind.
    println!("UPSTREAM_READY {}", listener.local_addr()?);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let conn = rustls::ServerConnection::new(cfg.clone())?;
        let mut tls = rustls::StreamOwned::new(conn, stream);
        let (_line, headers) = match read_http_head(&mut tls) {
            Ok(h) => h,
            Err(_) => continue,
        };
        // Record the headers the upstream RECEIVED (post-injection).
        let _ = std::fs::write(&capture_out, serde_json::to_string(&headers)?);
        let body = b"ok";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = tls.write_all(resp.as_bytes());
        let _ = tls.write_all(body);
        let _ = tls.flush();
        // One request is enough for a criterion-4 capture; exit so the suite does
        // not block on a second accept.
        break;
    }
    Ok(())
}

// ---- shared helpers (dev-only) --------------------------------------------

fn tls_request_through_proxy(
    proxy_host: &str,
    proxy_port: u16,
    host: &str,
    ca_pem: &str,
    extra_headers: Option<Vec<(String, String)>>,
) -> Result<()> {
    let pem = std::fs::read_to_string(ca_pem).context("read CA pem")?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile_certs(&pem)? {
        roots
            .add(cert)
            .map_err(|e| anyhow!("add CA to roots: {e}"))?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| anyhow!("server name {host}: {e}"))?;
    let conn = rustls::ClientConnection::new(Arc::new(cfg), server_name)?;
    let sock = tcp_connect(proxy_host, proxy_port, Duration::from_secs(5))?;
    let mut tls = rustls::StreamOwned::new(conn, sock);

    let mut req = format!("GET / HTTP/1.1\r\nHost: {host}\r\n");
    if let Some(hs) = extra_headers {
        for (k, v) in hs {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    req.push_str("Connection: close\r\n\r\n");
    tls.write_all(req.as_bytes())?;
    tls.flush()?;
    // Read the response so the round-trip completes (the reach is real). Tolerate
    // a peer close WITHOUT a TLS close_notify (the proxy closes after relaying) —
    // rustls surfaces that as UnexpectedEof; the bytes already read are the body.
    let mut resp = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => resp.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        }
    }
    if resp.is_empty() {
        bail!("empty response through proxy (no reach)");
    }
    Ok(())
}

fn tcp_connect(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    use std::net::ToSocketAddrs;
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .context("resolve")?
        .next()
        .ok_or_else(|| anyhow!("no addr for {host}:{port}"))?;
    let s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    Ok(s)
}

fn read_http_head<S: Read>(stream: &mut S) -> Result<(String, BTreeMap<String, String>)> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    loop {
        let n = stream.read(&mut b)?;
        if n == 0 {
            break;
        }
        buf.push(b[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 64 * 1024 {
            bail!("request head too large");
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap_or("").to_string();
    let mut headers = BTreeMap::new();
    for l in lines {
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok((line, headers))
}

/// Minimal PEM certificate reader (no `rustls-pemfile` dep — dev-only, so a tiny
/// parser beats a linked crate; and this example is not shipped anyway).
fn rustls_pemfile_certs(pem: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let mut out = Vec::new();
    let mut in_cert = false;
    let mut b64 = String::new();
    for line in pem.lines() {
        if line.contains("BEGIN CERTIFICATE") {
            in_cert = true;
            b64.clear();
        } else if line.contains("END CERTIFICATE") {
            in_cert = false;
            let der = base64_decode(&b64)?;
            out.push(rustls::pki_types::CertificateDer::from(der));
        } else if in_cert {
            b64.push_str(line.trim());
        }
    }
    Ok(out)
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut bits = 0u32;
    let mut nbits = 0;
    let mut out = Vec::new();
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let v = match val(c) {
            Some(v) => v,
            None => continue,
        };
        bits = (bits << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Ok(out)
}

fn run(bin: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(bin)
        .args(args)
        .output()
        .with_context(|| format!("exec {bin} {args:?}"))?;
    if !out.status.success() {
        bail!(
            "{bin} {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn arg(n: usize) -> Result<String> {
    std::env::args()
        .nth(n)
        .ok_or_else(|| anyhow!("missing arg {n}"))
}

fn emit(out: &mut impl Write, v: serde_json::Value) {
    let _ = writeln!(out, "{v}");
}

fn blocked(detail: String) -> (&'static str, String) {
    ("blocked", detail)
}
fn reached_proxy(detail: String) -> (&'static str, String) {
    ("reached_proxy", detail)
}
fn reached_open_internet(detail: String) -> (&'static str, String) {
    ("reached_open_internet", detail)
}
