// Copyright 2016 Google Inc. All Rights Reserved.
//
// Licensed under the MIT License, <LICENSE or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed except according to those terms.

use futures::{Future, Poll, Async};
use futures::stream::Stream;
use tokio_service::Service;
use std::fmt;
use std::sync::Arc;
use std::ops::Deref;
use std::error::Error;
use std::net::{SocketAddr, ToSocketAddrs};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A bottom type that impls `Error`, `Serialize`, and `Deserialize`. It is impossible to
/// instantiate this type.
#[derive(Debug)]
pub struct Never(!);

impl Error for Never {
    fn description(&self) -> &str {
        match self.0 {
            // TODO(tikue): remove when https://github.com/rust-lang/rust/issues/12609 lands
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            // TODO(tikue): remove when https://github.com/rust-lang/rust/issues/12609 lands
            _ => unreachable!(),
        }
    }
}

impl Future for Never {
    type Item = Never;
    type Error = Never;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.0 {
            // TODO(tikue): remove when https://github.com/rust-lang/rust/issues/12609 lands
            _ => unreachable!(),
        }
    }
}

impl Stream for Never {
    type Item = Never;
    type Error = Never;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.0 {
            // TODO(tikue): remove when https://github.com/rust-lang/rust/issues/12609 lands
            _ => unreachable!(),
        }
    }
}

impl Serialize for Never {
    fn serialize<S>(&self, _: &mut S) -> Result<(), S::Error>
        where S: Serializer
    {
        match self.0 {
            // TODO(tikue): remove when https://github.com/rust-lang/rust/issues/12609 lands
            _ => unreachable!(),
        }
    }
}

// Please don't try to deserialize this. :(
impl Deserialize for Never {
    fn deserialize<D>(_: &mut D) -> Result<Self, D::Error> 
        where D: Deserializer
    {
        panic!("Never cannot be instantiated!");
    }
}

/// A `String` that impls `std::error::Error`. Useful for quick-and-dirty error propagation.
#[derive(Debug, Serialize, Deserialize)]
pub struct Message(pub String);

impl Error for Message {
    fn description(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<S: Into<String>> From<S> for Message {
    fn from(s: S) -> Self {
        Message(s.into())
    }
}


/// Provides a utility method for more ergonomically parsing a `SocketAddr` when panicking is
/// acceptable.
pub trait FirstSocketAddr: ToSocketAddrs {
    /// Returns the first resolved `SocketAddr` or panics otherwise.
    fn first_socket_addr(&self) -> SocketAddr {
        self.to_socket_addrs().unwrap().next().unwrap()
    }
}

impl<A: ToSocketAddrs> FirstSocketAddr for A {}

/// A reference counted wrapper to a `tokio_proto::Service`.
///
/// This wrapper is used to enabled sharing of a service between multiple concurrent workers.
pub struct ArcService<T>(Arc<T>);

impl<T> ArcService<T> {
    /// Create a new reference counter `Service`.
    pub fn new(t: T) -> Self {
        ArcService(Arc::new(t))
    }
}

impl<T: Service> Service for ArcService<T> {
    type Request = T::Request;
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;
    fn call(&self, req: Self::Request) -> Self::Future {
        self.0.call(req)
    }
    fn poll_ready(&self) -> Async<()> {
        self.0.poll_ready()
    }
}

impl<T> Clone for ArcService<T> {
    fn clone(&self) -> Self {
        ArcService(self.0.clone())
    }
}

impl<T> Deref for ArcService<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.0
    }
}
