// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Provides a server that concurrently handles many connections sending multiplexed requests.

use crate::{context, ClientMessage, Response, Transport};
use futures::prelude::*;
use std::{fmt, hash::Hash};

#[cfg(feature = "tokio1")]
use {
    futures::ready,
    pin_project::pin_project,
    std::{
        pin::Pin,
        task::{Context, Poll},
    },
};

pub use channel::Channel;

/// Provides the [`Channel`] abstraction.
pub mod channel;

#[cfg(test)]
mod testing;

/// Returns a stream of server channels.
pub fn incoming<Req, Resp, S, T>(listener: S) -> impl Incoming<channel::BaseChannel<Req, Resp, T>>
where
    S: Stream<Item = T>,
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    listener.map(channel::BaseChannel::new)
}

/// Equivalent to a `FnMut(Req) -> impl Future<Output = Resp>`.
pub trait Serve<Req> {
    /// Type of response.
    type Resp;

    /// Type of response future.
    #[rustfmt::skip]
    type Fut<'a>: Future<Output = Self::Resp> where Self: 'a;

    /// Responds to a single request.
    fn serve<'a>(&'a mut self, ctx: &'a mut context::Context, req: Req) -> Self::Fut<'a>;
}

/// Converts `FnMut(&mut context::Context, Req) -> impl Future<Output = Response>` to
/// a `Serve<Req>` impl.
pub fn serve<Req, F, Fut>(f: F) -> ServeFn<F>
where
    F: FnMut(&mut context::Context, Req) -> Fut,
    Fut: Future,
{
    ServeFn(f)
}

/// A wrapper around a FnMut that impls Serve.
#[derive(Clone, Copy, Debug)]
pub struct ServeFn<F>(F);

impl<Req, Resp, F, Fut> Serve<Req> for ServeFn<F>
where
    F: FnMut(&mut context::Context, Req) -> Fut,
    Fut: Future<Output = Resp>,
{
    type Resp = Resp;
    type Fut<'a> = Fut;

    fn serve<'a>(&'a mut self, ctx: &'a mut context::Context, req: Req) -> Self::Fut<'a> {
        self.0(ctx, req)
    }
}

impl<Req, S> Serve<Req> for &mut S
where
    S: Serve<Req>,
{
    type Resp = S::Resp;
    type Fut<'a> = S::Fut<'a>;

    fn serve<'a>(&'a mut self, ctx: &'a mut context::Context, req: Req) -> Self::Fut<'a> {
        <S as Serve<_>>::serve(*self, ctx, req)
    }
}

/// An extension trait for [streams](Stream) of [`Channels`](Channel).
pub trait Incoming<C>
where
    Self: Sized + Stream<Item = C>,
    C: Channel,
{
    /// Enforces channel per-key limits.
    fn max_channels_per_key<K, KF>(
        self,
        n: u32,
        keymaker: KF,
    ) -> channel::ChannelFilter<Self, K, KF>
    where
        K: fmt::Display + Eq + Hash + Clone + Unpin,
        KF: Fn(&C) -> K,
    {
        channel::ChannelFilter::new(self, n, keymaker)
    }

    /// Caps the number of concurrent requests per channel.
    fn max_concurrent_requests_per_channel(self, n: usize) -> channel::ThrottlerStream<Self> {
        channel::ThrottlerStream::new(self, n)
    }

    /// [Executes](Channel::execute) each incoming channel. Each Channel and each request handler
    /// is [spawned](tokio::spawn) on tokio's default executor.
    #[cfg(feature = "tokio1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
    fn execute<S>(self, serve: S) -> TokioServerExecutor<Self, S>
    where
        S: Serve<C::Req, Resp = C::Resp>,
    {
        TokioServerExecutor { inner: self, serve }
    }
}

impl<S, C> Incoming<C> for S
where
    S: Sized + Stream<Item = C>,
    C: Channel,
{
}

/// A future that drives the server by [spawning](tokio::spawn) a
/// [`TokioChannelExecutor`](channel::TokioChannelExecutor) for each new channel.
#[pin_project]
#[derive(Debug)]
#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
pub struct TokioServerExecutor<T, S> {
    #[pin]
    inner: T,
    serve: S,
}

#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
impl<T, S> TokioServerExecutor<T, S> {
    fn inner_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut T> {
        self.as_mut().project().inner
    }
}

#[cfg(feature = "tokio1")]
impl<St, C, Se> Future for TokioServerExecutor<St, Se>
where
    St: Sized + Stream<Item = C>,
    C: Channel + Send + 'static,
    C::Req: Send + 'static,
    C::Resp: Send + 'static,
    Se: Serve<C::Req, Resp = C::Resp> + Send + Sync + 'static + Clone,
    for<'a> Se::Fut<'a>: send::Send<'a>,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        while let Some(channel) = ready!(self.inner_pin_mut().poll_next(cx)) {
            tokio::spawn(channel.execute(self.serve.clone()));
        }
        log::info!("Server shutting down.");
        Poll::Ready(())
    }
}

mod send {
    /// This variant Send trait includes a lifetime bound, which is (for
    /// some reason) required in order to make the trait bounds work in generic code.
    /// See https://github.com/rust-lang/rust/issues/56556
    pub trait Send<'a>: std::marker::Send {}
    impl<'a, T: std::marker::Send> Send<'a> for T {}
}
