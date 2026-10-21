use crate::models::ArchiveEntry;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub fn parse_patchlist(xml_str: &str, base_url: &str) -> Vec<ArchiveEntry> {
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"archive" {
                    let mut size: u64 = 0;
                    let mut checksum = 0u32;
                    let mut original = 0u32;
                    let mut filename = String::new();

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        let val = String::from_utf8_lossy(&attr.value);
                        match key {
                            b"size" => size = val.parse().unwrap_or(0),
                            b"checksum" => {
                                checksum = u32::from_str_radix(val.trim_start_matches("0x"), 16)
                                    .unwrap_or(0);
                            }
                            b"original" => {
                                original = u32::from_str_radix(val.trim_start_matches("0x"), 16)
                                    .unwrap_or(0);
                            }
                            b"filename" => {
                                filename = val.replace('\\', "/");
                            }
                            _ => {}
                        }
                    }

                    if !filename.is_empty() {
                        entries.push(ArchiveEntry {
                            filename,
                            size,
                            original_crc: original,
                            archive_crc: checksum,
                            base_url: base_url.to_string(),
                        });
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    entries
}

pub fn ping_server_tcp(url_str: &str) -> (Option<u128>, String) {
    let parsed = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(e) => return (None, format!("Неверный URL: {}", e)),
    };

    let host = match parsed.host_str() {
        Some(h) => h,
        None => return (None, "Нет хоста в URL".to_string()),
    };

    let port = parsed
        .port()
        .unwrap_or_else(|| if parsed.scheme() == "https" { 443 } else { 80 });

    let addr_str = format!("{}:{}", host, port);
    let t0 = Instant::now();

    let sock_addrs: Vec<_> = match addr_str.to_socket_addrs() {
        Ok(addrs) => addrs.collect(),
        Err(e) => return (None, format!("DNS ошибка: {}", e)),
    };

    if sock_addrs.is_empty() {
        return (None, "DNS не вернул IP".to_string());
    }

    let timeout = Duration::from_millis(2000);
    for addr in sock_addrs {
        if let Ok(_stream) = TcpStream::connect_timeout(&addr, timeout) {
            let elapsed_ms = t0.elapsed().as_millis();
            return (
                Some(elapsed_ms),
                format!("{} ms ({})", elapsed_ms, addr.ip()),
            );
        }
    }

    (None, "Таймаут соединения".to_string())
}
