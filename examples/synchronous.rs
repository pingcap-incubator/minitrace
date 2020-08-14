// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

mod common;

#[derive(Debug)]
enum SyncJob {
    #[allow(dead_code)]
    Unknown,
    Root,
    Func1,
    Func2,
}

impl Into<u32> for SyncJob {
    fn into(self) -> u32 {
        self as u32
    }
}

fn func1(i: u64) {
    let _guard = minitrace::new_span(SyncJob::Func1);
    std::thread::sleep(std::time::Duration::from_millis(i));
    func2(i);
}

#[minitrace::trace(SyncJob::Func2)]
fn func2(i: u64) {
    std::thread::sleep(std::time::Duration::from_millis(i));
}

fn main() {
    let (root, collector) = minitrace::trace_enable(SyncJob::Root);
    minitrace::property(b"sample property:it works");
    {
        let _guard = root;
        for i in 1..=10 {
            func1(i);
        }
    }

    let s = collector.collect();

    #[cfg(feature = "jaeger")]
    {
        let mut buf = Vec::with_capacity(2048);
        minitrace::jaeger::thrift_compact_encode(
            &mut buf,
            "Sync Example",
            &s,
            |e| {
                format!("{:?}", unsafe {
                    std::mem::transmute::<_, SyncJob>(e as u8)
                })
            },
            |property| {
                let mut split = property.splitn(2, |b| *b == b':');
                let key = String::from_utf8_lossy(split.next().unwrap()).to_owned();
                let value = String::from_utf8_lossy(split.next().unwrap()).to_owned();
                (key, value)
            },
        );
        let agent = std::net::SocketAddr::from(([127, 0, 0, 1], 6831));
        let _ = std::net::UdpSocket::bind(std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            0,
        ))
        .and_then(move |s| s.send_to(&buf, agent));
    }

    crate::common::draw_stdout(s);
}
