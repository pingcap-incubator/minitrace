// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

mod common;
use minitrace::prelude::*;

#[derive(Debug)]
enum AsyncJob {
    #[allow(dead_code)]
    Unknown,
    Root,
    IterJob,
    OtherJob,
}

impl Into<u32> for AsyncJob {
    fn into(self) -> u32 {
        self as u32
    }
}

fn parallel_job() -> Vec<tokio::task::JoinHandle<()>> {
    let mut v = Vec::with_capacity(4);
    for i in 0..4 {
        v.push(tokio::spawn(iter_job(i).trace_task(AsyncJob::IterJob)));
    }
    v
}

async fn iter_job(iter: u64) {
    std::thread::sleep(std::time::Duration::from_millis(iter * 10));
    tokio::task::yield_now().await;
    other_job().await;
}

#[minitrace::trace_async(AsyncJob::OtherJob)]
async fn other_job() {
    for i in 0..20 {
        if i == 10 {
            tokio::task::yield_now().await;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[cfg(feature = "jaeger")]
use clap::Clap;
#[cfg(feature = "jaeger")]
#[derive(Clap)]
struct Opts {
    /// Sets a custom config file. Could have been an Option<T> with no default too
    #[clap(long, default_value = "127.0.0.1:6831")]
    jaeger: String,
}

#[tokio::main]
async fn main() {
    let (collector, _) = async {
        minitrace::property(b"sample property:it works");
        let jhs = parallel_job();
        other_job().await;

        for jh in jhs {
            jh.await.unwrap();
        }
    }
    .future_trace_enable(AsyncJob::Root)
    .await;

    let trace_details = collector.collect();

    #[cfg(feature = "jaeger")]
    {
        use minitrace::jaeger::{JaegerSpanInfo, ReferenceType};
        use minitrace::State;

        use std::net::SocketAddr;
        let opts = Opts::parse();
        let mut buf = Vec::with_capacity(2048);
        minitrace::jaeger::thrift_compact_encode(
            &mut buf,
            "Async Example",
            rand::random(),
            rand::random(),
            &trace_details,
            |s| JaegerSpanInfo {
                self_id: s.id as _,
                parent_id: s.related_id as _,
                reference_type: match s.state {
                    State::Root => ReferenceType::ChildOf,
                    State::Local => ReferenceType::ChildOf,
                    State::Spawning => ReferenceType::FollowFrom,
                    State::Scheduling => ReferenceType::FollowFrom,
                    State::Settle => ReferenceType::FollowFrom,
                },
                operation_name: {
                    format!(
                        "{}{:?}",
                        match s.state {
                            State::Root => "[Root] ",
                            State::Local => "",
                            State::Spawning => "[Spawning] ",
                            State::Scheduling => "[Scheduling] ",
                            State::Settle => "",
                        },
                        unsafe { std::mem::transmute::<_, AsyncJob>(s.event as u8) }
                    )
                },
            },
            |property| {
                let mut split = property.splitn(2, |b| *b == b':');
                let key = String::from_utf8_lossy(split.next().unwrap()).to_owned();
                let value = String::from_utf8_lossy(split.next().unwrap()).to_owned();
                (key, value)
            },
        );

        let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        if let Ok(mut socket) = tokio::net::UdpSocket::bind(local_addr).await {
            let _ = socket.send_to(&buf, opts.jaeger).await;
        }
    }

    crate::common::draw_stdout(trace_details);
}
