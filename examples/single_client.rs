// Copyright 2016 Google Inc. All Rights Reserved.
//
// Licensed under the MIT License, <LICENSE or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed except according to those terms.

#[macro_use]
extern crate tarpc;

extern crate env_logger;
#[macro_use]
extern crate log;

use std::time::{Duration, Instant};

service! {
    rpc hello(s: String) -> String;
}

struct HelloServer;
impl Service for HelloServer {
    fn hello(&self, s: String) -> String {
        format!("Hello, {}!", s)
    }
}

fn main() {
    let _ = env_logger::init();
    let handle = HelloServer.spawn("localhost:0").unwrap();
    let client = FutureClient::dial(&handle.dialer()).unwrap();
    let concurrency = 100;
    let mut futures = Vec::with_capacity(concurrency);

    info!("Starting...");
    let start = Instant::now();
    let max = Duration::from_secs(10);
    let mut total_rpcs = 0;

    while start.elapsed() < max {
        for _ in 0..concurrency {
            futures.push(client.hello("Bob".into()));
        }
        for f in futures.drain(..) {
            f.get().unwrap();
        }
        total_rpcs += concurrency;
    }
    info!("Done. Total rpcs in 10s: {}", total_rpcs);
    client.shutdown().unwrap();
    handle.shutdown();
}
