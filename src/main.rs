use clap::Parser;
use hashbrown::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

const BUFFER_SIZE: usize = 4096;
const LINUX_KERNEL_TXT: &'static [u8] = include_bytes!("../assets/linux_kernel.txt");

#[tokio::main]
async fn run(
    bind_addr: SocketAddr,
    remote_addr: SocketAddr,
    ipv4: bool,
    max_client: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket = Arc::new(UdpSocket::bind(&bind_addr).await?);
    println!("Bind: {}, Forwarding to: {}", bind_addr, remote_addr);
    let mut addr_map: HashMap<SocketAddr, Arc<UdpSocket>> = HashMap::new();
    let mut handle_list: Vec<JoinHandle<()>> = Vec::new();
    loop {
        let mut buf = [0; BUFFER_SIZE];
        if let Ok((len, addr)) = socket.recv_from(&mut buf).await {
            //println!("Got packet from {}", addr);
            if let Some(sock) = addr_map.get(&addr) {
                sock.send(&buf[..len]).await.ok();
            } else {
                socket.send_to(&LINUX_KERNEL_TXT, addr).await.ok();
                println!("New client {}", addr);
                if addr_map.len() > max_client {
                    tokio::spawn(async move {
                        for v in handle_list.into_iter() {
                            v.abort();
                        }
                    });
                    addr_map = HashMap::new();
                    handle_list = Vec::new();
                }
                let sock_temp = Arc::new(
                    match UdpSocket::bind(if ipv4 { "0.0.0.0:0" } else { "[::]:0" }).await {
                        Ok(sock) => sock,
                        Err(e) => {
                            println!("Error: {}", e);
                            continue;
                        }
                    },
                );
                addr_map.insert(addr, sock_temp.clone());
                if let Err(e) = sock_temp.connect(&remote_addr).await {
                    println!("Error: {}", e);
                    continue;
                }
                match sock_temp.send(&buf[..len]).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("{} occured while sending packet to {}", e, addr);
                    }
                }
                let sock = sock_temp.clone();
                let sock_send = socket.clone();
                handle_list.push(tokio::spawn(async move {
                    let addr = addr.clone();
                    loop {
                        let mut buf = [0u8; BUFFER_SIZE];
                        if let Ok(len) = sock.recv(&mut buf).await {
                            //println!("Send packet to {}", addr);
                            sock_send.send_to(&buf[..len], addr).await.ok();
                        }
                    }
                }));
            }
        }
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short='b', long, value_parser=parse_socket_addr)]
    bind: SocketAddr,

    #[arg(short='r', long, value_parser=parse_socket_addr)]
    remote: SocketAddr,

    #[arg(short = 'm', long, default_value_t = 63)]
    max_client: usize,

    #[arg(short = '4', long, default_value = "false")]
    ipv4: bool,
}

fn parse_socket_addr(hostname_port: &str) -> std::io::Result<SocketAddr> {
    let socketaddr = hostname_port.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("Could not find destination {hostname_port}"),
        )
    })?;
    Ok(socketaddr)
}

fn main() {
    let args = Args::parse();

    run(args.bind, args.remote, args.ipv4, args.max_client).unwrap();
}
