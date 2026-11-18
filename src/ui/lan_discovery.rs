use std::{
    io,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
const PORT: u16 = 4445;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Server {
    pub motd: String,
    pub address: String,
}

#[derive(Default)]
pub(super) struct Discovery {
    socket: Option<UdpSocket>,
    last_poll: Option<Instant>,
    pub servers: Vec<Server>,
}

impl Discovery {
    pub fn start(&mut self) {
        self.socket = None;
        self.servers.clear();
        self.last_poll = None;
        match listen() {
            Ok(socket) => self.socket = Some(socket),
            Err(e) => eprintln!("Could not start LAN server discovery: {e}"),
        }
    }

    pub fn stop(&mut self) {
        self.socket = None;
    }

    pub fn poll(&mut self) {
        let Some(socket) = &self.socket else { return };
        if self.last_poll.is_some_and(|last| last.elapsed() < Duration::from_millis(50)) {
            return;
        }
        self.last_poll = Some(Instant::now());
        let mut packet = [0; 1024];
        for _ in 0..64 {
            match socket.recv_from(&mut packet) {
                Ok((n, source)) => {
                    if let Some(server) = parse(&packet[..n], source) {
                        if !self.servers.iter().any(|entry| entry.address == server.address) {
                            self.servers.push(server);
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("LAN server discovery stopped: {e}");
                    self.socket = None;
                    break;
                }
            }
        }
    }
}

fn parse(packet: &[u8], source: SocketAddr) -> Option<Server> {
    let packet = String::from_utf8_lossy(packet);
    let (title, suffix) = packet.split_once("[/MOTD]")?;
    if suffix.contains("[/MOTD]") {
        return None;
    }
    let (_, ad) = suffix.split_once("[AD]")?;
    let (port, _) = ad.split_once("[/AD]")?;
    let address = SocketAddr::new(source.ip(), port.parse::<u16>().ok()?);
    if address.port() == 0 {
        return None;
    }
    Some(Server {
        motd: title.split_once("[MOTD]").map_or("missing no", |(_, motd)| motd).to_owned(),
        address: address.to_string(),
    })
}

fn listen() -> io::Result<UdpSocket> {
    let socket = multicast_socket(PORT)?;
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

#[cfg(not(windows))]
fn multicast_socket(port: u16) -> io::Result<UdpSocket> {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
}

// Java MulticastSocket enables SO_REUSEADDR before bind. Without this a second
// vanilla or Ferrum client on the same Windows machine cannot discover games.
#[cfg(windows)]
fn multicast_socket(port: u16) -> io::Result<UdpSocket> {
    use std::os::windows::io::{AsRawSocket, FromRawSocket};

    #[repr(C)]
    struct SockAddrIn { family: u16, port: u16, ip: u32, padding: [u8; 8] }
    #[link(name = "ws2_32")]
    extern "system" {
        fn socket(af: i32, kind: i32, protocol: i32) -> usize;
        fn setsockopt(s: usize, level: i32, opt: i32, value: *const i8, len: i32) -> i32;
        fn bind(s: usize, addr: *const SockAddrIn, len: i32) -> i32;
        fn WSAGetLastError() -> i32;
    }

    // Initialize Winsock through std so its process-wide lifetime matches std's sockets.
    drop(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?);
    let s = unsafe { socket(2, 2, 17) };
    if s == usize::MAX {
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    let sock = unsafe { UdpSocket::from_raw_socket(s as _) };
    let handle = sock.as_raw_socket() as usize;
    let reuse: i32 = 1;
    let addr = SockAddrIn { family: 2, port: port.to_be(), ip: 0, padding: [0; 8] };
    if unsafe { setsockopt(handle, 0xffff, 4, (&reuse as *const i32).cast(), 4) } != 0
        || unsafe { bind(handle, &addr, std::mem::size_of_val(&addr) as i32) } != 0
    {
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(sock)
}

#[cfg(test)]
pub(super) fn loopback(discovery: &mut Discovery) -> SocketAddr {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = socket.local_addr().unwrap();
    socket.set_nonblocking(true).unwrap();
    discovery.socket = Some(socket);
    address
}
