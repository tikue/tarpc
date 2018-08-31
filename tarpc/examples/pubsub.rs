// Copyright 2016 Google Inc. All Rights Reserved.
//
// Licensed under the MIT License, <LICENSE or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed except according to those terms.

#![feature(
    plugin,
    futures_api,
    await_macro,
    async_await,
    existential_type
)]
#![plugin(tarpc_plugins)]

extern crate env_logger;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate tarpc;

use futures::{
    compat::TokioDefaultSpawner,
    future::{self, Ready},
    prelude::*,
    Future,
};
use rpc::{
    client,
    server::{self, Handler, Server},
};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub mod subscriber {
    service! {
        rpc receive(message: String);
    }
}

pub mod publisher {
    use std::net::SocketAddr;

    service! {
        rpc broadcast(message: String);
        rpc subscribe(id: u32, address: SocketAddr) -> Result<(), String>;
        rpc unsubscribe(id: u32);
    }
}

#[derive(Clone, Debug)]
struct Subscriber {
    id: u32,
}

impl subscriber::Service for Subscriber {
    type ReceiveFut = Ready<()>;

    fn receive(&self, _: server::Context, message: String) -> Self::ReceiveFut {
        println!("{} received message: {}", self.id, message);
        future::ready(())
    }
}

impl Subscriber {
    async fn listen(id: u32, config: server::Config) -> io::Result<SocketAddr> {
        let incoming = bincode_transport::listen(&"0.0.0.0:0".parse().unwrap())?;
        let addr = incoming.local_addr();
        spawn!(
            Server::new(config)
                .incoming(incoming)
                .take(1)
                .respond_with(subscriber::serve(Subscriber { id }))
        ).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Could not spawn server: {:?}", e),
            )
        })?;
        Ok(addr)
    }
}

#[derive(Clone, Debug)]
struct Publisher {
    clients: Arc<Mutex<HashMap<u32, subscriber::Client>>>,
}

impl Publisher {
    fn new() -> Publisher {
        Publisher {
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl publisher::Service for Publisher {
    existential type BroadcastFut: Future<Output = ()>;

    fn broadcast(&self, _: server::Context, message: String) -> Self::BroadcastFut {
        async fn broadcast(clients: Arc<Mutex<HashMap<u32, subscriber::Client>>>, message: String) {
            let mut clients = clients.lock().unwrap().clone();
            for client in clients.values_mut() {
                // Ignore failing subscribers. In a real pubsub,
                // you'd want to continually retry until subscribers
                // ack.
                let _ = await!(client.receive(client::Context::current(), message.clone()));
            }
        }

        broadcast(self.clients.clone(), message)
    }

    existential type SubscribeFut: Future<Output = Result<(), String>>;

    fn subscribe(&self, _: server::Context, id: u32, addr: SocketAddr) -> Self::SubscribeFut {
        async fn subscribe(
            clients: Arc<Mutex<HashMap<u32, subscriber::Client>>>,
            id: u32,
            addr: SocketAddr,
        ) -> io::Result<()> {
            let conn = await!(bincode_transport::connect(&addr))?;
            let subscriber = await!(subscriber::new_stub(client::Config::default(), conn));
            println!("Subscribing {}.", id);
            clients.lock().unwrap().insert(id, subscriber);
            Ok(())
        }

        subscribe(Arc::clone(&self.clients), id, addr).map_err(|e| e.to_string())
    }

    existential type UnsubscribeFut: Future<Output = ()>;

    fn unsubscribe(&self, _: server::Context, id: u32) -> Self::UnsubscribeFut {
        println!("Unsubscribing {}", id);
        let mut clients = self.clients.lock().unwrap();
        if let None = clients.remove(&id) {
            eprintln!(
                "Client {} not found. Existings clients: {:?}",
                id, &*clients
            );
        }
        future::ready(())
    }
}

async fn run() -> io::Result<()> {
    env_logger::init();
    let transport = bincode_transport::listen(&"0.0.0.0:0".parse().unwrap())?;
    let publisher_addr = transport.local_addr();
    spawn!(
        Server::new(server::Config::default())
            .incoming(transport)
            .take(1)
            .respond_with(publisher::serve(Publisher::new()))
    ).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Could not spawn server: {:?}", e),
        )
    })?;

    let subscriber1 = await!(Subscriber::listen(0, server::Config::default()))?;
    let subscriber2 = await!(Subscriber::listen(1, server::Config::default()))?;

    let publisher_conn = bincode_transport::connect(&publisher_addr);
    let publisher_conn = await!(publisher_conn)?;
    let mut publisher = await!(publisher::new_stub(
        client::Config::default(),
        publisher_conn
    ));

    if let Err(e) = await!(publisher.subscribe(client::Context::current(), 0, subscriber1))? {
        eprintln!("Couldn't subscribe subscriber 0: {}", e);
    }
    if let Err(e) = await!(publisher.subscribe(client::Context::current(), 1, subscriber2))? {
        eprintln!("Couldn't subscribe subscriber 1: {}", e);
    }

    println!("Broadcasting...");
    await!(publisher.broadcast(client::Context::current(), "hello to all".to_string()))?;
    await!(publisher.unsubscribe(client::Context::current(), 1))?;
    await!(publisher.broadcast(client::Context::current(), "hi again".to_string()))?;
    Ok(())
}

fn main() {
    tokio::run(
        run()
            .boxed()
            .map_err(|e| panic!(e))
            .compat(TokioDefaultSpawner),
    );
    thread::sleep(Duration::from_millis(100));
}