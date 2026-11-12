//! Instant notice of new programs through the netlink "proc connector" (PROC_EVENT_EXEC).
//! That way the daemon places the program right after exec(), before it allocates memory.
//! Needs root (CAP_NET_ADMIN); without it the daemon carries on with scanning only.

use std::sync::mpsc::{Receiver, channel};

const NETLINK_CONNECTOR: i32 = 11;
const CN_IDX_PROC: u32 = 1;
const CN_VAL_PROC: u32 = 1;
const PROC_CN_MCAST_LISTEN: u32 = 1;
const PROC_EVENT_EXEC: u32 = 0x0000_0002;
const NLMSG_DONE: u16 = 3;
const NLMSG_HDR: usize = 16;
const CN_MSG: usize = 20;

fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_ne_bytes());
}
fn put_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_ne_bytes());
}
fn get_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_ne_bytes(s.try_into().unwrap()))
}

/// Subscription message (nlmsghdr + cn_msg + operation).
fn listen_msg() -> Vec<u8> {
    let mut b = Vec::with_capacity(40);
    put_u32(&mut b, (NLMSG_HDR + CN_MSG + 4) as u32);
    put_u16(&mut b, NLMSG_DONE);
    put_u16(&mut b, 0);
    put_u32(&mut b, 0);
    put_u32(&mut b, std::process::id());
    put_u32(&mut b, CN_IDX_PROC);
    put_u32(&mut b, CN_VAL_PROC);
    put_u32(&mut b, 0);
    put_u32(&mut b, 0);
    put_u16(&mut b, 4);
    put_u16(&mut b, 0);
    put_u32(&mut b, PROC_CN_MCAST_LISTEN);
    b
}

/// Extracts the PIDs (tgid) of the exec events in a datagram.
pub fn parse_exec_pids(buf: &[u8]) -> Vec<i32> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + NLMSG_HDR <= buf.len() {
        let Some(len) = get_u32(buf, off).map(|l| l as usize) else {
            break;
        };
        if len < NLMSG_HDR || off + len > buf.len() {
            break;
        }
        let ev = off + NLMSG_HDR + CN_MSG;
        if get_u32(buf, ev) == Some(PROC_EVENT_EXEC)
            && let Some(tgid) = get_u32(buf, ev + 20)
        {
            out.push(tgid as i32);
        }
        off += (len + 3) & !3;
    }
    out
}

/// Opens the event channel. Returns None if not possible (no root, kernel without support...).
pub fn start() -> Option<Receiver<i32>> {
    unsafe {
        let fd = libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            NETLINK_CONNECTOR,
        );
        if fd < 0 {
            return None;
        }
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_pid = std::process::id();
        addr.nl_groups = CN_IDX_PROC;
        if libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_nl>() as u32,
        ) != 0
        {
            libc::close(fd);
            return None;
        }
        let msg = listen_msg();
        if libc::send(fd, msg.as_ptr() as *const libc::c_void, msg.len(), 0) < 0 {
            libc::close(fd);
            return None;
        }
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("procevents".into())
            .spawn(move || {
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0);
                    if n <= 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        // ENOBUFS (too many events at once): carry on; the scan covers the rest.
                        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOBUFS) {
                            continue;
                        }
                        break;
                    }
                    for pid in parse_exec_pids(&buf[..n as usize]) {
                        if tx.send(pid).is_err() {
                            return;
                        }
                    }
                }
                libc::close(fd);
            })
            .ok()?;
        Some(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_synthetic_exec_event() {
        let mut b = Vec::new();
        let total = NLMSG_HDR + CN_MSG + 24;
        put_u32(&mut b, total as u32);
        put_u16(&mut b, NLMSG_DONE);
        put_u16(&mut b, 0);
        put_u32(&mut b, 0);
        put_u32(&mut b, 0);
        b.extend_from_slice(&[0u8; CN_MSG]);
        put_u32(&mut b, PROC_EVENT_EXEC);
        put_u32(&mut b, 3); // cpu
        b.extend_from_slice(&0u64.to_ne_bytes());
        put_u32(&mut b, 4321); // pid (thread)
        put_u32(&mut b, 4300); // tgid
        assert_eq!(parse_exec_pids(&b), vec![4300]);
        assert_eq!(listen_msg().len(), 40);
    }

    #[test]
    fn live_events_if_permitted() {
        // Only really runs if the environment allows it (root with CAP_NET_ADMIN).
        let Some(rx) = start() else { return };
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id() as i32;
        let got = (0..50).any(|_| {
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .is_ok_and(|p| p == pid)
        });
        let _ = child.wait();
        assert!(got, "não recebi o PROC_EVENT_EXEC do processo filho");
    }
}
