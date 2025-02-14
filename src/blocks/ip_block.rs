use nix::sys::socket::SockaddrLike;

use super::{Block, I3Block, I3Event};
use std::fs::File;
use std::io::{BufRead as _, BufReader};
use std::process::Command;

#[derive(Default)]
pub struct IPBlock {}

impl Block for IPBlock {
    fn render(&self) -> Option<I3Block> {
        let reader = BufReader::new(File::open("/proc/net/route").ok()?).lines();
        for line in reader.map_while(Result::ok) {
            let mut split = line.split('\t');
            let Some(interface) = split.next() else {
                continue;
            };
            // Detect default route
            if split.next() == Some("00000000") {
                // get ip for: interface
                let mut addr = "".to_string();

                let addrs = nix::ifaddrs::getifaddrs().unwrap();
                for ifaddr in addrs {
                    match ifaddr.address {
                        Some(address) => {
                            if ifaddr.interface_name == interface
                                && address.family() == Some(nix::sys::socket::AddressFamily::Inet)
                            {
                                addr = address
                                    .to_string()
                                    .split(":")
                                    .next()
                                    .unwrap_or_default()
                                    .to_string();
                            };
                        }
                        None => {}
                    }
                }

                let mut ssid = get_nm_ssid(interface);
                if !ssid.trim().is_empty() {
                    ssid = format!(" - {}", ssid)
                };
                if !addr.trim().is_empty() {
                    addr = format!(" - {}", addr)
                };

                return Some(I3Block {
                    full_text: format!("{}{}{}", interface.to_owned(), ssid, addr),
                    ..Default::default()
                });
            }
        }
        Some(I3Block {
            full_text: "No link".to_owned(),
            color: Some("#ff0202".to_owned()),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}

fn get_nm_ssid(interface: &str) -> String {
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "nmcli connection show | grep {} | grep wifi",
            interface
        ))
        .output()
        .unwrap_or_default();
    return String::from_utf8(output.stdout)
        .unwrap_or_default()
        .split("  ")
        .next()
        .unwrap_or_default()
        .to_owned();
}
