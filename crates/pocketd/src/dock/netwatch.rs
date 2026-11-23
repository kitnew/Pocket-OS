//! Network-change hints for the Dock link (issue #7): rtnetlink notifications, no polling.
//!
//! One `NETLINK_ROUTE` socket subscribed to link, IPv4 address and IPv4 route
//! notifications, read by a Tokio task. A notification counts as a hint that the
//! computer may be reachable again when:
//! - an interface's carrier comes up (`IFF_UP` + `IFF_LOWER_UP`), which is an edge: repeated
//!   notifications for an interface that is already up are ignored, and an interface
//!   seen for the first time counts as having been down (the USB gadget re-appears);
//! - an IPv4 address is added (not host-scoped);
//! - a unicast route is added to the main table.
//!
//! Each hint bumps a counter in a `watch` channel, so a burst of notifications collapses
//! into one wake-up. Deciding what to do with a hint (only while a retry is pending,
//! rate limits, a route to the configured host) is the link's job (`link.rs`). A hint is
//! never proof that the agent is there: the link still makes a normal authenticated
//! attempt. If the socket cannot be opened, the link keeps working with its backoff alone.

use std::collections::HashMap;
use std::io;

use rustix::net::{
    AddressFamily, RecvFlags, SocketFlags, SocketType, bind, netlink, recv, socket_with,
};
use tokio::io::unix::AsyncFd;
use tokio::sync::watch;
use tracing::{debug, info, warn};

const RTMGRP_LINK: u32 = 0x1;
const RTMGRP_IPV4_IFADDR: u32 = 0x10;
const RTMGRP_IPV4_ROUTE: u32 = 0x40;

const NLMSG_HDRLEN: usize = 16;
const RTM_NEWLINK: u16 = 16;
const RTM_DELLINK: u16 = 17;
const RTM_NEWADDR: u16 = 20;
const RTM_NEWROUTE: u16 = 24;

const IFF_UP: u32 = 0x1;
const IFF_LOOPBACK: u32 = 0x8;
const IFF_LOWER_UP: u32 = 0x1_0000;
const AF_INET: u8 = 2;
const RT_SCOPE_HOST: u8 = 254;
const RT_TABLE_MAIN: u8 = 254;
const RTN_UNICAST: u8 = 1;

/// Starts the watcher. The receiver's value changes once per batch of relevant
/// notifications; it never changes if netlink is unavailable.
pub fn spawn() -> watch::Receiver<u64> {
    let (tx, rx) = watch::channel(0u64);
    match open() {
        Ok(fd) => {
            info!("dock: watching network changes (rtnetlink)");
            tokio::spawn(run(fd, tx));
        }
        Err(e) => {
            warn!(error = %e, "dock: cannot watch network changes; reconnecting by backoff only");
            // Keep the sender alive so receivers wait forever instead of seeing "closed".
            tokio::spawn(async move {
                tx.closed().await;
            });
        }
    }
    rx
}

fn open() -> io::Result<AsyncFd<rustix::fd::OwnedFd>> {
    let fd = socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None, // protocol 0 = NETLINK_ROUTE
    )?;
    bind(
        &fd,
        &netlink::SocketAddrNetlink::new(0, RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV4_ROUTE),
    )?;
    // `AsyncFd::register` is the replacement, but it is `unsafe` (forbidden in this
    // workspace). Its I/O-safety concern is a `T` that does not own the descriptor;
    // here `AsyncFd` owns the `OwnedFd`, so the descriptor cannot close under it.
    #[allow(deprecated)]
    AsyncFd::new(fd)
}

async fn run(fd: AsyncFd<rustix::fd::OwnedFd>, tx: watch::Sender<u64>) {
    let mut links = HashMap::new();
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let mut guard = match fd.readable().await {
            Ok(g) => g,
            Err(e) => {
                warn!(error = %e, "dock: network watch stopped");
                return;
            }
        };
        let mut hints = Vec::new();
        // Drain everything that is queued, then publish once.
        loop {
            match guard.try_io(|inner| {
                recv(inner.get_ref(), &mut buf[..], RecvFlags::empty())
                    .map(|(n, _)| n)
                    .map_err(io::Error::from)
            }) {
                Ok(Ok(n)) => hints.extend(classify(&buf[..n], &mut links)),
                // The kernel dropped notifications (receive queue full): something changed.
                Ok(Err(e))
                    if e.raw_os_error() == Some(rustix::io::Errno::NOBUFS.raw_os_error()) =>
                {
                    hints.push(Hint::Overrun)
                }
                Ok(Err(e)) => {
                    warn!(error = %e, "dock: network watch stopped");
                    return;
                }
                Err(_would_block) => break,
            }
        }
        if !hints.is_empty() {
            debug!(?hints, "dock: network change");
            tx.send_modify(|n| *n += 1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    LinkUp(i32),
    Address(u32),
    Route,
    Overrun,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_ne_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Parses one netlink datagram. `links` remembers which interfaces are up, so only
/// down → up transitions count.
pub fn classify(buf: &[u8], links: &mut HashMap<i32, bool>) -> Vec<Hint> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + NLMSG_HDRLEN <= buf.len() {
        let len = u32_at(buf, off) as usize;
        if len < NLMSG_HDRLEN || off + len > buf.len() {
            break; // malformed or truncated: ignore the rest
        }
        let kind = u16_at(buf, off + 4);
        let body = &buf[off + NLMSG_HDRLEN..off + len];
        match kind {
            // struct ifinfomsg { u8 family, u8 pad, u16 type, i32 index, u32 flags, u32 change }
            RTM_NEWLINK if body.len() >= 16 => {
                let index = u32_at(body, 4) as i32;
                let flags = u32_at(body, 8);
                let up = flags & IFF_UP != 0 && flags & IFF_LOWER_UP != 0;
                if flags & IFF_LOOPBACK == 0 {
                    let was_up = links.insert(index, up).unwrap_or(false);
                    if up && !was_up {
                        out.push(Hint::LinkUp(index));
                    }
                }
            }
            RTM_DELLINK if body.len() >= 16 => {
                links.remove(&(u32_at(body, 4) as i32));
            }
            // struct ifaddrmsg { u8 family, u8 prefixlen, u8 flags, u8 scope, u32 index }
            RTM_NEWADDR if body.len() >= 8 && body[0] == AF_INET && body[3] != RT_SCOPE_HOST => {
                out.push(Hint::Address(u32_at(body, 4)));
            }
            // struct rtmsg { u8 family, dst_len, src_len, tos, table, protocol, scope, type }
            RTM_NEWROUTE
                if body.len() >= 8
                    && body[0] == AF_INET
                    && body[4] == RT_TABLE_MAIN
                    && body[7] == RTN_UNICAST =>
            {
                out.push(Hint::Route);
            }
            _ => {}
        }
        off += (len + 3) & !3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(kind: u16, body: &[u8]) -> Vec<u8> {
        let len = (NLMSG_HDRLEN + body.len()) as u32;
        let mut m = Vec::new();
        m.extend(len.to_ne_bytes());
        m.extend(kind.to_ne_bytes());
        m.extend(0u16.to_ne_bytes()); // flags
        m.extend(0u32.to_ne_bytes()); // seq
        m.extend(0u32.to_ne_bytes()); // pid
        m.extend(body);
        while m.len() % 4 != 0 {
            m.push(0);
        }
        m
    }
    fn link(kind: u16, index: i32, flags: u32) -> Vec<u8> {
        let mut b = vec![0u8, 0, 1, 0]; // family, pad, type (ARPHRD_ETHER)
        b.extend(index.to_ne_bytes());
        b.extend(flags.to_ne_bytes());
        b.extend(0u32.to_ne_bytes());
        msg(kind, &b)
    }
    const UP: u32 = IFF_UP | IFF_LOWER_UP | 0x40;

    #[test]
    fn only_carrier_up_edges_count() {
        let mut links = HashMap::new();
        // First sight of an up interface counts (a re-created USB gadget).
        assert_eq!(
            classify(&link(RTM_NEWLINK, 5, UP), &mut links),
            [Hint::LinkUp(5)]
        );
        // Duplicates while up: nothing.
        let dup = [link(RTM_NEWLINK, 5, UP), link(RTM_NEWLINK, 5, UP)].concat();
        assert!(classify(&dup, &mut links).is_empty());
        // Carrier lost (administratively up, no LOWER_UP), then back: one hint.
        assert!(classify(&link(RTM_NEWLINK, 5, IFF_UP), &mut links).is_empty());
        let back = [link(RTM_NEWLINK, 5, UP), link(RTM_NEWLINK, 5, UP)].concat();
        assert_eq!(classify(&back, &mut links), [Hint::LinkUp(5)]);
        // Removed and re-created: counts again.
        assert!(classify(&link(RTM_DELLINK, 5, 0), &mut links).is_empty());
        assert_eq!(
            classify(&link(RTM_NEWLINK, 5, UP), &mut links),
            [Hint::LinkUp(5)]
        );
        // Loopback never counts.
        assert!(classify(&link(RTM_NEWLINK, 1, UP | IFF_LOOPBACK), &mut links).is_empty());
    }

    #[test]
    fn ipv4_addresses_and_main_routes_count() {
        let mut links = HashMap::new();
        let mut addr = vec![AF_INET, 16, 0, 0];
        addr.extend(7u32.to_ne_bytes());
        assert_eq!(
            classify(&msg(RTM_NEWADDR, &addr), &mut links),
            [Hint::Address(7)]
        );
        let mut host = vec![AF_INET, 8, 0, RT_SCOPE_HOST];
        host.extend(1u32.to_ne_bytes());
        assert!(classify(&msg(RTM_NEWADDR, &host), &mut links).is_empty());
        let route = [
            AF_INET,
            16,
            0,
            0,
            RT_TABLE_MAIN,
            2,
            253,
            RTN_UNICAST,
            0,
            0,
            0,
            0,
        ];
        assert_eq!(
            classify(&msg(RTM_NEWROUTE, &route), &mut links),
            [Hint::Route]
        );
        let local = [AF_INET, 32, 0, 0, 255, 2, 254, 2, 0, 0, 0, 0]; // local table
        assert!(classify(&msg(RTM_NEWROUTE, &local), &mut links).is_empty());
    }

    #[test]
    fn malformed_input_is_ignored() {
        let mut links = HashMap::new();
        assert!(classify(&[], &mut links).is_empty());
        assert!(classify(&[1, 2, 3], &mut links).is_empty());
        let mut m = link(RTM_NEWLINK, 5, UP);
        m[0] = 200; // length beyond the buffer
        assert!(classify(&m, &mut links).is_empty());
        let short = msg(RTM_NEWLINK, &[0, 0]); // body too short
        assert!(classify(&short, &mut links).is_empty());
    }
}
