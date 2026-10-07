//! Single instance, second half: the instance holding `instance.lock` also listens on a
//! loopback port (written to `instance.port`); a second launch connects there so the first
//! shows its window instead of two nodes sharing one data dir. Loopback TCP works the same on
//! Linux and Windows and needs no extra dependency.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

const MAGIC: &[u8] = b"tinline-show\n";

/// Starts the listener thread; `on_show` runs for every second launch. Failure is non-fatal:
/// the lock still prevents a second node, the first just is not woken.
pub fn listen(data: &Path, on_show: impl Fn() + Send + 'static) {
    let Ok(l) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else { return };
    let Ok(addr) = l.local_addr() else { return };
    let _ = std::fs::write(data.join("instance.port"), addr.port().to_string());
    let _ = std::thread::Builder::new().name("single".into()).spawn(move || {
        for conn in l.incoming().flatten() {
            let _ = conn.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 32];
            let mut got = Vec::new();
            let mut c = conn;
            while got.len() < MAGIC.len() {
                match c.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                }
            }
            if got.starts_with(MAGIC) {
                on_show();
            }
        }
    });
}

/// Tells the running instance to show itself. False if it could not be reached.
pub fn poke(data: &Path) -> bool {
    let Some(port) = std::fs::read_to_string(data.join("instance.port"))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
    else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .and_then(|mut c| c.write_all(MAGIC))
        .is_ok()
}
