#![allow(incomplete_features)]
#![feature(generic_associated_types)]

use assert_matches::assert_matches;
use futures::{
    future::{join_all, ready},
    prelude::*,
};
use std::io;
use tarpc::{
    client::{self},
    context, serde_transport,
    server::{self, BaseChannel, Channel, Handler},
    transport::channel,
};
use tokio::join;
use tokio_serde::formats::Json;

#[tarpc_plugins::service]
trait Service {
    async fn add(x: i32, y: i32) -> i32;
    async fn hey(name: String) -> String;
}

#[derive(Clone)]
struct Server;

#[tarpc::server]
impl Service for Server {
    async fn add(&self, _: &mut context::Context, x: i32, y: i32) -> i32 {
        x + y
    }

    async fn hey(&self, _: &mut context::Context, name: String) -> String {
        format!("Hey, {}.", name)
    }
}

#[tokio::test(threaded_scheduler)]
async fn sequential() -> io::Result<()> {
    let _ = env_logger::try_init();

    let (tx, rx) = channel::unbounded();

    tokio::spawn(
        BaseChannel::new(server::Config::default(), rx)
            .requests()
            .execute(Server.serve()),
    );

    let client = ServiceClient::new(client::Config::default(), tx).spawn()?;

    assert_matches!(client.add(context::current(), 1, 2).await, Ok(3));
    assert_matches!(
        client.hey(context::current(), "Tim".into()).await,
        Ok(ref s) if s == "Hey, Tim.");

    Ok(())
}

#[cfg(feature = "serde1")]
#[tokio::test(threaded_scheduler)]
async fn serde() -> io::Result<()> {
    let _ = env_logger::try_init();

    let transport = serde_transport::tcp::listen("localhost:56789", Json::default).await?;
    let addr = transport.local_addr();
    tokio::spawn(
        tarpc::Server::default()
            .incoming(transport.take(1).filter_map(|r| async { r.ok() }))
            .respond_with(Server.serve()),
    );

    let transport = serde_transport::tcp::connect(addr, Json::default()).await?;
    let client = ServiceClient::new(client::Config::default(), transport).spawn()?;

    assert_matches!(client.add(context::current(), 1, 2).await, Ok(3));
    assert_matches!(
        client.hey(context::current(), "Tim".to_string()).await,
        Ok(ref s) if s == "Hey, Tim."
    );

    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn concurrent() -> io::Result<()> {
    let _ = env_logger::try_init();

    let (tx, rx) = channel::unbounded();
    tokio::spawn(
        tarpc::Server::default()
            .incoming(stream::once(ready(rx)))
            .respond_with(Server.serve()),
    );

    let client = ServiceClient::new(client::Config::default(), tx).spawn()?;

    let req1 = client.add(context::current(), 1, 2);
    let req2 = client.add(context::current(), 3, 4);
    let req3 = client.hey(context::current(), "Tim".to_string());

    assert_matches!(req1.await, Ok(3));
    assert_matches!(req2.await, Ok(7));
    assert_matches!(req3.await, Ok(ref s) if s == "Hey, Tim.");

    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn concurrent_join() -> io::Result<()> {
    let _ = env_logger::try_init();

    let (tx, rx) = channel::unbounded();
    tokio::spawn(
        tarpc::Server::default()
            .incoming(stream::once(ready(rx)))
            .respond_with(Server.serve()),
    );

    let client = ServiceClient::new(client::Config::default(), tx).spawn()?;
    let req1 = client.add(context::current(), 1, 2);
    let req2 = client.add(context::current(), 3, 4);
    let req3 = client.hey(context::current(), "Tim".to_string());

    let (resp1, resp2, resp3) = join!(req1, req2, req3);
    assert_matches!(resp1, Ok(3));
    assert_matches!(resp2, Ok(7));
    assert_matches!(resp3, Ok(ref s) if s == "Hey, Tim.");

    Ok(())
}

#[tokio::test(threaded_scheduler)]
async fn concurrent_join_all() -> io::Result<()> {
    let _ = env_logger::try_init();

    let (tx, rx) = channel::unbounded();
    tokio::spawn(
        tarpc::Server::default()
            .incoming(stream::once(ready(rx)))
            .respond_with(Server.serve()),
    );

    let client = ServiceClient::new(client::Config::default(), tx).spawn()?;
    let req1 = client.add(context::current(), 1, 2);
    let req2 = client.add(context::current(), 3, 4);

    let responses = join_all(vec![req1, req2]).await;
    assert_matches!(responses[0], Ok(3));
    assert_matches!(responses[1], Ok(7));

    Ok(())
}
