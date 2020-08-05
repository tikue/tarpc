// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Provides a client that connects to a server and sends multiplexed requests.

use crate::context;
use futures::prelude::*;
use std::io;

/// Provides a [`Client`] backed by a transport.
pub mod channel;
pub use channel::{new, Channel};

/// Sends multiplexed requests to, and receives responses from, a server.
pub trait Client<Req> {
    /// The response type.
    type Response;

    /// The future response.
    #[rustfmt::skip]
    type Future<'a>: Future<Output = io::Result<Self::Response>> where Self: 'a;

    /// Initiates a request, sending it to the dispatch task.
    ///
    /// Returns a [`Future`] that resolves to this client and the future response
    /// once the request is successfully enqueued.
    ///
    /// [`Future`]: futures::Future
    fn call(&mut self, ctx: context::Context, request: Req) -> Self::Future<'_>;

    /// Returns a Client that applies a post-processing function to the returned response.
    fn map_response<F, R>(self, f: F) -> MapResponse<Self, F>
    where
        F: FnMut(Self::Response) -> R,
        Self: Sized,
    {
        MapResponse { inner: self, f }
    }

    /// Returns a Client that applies a pre-processing function to the request.
    fn with_request<F, Req2>(self, f: F) -> WithRequest<Self, F>
    where
        F: FnMut(Req2) -> Req,
        Self: Sized,
    {
        WithRequest { inner: self, f }
    }

    /// Returns a Client that applies a pre-processing function to the request
    /// [`context`](context::Context). This can be used to enable context extensions,
    /// which may be used by the transport.
    fn with_context<F>(self, f: F) -> WithContext<Self, F>
    where
        F: FnMut(&mut context::Context),
        Self: Sized,
    {
        WithContext { inner: self, f }
    }
}

/// A Client that applies a function to the returned response.
#[derive(Clone, Debug)]
pub struct MapResponse<C, F> {
    inner: C,
    f: F,
}

impl<C, F, Req, Resp, Resp2> Client<Req> for MapResponse<C, F>
where
    C: Client<Req, Response = Resp>,
    F: FnMut(Resp) -> Resp2,
{
    type Response = Resp2;
    #[rustfmt::skip]
    type Future<'a> where Self: 'a = futures::future::MapOk<<C as Client<Req>>::Future<'a>, &'a mut F>;

    fn call(&mut self, ctx: context::Context, request: Req) -> Self::Future<'_> {
        self.inner.call(ctx, request).map_ok(&mut self.f)
    }
}

/// A Client that applies a pre-processing function to the request.
#[derive(Clone, Debug)]
pub struct WithRequest<C, F> {
    inner: C,
    f: F,
}

impl<C, F, Req, Req2, Resp> Client<Req2> for WithRequest<C, F>
where
    C: Client<Req, Response = Resp>,
    F: FnMut(Req2) -> Req,
{
    type Response = Resp;
    type Future<'a> = <C as Client<Req>>::Future<'a>;

    fn call(&mut self, ctx: context::Context, request: Req2) -> Self::Future<'_> {
        self.inner.call(ctx, (self.f)(request))
    }
}

/// A Client that applies a pre-processing function to the request [`Context`](context::Context).
#[derive(Clone, Debug)]
pub struct WithContext<C, F> {
    inner: C,
    f: F,
}

impl<C, F, Req, Resp> Client<Req> for WithContext<C, F>
where
    C: Client<Req, Response = Resp>,
    F: FnMut(&mut context::Context),
{
    type Response = Resp;
    type Future<'a> = <C as Client<Req>>::Future<'a>;

    fn call(&mut self, mut ctx: context::Context, request: Req) -> Self::Future<'_> {
        (self.f)(&mut ctx);
        self.inner.call(ctx, request)
    }
}

impl<Req, Resp> Client<Req> for Channel<Req, Resp> {
    type Response = Resp;
    #[rustfmt::skip]
    type Future<'a> where Self: 'a = impl Future<Output = io::Result<Self::Response>>;

    fn call(&mut self, ctx: context::Context, request: Req) -> Self::Future<'_> {
        self.call(ctx, request)
    }
}

#[allow(unused)]
fn ensure_channel_future_is_send_when_req_and_resp_are_send() {
    struct IsSend;
    static_assertions::assert_impl_all!(IsSend: Send);

    static_assertions::assert_impl_all!(<Channel<IsSend, IsSend> as Client<_>>::Future<'_>: Send);
}

/// Settings that control the behavior of the client.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Config {
    /// The number of requests that can be in flight at once.
    /// `max_in_flight_requests` controls the size of the map used by the client
    /// for storing pending requests.
    pub max_in_flight_requests: usize,
    /// The number of requests that can be buffered client-side before being sent.
    /// `pending_requests_buffer` controls the size of the channel clients use
    /// to communicate with the request dispatch task.
    pub pending_request_buffer: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_in_flight_requests: 1_000,
            pending_request_buffer: 100,
        }
    }
}

/// A channel and dispatch pair. The dispatch drives the sending and receiving of requests
/// and must be polled continuously or spawned.
#[derive(Debug)]
pub struct NewClient<C, D> {
    /// The new client.
    pub client: C,
    /// The client's dispatch.
    pub dispatch: D,
}

impl<C, D, E> NewClient<C, D>
where
    D: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display,
{
    /// Helper method to spawn the dispatch on the default executor.
    #[cfg(feature = "tokio1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
    pub fn spawn(self) -> io::Result<C> {
        use log::error;

        let dispatch = self
            .dispatch
            .unwrap_or_else(move |e| error!("Connection broken: {}", e));
        tokio::spawn(dispatch);
        Ok(self.client)
    }
}
