// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Provides a request context that carries a deadline and trace context. This context is sent from
//! client to server and is used by the server to enforce response deadlines.

use crate::{
    rpc::context::extensions::Extensions,
    trace::{self, TraceId},
    ClientMessage, Request, Response,
};
use static_assertions::assert_impl_all;
use std::time::{Duration, SystemTime};

pub mod extensions;

/// A request context that carries request-scoped information like deadlines and trace information.
/// It is sent from client to server and is used by the server to enforce response deadlines.
///
/// The context should not be stored directly in a server implementation, because the context will
/// be different for each request in scope.
#[derive(Clone, Debug)]
#[non_exhaustive]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Context {
    /// When the client expects the request to be complete by. The server should cancel the request
    /// if it is not complete by this time.
    #[cfg_attr(
        feature = "serde1",
        serde(serialize_with = "crate::util::serde::serialize_epoch_secs")
    )]
    #[cfg_attr(
        feature = "serde1",
        serde(deserialize_with = "crate::util::serde::deserialize_epoch_secs")
    )]
    #[cfg_attr(feature = "serde1", serde(default = "ten_seconds_from_now"))]
    pub deadline: SystemTime,
    /// Uniquely identifies requests originating from the same source.
    /// When a service handles a request by making requests itself, those requests should
    /// include the same `trace_id` as that included on the original request. This way,
    /// users can trace related actions across a distributed system.
    pub trace_context: trace::Context,
    /// Context extensions. These extensions are not intended to be sent over the transport.
    /// It is intended as a method to pass data between the application and transport layers.
    /// For example, a transport could optionally enable compression if a compression extension is
    /// present in the extensions.
    #[cfg_attr(feature = "serde1", serde(skip))]
    pub extensions: Extensions,
}

impl Default for Context {
    fn default() -> Self {
        Context {
            deadline: SystemTime::UNIX_EPOCH,
            trace_context: Default::default(),
            extensions: Default::default(),
        }
    }
}

assert_impl_all!(Context: Send, Sync);

/// Types that may contain a Context (i.e. ClientMessage and Response).
pub trait HasContext {
    /// Returns a reference to the contained Context.
    fn context(&self) -> &Context;
    /// Returns a mutable reference to the contained Context.
    fn context_mut(&mut self) -> &mut Context;
}

impl<T> HasContext for ClientMessage<T> {
    fn context(&self) -> &Context {
        match *self {
            ClientMessage::Request(Request { ref context, .. }) => context,
            ClientMessage::Cancel { ref context, .. } => context,
        }
    }

    fn context_mut(&mut self) -> &mut Context {
        match *self {
            ClientMessage::Request(Request {
                ref mut context, ..
            }) => context,
            ClientMessage::Cancel {
                ref mut context, ..
            } => context,
        }
    }
}

impl<T> HasContext for Response<T> {
    fn context(&self) -> &Context {
        &self.context
    }

    fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }
}

#[cfg(feature = "serde1")]
fn ten_seconds_from_now() -> SystemTime {
    SystemTime::now() + Duration::from_secs(10)
}

/// Returns the context for the current request, or a default Context if no request is active.
// TODO: populate Context with request-scoped data, with default fallbacks.
pub fn current() -> Context {
    Context {
        deadline: SystemTime::now() + Duration::from_secs(10),
        trace_context: trace::Context::new_root(),
        extensions: Default::default(),
    }
}

impl Context {
    /// Returns the ID of the request-scoped trace.
    pub fn trace_id(&self) -> &TraceId {
        &self.trace_context.trace_id
    }
}
