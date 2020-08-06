#![allow(incomplete_features)]
#![feature(generic_associated_types, type_alias_impl_trait)]

use assert_matches::assert_matches;
use futures::{
    future::{join_all, ready},
    prelude::*,
};
use static_assertions::_core::cell::RefCell;
use std::{
    collections::HashMap,
    io,
    time::{Duration, SystemTime},
};
use tarpc::{
    client::{self},
    context, serde_transport,
    server::{self, BaseChannel, Channel, Handler},
    transport::channel,
};
use tokio::{join, task};
use tokio_serde::formats::Json;

#[tarpc::service]
trait Service {
    async fn add(x: i32, y: i32) -> i32;
    async fn hey(name: String) -> String;
}

#[derive(Clone)]
struct Server;

#[tarpc::server]
impl Service for Server {
    async fn add(&mut self, _: &mut context::Context, x: i32, y: i32) -> i32 {
        x + y
    }

    async fn hey(&mut self, _: &mut context::Context, name: String) -> String {
        format!("Hey, {}.", name)
    }
}

#[tokio::test(threaded_scheduler)]
async fn sequential() -> io::Result<()> {
    let _ = env_logger::try_init();

    let (tx, rx) = channel::unbounded();

    tokio::spawn(BaseChannel::new(server::Config::default(), rx).execute(Server.serve()));

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
            .execute(Server.serve()),
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
            .execute(Server.serve()),
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
            .execute(Server.serve()),
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
            .execute(Server.serve()),
    );

    let client = ServiceClient::new(client::Config::default(), tx).spawn()?;
    let req1 = client.add(context::current(), 1, 2);
    let req2 = client.add(context::current(), 3, 4);

    let responses = join_all(vec![req1, req2]).await;
    assert_matches!(responses[0], Ok(3));
    assert_matches!(responses[1], Ok(7));

    Ok(())
}

#[tarpc::service(derive_serde = false)]
trait JustCancel {
    async fn wait(key: &'static str, started: async_channel::Sender<()>, done: async_channel::Sender<()>);
    async fn status(key: &'static str) -> Option<Status>;
}

#[derive(Debug)]
enum Status {
    Done,
    NotDone,
}

struct JustCancelServer {
    requests: RefCell<HashMap<&'static str, Status>>,
}

impl<'b> JustCancel for &'b JustCancelServer {
    #[rustfmt::skip]
    type WaitFut<'a> where 'b: 'a = impl Future<Output = ()> + 'a;

    fn wait<'a>(
        &'a mut self,
        ctx: &'a mut context::Context,
        key: &'static str,
        started: async_channel::Sender<()>,
        _done: async_channel::Sender<()>,
    ) -> Self::WaitFut<'a> {
        async move {
            self.requests
                .borrow_mut()
                .insert(key.clone(), Status::NotDone);
            let _ = started.send(()).await;
            tokio::time::delay_for(
                ctx.deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default()
                    - Duration::from_secs(2),
            )
            .await;
            if let Some(b) = self.requests.borrow_mut().get_mut(&key) {
                *b = Status::Done;
            }
            log::info!("requests: {:?}", self.requests);
        }
    }

    #[rustfmt::skip]
    type StatusFut<'a> where 'b: 'a = impl Future<Output = Option<Status>> + 'a;

    fn status<'a>(&'a mut self, _: &'a mut context::Context, key: &'static str) -> Self::StatusFut<'a> {
        async move { self.requests.borrow_mut().remove(&key) }
    }
}

#[tokio::test(basic_scheduler)]
async fn cancellation() -> io::Result<()> {
    let _ = env_logger::try_init();
    let local = task::LocalSet::new();
    local
        .run_until(async {
            let (tx, rx) = channel::unbounded();
            task::spawn_local(async move {
                // Serve two requests, in order.
                let server = &JustCancelServer {
                    requests: RefCell::new(HashMap::new()),
                };
                BaseChannel::with_defaults(rx)
                    .requests()
                    .for_each_concurrent(None, |handler| async {
                        handler.unwrap().execute(&mut server.serve()).await
                    })
                    .await;
            });
            let client = JustCancelClient::with_defaults(tx).spawn()?;

            let (started, started_rx) = async_channel::unbounded();
            let (done, done_rx) = async_channel::unbounded();

            // Wait for the server to signal it has started processing the request, then immediately
            // cancel the request.
            tokio::select!(
                _ = client.wait(context::current(), "key", started, done) => {}
                _ = started_rx.recv() => {}
            );

            // By waiting for the done signal, we know that the server finished handling the request,
            // either successfully or (hopefully) via cancellation.
            let _ = done_rx.recv().await;

            assert_matches!(
                client.status(context::current(), "key").await,
                Ok(Some(Status::NotDone))
            );

            Ok(())
        })
        .await
}
