// Copyright 2020 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

use crate::server::{Channel, Config};
use crate::{context, Request, Response};
use fnv::FnvHashSet;
use futures::{
    future::{AbortHandle, AbortRegistration},
    task::noop_waker_ref,
    Sink, Stream,
};
use pin_project::pin_project;
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::SystemTime,
};

#[pin_project]
pub(crate) struct FakeChannel<In, Out> {
    #[pin]
    pub stream: VecDeque<In>,
    #[pin]
    pub sink: VecDeque<Out>,
    pub config: Config,
    pub in_flight_requests: FnvHashSet<u64>,
}

impl<In, Out> FakeChannel<In, Out> {
    fn stream_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut VecDeque<In>> {
        self.as_mut().project().stream
    }

    fn sink_pin_mut<'a>(self: &'a mut Pin<&mut Self>) -> Pin<&'a mut VecDeque<Out>> {
        self.as_mut().project().sink
    }

    fn in_flight_requests<'a>(self: &'a mut Pin<&mut Self>) -> &'a mut FnvHashSet<u64> {
        self.as_mut().project().in_flight_requests
    }
}

impl<In, Out> Stream for FakeChannel<In, Out>
where
    In: Unpin,
{
    type Item = In;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.stream_pin_mut().pop_front())
    }
}

impl<In, Resp> Sink<Response<Resp>> for FakeChannel<In, Response<Resp>> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.sink_pin_mut().poll_ready(cx).map_err(|e| match e {})
    }

    fn start_send(mut self: Pin<&mut Self>, response: Response<Resp>) -> Result<(), Self::Error> {
        self.in_flight_requests().remove(&response.request_id);
        self.sink_pin_mut()
            .start_send(response)
            .map_err(|e| match e {})
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.sink_pin_mut().poll_flush(cx).map_err(|e| match e {})
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.sink_pin_mut().poll_close(cx).map_err(|e| match e {})
    }
}

impl<Req, Resp> Channel for FakeChannel<io::Result<Request<Req>>, Response<Resp>>
where
    Req: Unpin,
{
    type Req = Req;
    type Resp = Resp;

    fn config(&self) -> &Config {
        &self.config
    }

    fn in_flight_requests(&self) -> usize {
        self.in_flight_requests.len()
    }

    fn start_request(mut self: Pin<&mut Self>, id: u64) -> AbortRegistration {
        self.in_flight_requests().insert(id);
        AbortHandle::new_pair().1
    }
}

impl<Req, Resp> FakeChannel<io::Result<Request<Req>>, Response<Resp>> {
    pub fn push_req(&mut self, id: u64, message: Req) {
        self.stream.push_back(Ok(Request {
            context: context::Context {
                deadline: SystemTime::UNIX_EPOCH,
                trace_context: Default::default(),
                extensions: Default::default(),
            },
            id,
            message,
        }));
    }
}

impl FakeChannel<(), ()> {
    pub fn default<Req, Resp>() -> FakeChannel<io::Result<Request<Req>>, Response<Resp>> {
        FakeChannel {
            stream: Default::default(),
            sink: Default::default(),
            config: Default::default(),
            in_flight_requests: Default::default(),
        }
    }
}

pub trait PollExt {
    fn is_done(&self) -> bool;
}

impl<T> PollExt for Poll<Option<T>> {
    fn is_done(&self) -> bool {
        match self {
            Poll::Ready(None) => true,
            _ => false,
        }
    }
}

pub fn cx() -> Context<'static> {
    Context::from_waker(&noop_waker_ref())
}
