// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Provides a server that concurrently handles many connections sending multiplexed requests.

use crate::{
    context, trace, util::Compact, util::TimeUntil, ClientMessage, PollIo, Request, Response,
    ServerError, Transport,
};
use fnv::FnvHashMap;
use futures::{
    channel::mpsc,
    future::{AbortHandle, AbortRegistration, Abortable},
    prelude::*,
    ready,
    stream::Fuse,
};
use humantime::format_rfc3339;
use log::{debug, trace};
use pin_project::pin_project;
use std::{
    fmt,
    hash::Hash,
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

mod filter;
#[cfg(test)]
mod testing;
mod throttle;

pub use self::{
    filter::ChannelFilter,
    throttle::{Throttler, ThrottlerStream},
};

/// Manages clients, serving multiplexed requests over each connection.
#[derive(Debug)]
pub struct Server<Req, Resp> {
    config: Config,
    ghost: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> Default for Server<Req, Resp> {
    fn default() -> Self {
        new(Config::default())
    }
}

/// Settings that control the behavior of the server.
#[derive(Clone, Debug)]
pub struct Config {
    /// The number of responses per client that can be buffered server-side before being sent.
    /// `pending_response_buffer` controls the buffer size of the channel that a server's
    /// response tasks use to send responses to the client handler task.
    pub pending_response_buffer: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pending_response_buffer: 100,
        }
    }
}

impl Config {
    /// Returns a channel backed by `transport` and configured with `self`.
    pub fn channel<Req, Resp, T>(self, transport: T) -> BaseChannel<Req, Resp, T>
    where
        T: Transport<Response<Resp>, ClientMessage<Req>>,
    {
        BaseChannel::new(self, transport)
    }
}

/// Returns a new server with configuration specified `config`.
pub fn new<Req, Resp>(config: Config) -> Server<Req, Resp> {
    Server {
        config,
        ghost: PhantomData,
    }
}

impl<Req, Resp> Server<Req, Resp> {
    /// Returns the config for this server.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns a stream of server channels.
    pub fn incoming<S, T>(self, listener: S) -> impl Stream<Item = BaseChannel<Req, Resp, T>>
    where
        S: Stream<Item = T>,
        T: Transport<Response<Resp>, ClientMessage<Req>>,
    {
        listener.map(move |t| BaseChannel::new(self.config.clone(), t))
    }
}

/// Basically a Fn(Req) -> impl Future<Output = Resp>;
pub trait Serve<Req>: Sized + Clone {
    /// Type of response.
    type Resp;

    /// Type of response future.
    #[rustfmt::skip]
    type Fut<'a>: Future<Output = Self::Resp> + Send where Self: 'a;

    /// Responds to a single request.
    fn serve<'a>(self, ctx: &'a mut context::Context, req: Req) -> Self::Fut<'a>;
}

/// A utility trait enabling a stream to fluently chain a request handler.
pub trait Handler<C>
where
    Self: Sized + Stream<Item = C>,
    C: Channel,
{
    /// Enforces channel per-key limits.
    fn max_channels_per_key<K, KF>(self, n: u32, keymaker: KF) -> filter::ChannelFilter<Self, K, KF>
    where
        K: fmt::Display + Eq + Hash + Clone + Unpin,
        KF: Fn(&C) -> K,
    {
        ChannelFilter::new(self, n, keymaker)
    }

    /// Caps the number of concurrent requests per channel.
    fn max_concurrent_requests_per_channel(self, n: usize) -> ThrottlerStream<Self> {
        ThrottlerStream::new(self, n)
    }

    /// Responds to all requests with [`server::serve`](Serve).
    #[cfg(feature = "tokio1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
    fn respond_with<S>(self, server: S) -> Running<Self, S>
    where
        S: Serve<C::Req, Resp = C::Resp>,
    {
        Running {
            incoming: self,
            server,
        }
    }
}

impl<S, C> Handler<C> for S
where
    S: Sized + Stream<Item = C>,
    C: Channel,
{
}

/// BaseChannel lifts a Transport to a Channel by tracking in-flight requests.
#[pin_project]
#[derive(Debug)]
pub struct BaseChannel<Req, Resp, T> {
    config: Config,
    /// Writes responses to the wire and reads requests off the wire.
    #[pin]
    transport: Fuse<T>,
    /// Number of requests currently being responded to.
    in_flight_requests: FnvHashMap<u64, AbortHandle>,
    /// Types the request and response.
    ghost: PhantomData<(Req, Resp)>,
}

impl<Req, Resp, T> BaseChannel<Req, Resp, T>
where
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    /// Creates a new channel backed by `transport` and configured with `config`.
    pub fn new(config: Config, transport: T) -> Self {
        BaseChannel {
            config,
            transport: transport.fuse(),
            in_flight_requests: FnvHashMap::default(),
            ghost: PhantomData,
        }
    }

    /// Creates a new channel backed by `transport` and configured with the defaults.
    pub fn with_defaults(transport: T) -> Self {
        Self::new(Config::default(), transport)
    }

    /// Returns the inner transport over which messages are sent and received.
    pub fn get_ref(&self) -> &T {
        self.transport.get_ref()
    }

    /// Returns the inner transport over which messages are sent and received.
    pub fn get_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut T> {
        self.as_mut().project().transport.get_pin_mut()
    }

    /// Returns a mutable reference to the map of in-flight requests.
    fn requests_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> &'a mut FnvHashMap<u64, AbortHandle> {
        self.as_mut().project().in_flight_requests
    }

    fn cancel_request(mut self: Pin<&mut Self>, trace_context: &trace::Context, request_id: u64) {
        // It's possible the request was already completed, so it's fine
        // if this is None.
        if let Some(cancel_handle) = self.requests_pin_mut().remove(&request_id) {
            self.requests_pin_mut().compact(0.1);

            cancel_handle.abort();
            let remaining = self.in_flight_requests.len();
            trace!(
                "[{}] Request canceled. In-flight requests = {}",
                trace_context.trace_id,
                remaining,
            );
        } else {
            trace!(
                "[{}] Received cancellation, but response handler \
                 is already complete.",
                trace_context.trace_id,
            );
        }
    }
}

/// The server end of an open connection with a client, streaming in requests from, and sinking
/// responses to, the client.
///
/// Channels are free to somewhat rely on the assumption that all in-flight requests are eventually
/// either [cancelled](BaseChannel::cancel_request) or [responded to](Sink::start_send). Safety cannot
/// rely on this assumption, but it is best for `Channel` users to always account for all outstanding
/// requests.
pub trait Channel
where
    Self: Transport<Response<<Self as Channel>::Resp>, Request<<Self as Channel>::Req>>,
{
    /// Type of request item.
    type Req;

    /// Type of response sink item.
    type Resp;

    /// Configuration of the channel.
    fn config(&self) -> &Config;

    /// Returns the number of in-flight requests over this channel.
    fn in_flight_requests(&self) -> usize;

    /// Caps the number of concurrent requests.
    fn max_concurrent_requests(self, n: usize) -> Throttler<Self>
    where
        Self: Sized,
    {
        Throttler::new(self, n)
    }

    /// Tells the Channel that request with ID `request_id` is being handled.
    /// The request will be tracked until a response with the same ID is sent
    /// to the Channel.
    fn start_request(self: Pin<&mut Self>, request_id: u64) -> AbortRegistration;

    /// Respond to requests coming over the channel with `f`. Returns a future that drives the
    /// responses and resolves when the connection is closed.
    fn respond_with<S>(self, server: S) -> ClientHandler<Self, S>
    where
        S: Serve<Self::Req, Resp = Self::Resp>,
        Self: Sized,
    {
        let (responses_tx, responses) = mpsc::channel(self.config().pending_response_buffer);
        let responses = responses.fuse();

        ClientHandler {
            channel: self,
            server,
            pending_responses: responses,
            responses_tx,
        }
    }
}

impl<Req, Resp, T> Stream for BaseChannel<Req, Resp, T>
where
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    type Item = io::Result<Request<Req>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            match ready!(self.get_pin_mut().poll_next(cx)?) {
                Some(message) => match message {
                    ClientMessage::Request(request) => {
                        return Poll::Ready(Some(Ok(request)));
                    }
                    ClientMessage::Cancel {
                        context,
                        request_id,
                    } => {
                        self.as_mut()
                            .cancel_request(&context.trace_context, request_id);
                    }
                },
                None => return Poll::Ready(None),
            }
        }
    }
}

impl<Req, Resp, T> Sink<Response<Resp>> for BaseChannel<Req, Resp, T>
where
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_pin_mut().poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, response: Response<Resp>) -> Result<(), Self::Error> {
        if self
            .requests_pin_mut()
            .remove(&response.request_id)
            .is_some()
        {
            self.requests_pin_mut().compact(0.1);
        }

        self.get_pin_mut().start_send(response)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_pin_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_pin_mut().poll_close(cx)
    }
}

impl<Req, Resp, T> AsRef<T> for BaseChannel<Req, Resp, T> {
    fn as_ref(&self) -> &T {
        self.transport.get_ref()
    }
}

impl<Req, Resp, T> Channel for BaseChannel<Req, Resp, T>
where
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    type Req = Req;
    type Resp = Resp;

    fn config(&self) -> &Config {
        &self.config
    }

    fn in_flight_requests(&self) -> usize {
        self.in_flight_requests.len()
    }

    fn start_request(mut self: Pin<&mut Self>, request_id: u64) -> AbortRegistration {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        assert!(self
            .requests_pin_mut()
            .insert(request_id, abort_handle)
            .is_none());
        abort_registration
    }
}

/// A running handler serving all requests coming over a channel.
#[pin_project]
#[derive(Debug)]
pub struct ClientHandler<C, S>
where
    C: Channel,
{
    #[pin]
    channel: C,
    /// Responses waiting to be written to the wire.
    #[pin]
    pending_responses: Fuse<mpsc::Receiver<Response<C::Resp>>>,
    /// Handed out to request handlers to fan in responses.
    #[pin]
    responses_tx: mpsc::Sender<Response<C::Resp>>,
    /// Server
    server: S,
}

impl<C, S> ClientHandler<C, S>
where
    C: Channel,
    S: Serve<C::Req, Resp = C::Resp>,
{
    /// Returns the inner channel over which messages are sent and received.
    pub fn channel_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut C> {
        self.as_mut().project().channel
    }

    fn pump_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> PollIo<ResponseHandler<C::Req, C::Resp, S>> {
        match ready!(self.channel_pin_mut().poll_next(cx)?) {
            Some(request) => {
                let abort_registration = self.channel_pin_mut().start_request(request.id);
                Poll::Ready(Some(Ok(ResponseHandler {
                    request,
                    serve: self.server.clone(),
                    response_tx: self.responses_tx.clone(),
                    abort_registration,
                })))
            }
            None => Poll::Ready(None),
        }
    }

    fn pump_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        read_half_closed: bool,
    ) -> PollIo<()> {
        match self.as_mut().poll_next_response(cx)? {
            Poll::Ready(Some(response)) => {
                trace!(
                    "[{}] Staging response. In-flight requests = {}.",
                    response.context.trace_id(),
                    self.channel.in_flight_requests(),
                );
                self.channel_pin_mut().start_send(response)?;
                Poll::Ready(Some(Ok(())))
            }
            Poll::Ready(None) => {
                // Shutdown can't be done before we finish pumping out remaining responses.
                ready!(self.channel_pin_mut().poll_flush(cx)?);
                Poll::Ready(None)
            }
            Poll::Pending => {
                // No more requests to process, so flush any requests buffered in the transport.
                ready!(self.channel_pin_mut().poll_flush(cx)?);

                // Being here means there are no staged requests and all written responses are
                // fully flushed. So, if the read half is closed and there are no in-flight
                // requests, then we can close the write half.
                if read_half_closed && self.channel.in_flight_requests() == 0 {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_next_response(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> PollIo<Response<C::Resp>> {
        // Ensure there's room to write a response.
        while let Poll::Pending = self.channel_pin_mut().poll_ready(cx)? {
            ready!(self.channel_pin_mut().poll_flush(cx)?);
        }

        match ready!(self.as_mut().project().pending_responses.poll_next(cx)) {
            Some(response) => Poll::Ready(Some(Ok(response))),
            None => {
                // This branch likely won't happen, since the ClientHandler is holding a Sender.
                Poll::Ready(None)
            }
        }
    }
}

/// Executes the handling of a single request.
#[derive(Debug)]
pub struct ResponseHandler<Req, Res, S> {
    request: Request<Req>,
    serve: S,
    response_tx: mpsc::Sender<Response<Res>>,
    abort_registration: AbortRegistration,
}

impl<Req, Res, S> ResponseHandler<Req, Res, S>
where
    S: Serve<Req, Resp = Res>,
{
    /// Runs until response completion.
    pub fn execute(self) -> impl Future<Output = ()> {
        let Self {
            abort_registration,
            request,
            mut response_tx,
            serve,
        } = self;
        let trace_id = *request.context.trace_id();
        let deadline = request.context.deadline;
        let timeout = deadline.time_until();
        trace!(
            "[{}] Handling request with deadline {} (timeout {:?}).",
            trace_id,
            format_rfc3339(deadline),
            timeout,
        );
        Abortable::new(
            async move {
                let Request {
                    mut context,
                    message,
                    id: request_id,
                } = request;
                let result = tokio::time::timeout(timeout, async {
                    serve.serve(&mut context, message).await
                })
                .await;
                let response = Response {
                    context,
                    request_id,
                    message: match result {
                        Ok(message) => Ok(message),
                        Err(tokio::time::Elapsed { .. }) => {
                            debug!(
                                "[{}] Response did not complete before deadline of {}s.",
                                trace_id,
                                format_rfc3339(deadline)
                            );
                            // No point in responding, since the client will have dropped the
                            // request.
                            Err(ServerError {
                                kind: io::ErrorKind::TimedOut,
                                detail: Some(format!(
                                    "Response did not complete before deadline of {}s.",
                                    format_rfc3339(deadline)
                                )),
                            })
                        }
                    },
                };
                let _ = response_tx.send(response).await;
            },
            abort_registration,
        )
        .unwrap_or_else(|_| {})
    }
}

impl<C, S> Stream for ClientHandler<C, S>
where
    C: Channel,
    S: Serve<C::Req, Resp = C::Resp>,
{
    type Item = io::Result<ResponseHandler<C::Req, C::Resp, S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let read = self.as_mut().pump_read(cx)?;
            let read_closed = if let Poll::Ready(None) = read {
                true
            } else {
                false
            };
            match (read, self.as_mut().pump_write(cx, read_closed)?) {
                (Poll::Ready(None), Poll::Ready(None)) => {
                    return Poll::Ready(None);
                }
                (Poll::Ready(Some(request_handler)), _) => {
                    return Poll::Ready(Some(Ok(request_handler)));
                }
                (_, Poll::Ready(Some(()))) => {}
                _ => {
                    return Poll::Pending;
                }
            }
        }
    }
}

// Send + 'static execution helper methods.

impl<C, S> ClientHandler<C, S>
where
    C: Channel + 'static,
    C::Req: Send + 'static,
    C::Resp: Send + 'static,
    S: Serve<C::Req, Resp = C::Resp> + Send + 'static,
{
    /// Runs the client handler until completion by [spawning](tokio::spawn) each
    /// request handler onto the default executor.
    #[cfg(feature = "tokio1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
    pub fn execute(self) -> impl Future<Output = ()> {
        self.try_for_each(|resp| async {
            tokio::spawn(resp.execute());
            Ok(())
        })
        .map_ok(|()| log::info!("ClientHandler finished."))
        .unwrap_or_else(|e| log::info!("ClientHandler errored out: {}", e))
    }
}

/// A future that drives the server by [spawning](tokio::spawn) channels and request handlers on the default
/// executor.
#[pin_project]
#[derive(Debug)]
#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
pub struct Running<St, Se> {
    #[pin]
    incoming: St,
    server: Se,
}

#[cfg(feature = "tokio1")]
impl<St, C, Se> Future for Running<St, Se>
where
    St: Sized + Stream<Item = C>,
    C: Channel + Send + 'static,
    C::Req: Send + 'static,
    C::Resp: Send + 'static,
    Se: Serve<C::Req, Resp = C::Resp> + Send + 'static + Clone,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        while let Some(channel) = ready!(self.as_mut().project().incoming.poll_next(cx)) {
            tokio::spawn(
                channel
                    .respond_with(self.as_mut().project().server.clone())
                    .execute(),
            );
        }
        log::info!("Server shutting down.");
        Poll::Ready(())
    }
}
