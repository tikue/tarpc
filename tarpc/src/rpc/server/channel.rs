use crate::{
    server::{send, Serve},
    trace,
    util::Compact,
    util::TimeUntil,
    ClientMessage, PollIo, Request, Response, ServerError, Transport,
};
use fnv::FnvHashMap;
use futures::{
    future::{AbortHandle, AbortRegistration, Abortable},
    prelude::*,
    ready,
    stream::Fuse,
};
use humantime::format_rfc3339;
use log::{debug, trace};
use pin_project::pin_project;
use std::{
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

mod filter;
mod throttle;

pub use {
    filter::ChannelFilter,
    throttle::{Throttler, ThrottlerStream},
};

/// The server end of an open connection with a client, streaming in requests from, and sinking
/// responses to, the client.
///
///
/// The ways to use a Channel, in order of simplest to most complex, is:
/// 1. [Channel::execute] - Requires the `tokio1` feature. This method is best for those who
///    do not have specific scheduling needs and whose services are `Send + 'static`.
/// 2. [Channel::requests] - This method is best for those who need direct access to individual
///    requests, or are not using `tokio`, or want control over [futures](Future) scheduling.
///    Users manually scheduling request executions should strive to let handler futures complete
///    before dropping them, in order to ensure cleanup of all resources associated with the
///    request. TODO: use a drop impl to clean up resources when futures are early-dropped.
/// 3. [Raw stream](<Channel as Stream>) - A user is free to manually handle requests produced by
///    Channel. If they do so, they should uphold the service contract:
///    1. All work being done as part of processing request `request_id` is aborted when
///       either of the following occurs:
///       - The channel receives a [cancellation message](ClientMessage::Cancel) for request
///         `request_id`.
///       - The [deadline](crate::context::Context::deadline) of request `request_id` is reached.
///    2. When a server completes a response for request `request_id`, it is
///       [sent](Sink::start_send) into the Channel. Because there is no guarantee that a
///       cancellation message will ever be received for a request, services should strive to clean
///       up Channel resources by sending a response for every request. For example, [`BaseChannel`]
///       has a map of requests to [abort handles][AbortHandle] whose entries are only removed
///       upon either request cancellation or response completion.
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

    /// Configuration of the channel.
    fn config_mut(&mut self) -> &mut Config;

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

    /// Returns a stream of requests that automatically handle request cancellation and response
    /// routing.
    fn requests(self) -> Requests<Self>
    where
        Self: Sized,
    {
        let (responses_tx, responses) =
            async_channel::bounded(self.config().pending_response_buffer);
        let responses = responses.fuse();

        Requests {
            channel: self,
            pending_responses: responses,
            responses_tx,
        }
    }

    /// Runs the channel until completion by executing all requests using the given service
    /// function. Request handlers are run concurrently by [spawning](tokio::spawn) on tokio's
    /// default executor.
    #[cfg(feature = "tokio1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
    fn execute<S>(self, serve: S) -> TokioChannelExecutor<Requests<Self>, S>
    where
        Self: Sized,
        S: Serve<Self::Req, Resp = Self::Resp> + Send + Sync + 'static,
        for<'a> S::Fut<'a>: send::Send<'a>,
        Self::Req: Send + 'static,
        Self::Resp: Send + 'static,
    {
        self.requests().execute(serve)
    }
}

/// Settings that control the behavior of [channels](Channel).
#[derive(Clone, Debug)]
pub struct Config {
    /// Controls the buffer size of the [`async_channel`] over which a server's
    /// handlers send responses to the [`Channel`]. In other words, this is the number of responses
    /// that can sit in the outbound queue before request handlers begin blocking.
    pub pending_response_buffer: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pending_response_buffer: 100,
        }
    }
}

/// Creates a new [`BaseChannel`] backed by `transport` and configured with the defaults.
pub fn new<Req, Resp, T>(transport: T) -> BaseChannel<Req, Resp, T>
where
    T: Transport<Response<Resp>, ClientMessage<Req>>,
{
    BaseChannel::new(transport)
}

/// BaseChannel is a [Transport] that keeps track of in-flight requests. It coverts a
/// [`Transport`](Transport) whose [`Stream`](Stream) is [`ClientMessages`](ClientMessage)
/// into a [request](ClientMessage::Request) stream.
///
/// Besides requests, the other type of client message is [cancellation
/// messages](ClientMessage::Cancel). `BaseChannel` does not allow direct access to cancellation
/// messages. Instead, it transparently handles them by removing the corresponding in-flight
/// requests and aborting their handlers.
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
    pub fn new(transport: T) -> Self {
        BaseChannel {
            config: Config::default(),
            transport: transport.fuse(),
            in_flight_requests: FnvHashMap::default(),
            ghost: PhantomData,
        }
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

    fn config_mut(&mut self) -> &mut Config {
        &mut self.config
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

/// A stream of requests requests coming over a channel.
#[pin_project]
#[derive(Debug)]
pub struct Requests<C>
where
    C: Channel,
{
    #[pin]
    channel: C,
    /// Responses waiting to be written to the wire.
    #[pin]
    pending_responses: Fuse<async_channel::Receiver<Response<C::Resp>>>,
    /// Handed out to request handlers to fan in responses.
    #[pin]
    responses_tx: async_channel::Sender<Response<C::Resp>>,
}

impl<C> Requests<C>
where
    C: Channel,
{
    /// Returns the inner channel over which messages are sent and received.
    pub fn channel_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut C> {
        self.as_mut().project().channel
    }

    fn pump_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> PollIo<InFlightRequest<C::Req, C::Resp>> {
        match ready!(self.channel_pin_mut().poll_next(cx)?) {
            Some(request) => {
                let abort_registration = self.channel_pin_mut().start_request(request.id);
                Poll::Ready(Some(Ok(InFlightRequest {
                    request,
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
                // This branch likely won't happen, since the Requests is holding a Sender.
                Poll::Ready(None)
            }
        }
    }
}

/// A request produced by [Channel::requests].
#[derive(Debug)]
pub struct InFlightRequest<Req, Res> {
    request: Request<Req>,
    response_tx: async_channel::Sender<Response<Res>>,
    abort_registration: AbortRegistration,
}

impl<Req, Res> InFlightRequest<Req, Res> {
    /// Returns a reference to the request.
    pub fn get(&self) -> &Request<Req> {
        &self.request
    }

    /// Returns a [future](Future) that executes the request using the given [service
    /// function](Serve). The service function's output is automatically sent back to the
    /// [Channel] that yielded this request.
    ///
    /// The returned future will stop executing when the first of the following conditions is met:
    ///
    /// 1. The channel that yielded this request receives a [cancellation
    ///    message](ClientMessage::Cancel) for this request.
    /// 2. The request [deadline](crate::context::Context::deadline) is reached.
    /// 3. The service function completes.
    pub fn execute<'a, S>(self, serve: &'a mut S) -> impl Future<Output = ()> + 'a
    where
        S: Serve<Req, Resp = Res>,
        Req: 'a,
        Res: 'a,
    {
        let Self {
            abort_registration,
            request,
            response_tx,
        } = self;
        Abortable::new(
            async move {
                let Request {
                    mut context,
                    message,
                    id: request_id,
                } = request;
                let trace_id = *context.trace_id();
                let deadline = context.deadline;
                let timeout = deadline.time_until();
                trace!(
                    "[{}] Handling request with deadline {} (timeout {:?}).",
                    trace_id,
                    format_rfc3339(deadline),
                    timeout,
                );
                let result =
                    tokio::time::timeout(timeout, serve.serve(&mut context, message)).await;
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

impl<C> Stream for Requests<C>
where
    C: Channel,
{
    type Item = io::Result<InFlightRequest<C::Req, C::Resp>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let read = self.as_mut().pump_read(cx)?;
            let read_closed = matches!(read, Poll::Ready(None));
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

#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
impl<C> Requests<C>
where
    C: Channel,
    C::Req: Send + 'static,
    C::Resp: Send + 'static,
{
    /// Executes all requests using the given service function. Requests are handled concurrently
    /// by [spawning](tokio::spawn) each handler on tokio's default executor.
    pub fn execute<S>(self, serve: S) -> TokioChannelExecutor<Self, S>
    where
        S: Serve<C::Req, Resp = C::Resp> + Send + Sync + 'static,
    {
        TokioChannelExecutor { inner: self, serve }
    }
}

/// A future that drives the server by [spawning](tokio::spawn) each [request](InFlightRequest)
/// on tokio's default executor.
#[pin_project]
#[derive(Debug)]
#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
pub struct TokioChannelExecutor<T, S> {
    #[pin]
    inner: T,
    serve: S,
}

#[cfg(feature = "tokio1")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio1")))]
impl<T, S> TokioChannelExecutor<T, S> {
    fn inner_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut T> {
        self.as_mut().project().inner
    }
}

#[cfg(feature = "tokio1")]
impl<C, S> Future for TokioChannelExecutor<Requests<C>, S>
where
    C: Channel + 'static,
    C::Req: Send + 'static,
    C::Resp: Send + 'static,
    S: Serve<C::Req, Resp = C::Resp> + Send + Sync + 'static + Clone,
    for<'a> S::Fut<'a>: send::Send<'a>,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while let Some(response_handler) = ready!(self.inner_pin_mut().poll_next(cx)) {
            match response_handler {
                Ok(resp) => {
                    let mut server = self.serve.clone();
                    tokio::spawn(async move {
                        resp.execute(&mut server).await;
                    });
                }
                Err(e) => {
                    log::info!("Requests errored out: {}", e);
                    break;
                }
            }
        }
        Poll::Ready(())
    }
}
