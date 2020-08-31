// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! *Disclaimer*: This is not an official Google product.
//!
//! tarpc is an RPC framework for rust with a focus on ease of use. Defining a
//! service can be done in just a few lines of code, and most of the boilerplate of
//! writing a server is taken care of for you.
//!
//! [Documentation](https://docs.rs/crate/tarpc/)
//!
//! ## What is an RPC framework?
//! "RPC" stands for "Remote Procedure Call," a function call where the work of
//! producing the return value is being done somewhere else. When an rpc function is
//! invoked, behind the scenes the function contacts some other process somewhere
//! and asks them to evaluate the function instead. The original function then
//! returns the value produced by the other process.
//!
//! RPC frameworks are a fundamental building block of most microservices-oriented
//! architectures. Two well-known ones are [gRPC](http://www.grpc.io) and
//! [Cap'n Proto](https://capnproto.org/).
//!
//! tarpc differentiates itself from other RPC frameworks by defining the schema in code,
//! rather than in a separate language such as .proto. This means there's no separate compilation
//! process, and no context switching between different languages.
//!
//! Some other features of tarpc:
//! - Pluggable transport: any type impling `Stream<Item = Request> + Sink<Response>` can be
//!   used as a transport to connect the client and server.
//! - `Send` optional: if the transport doesn't require it, neither does tarpc!
//! - Cascading cancellation: dropping a request will send a cancellation message to the server.
//!   The server will cease any unfinished work on the request, subsequently cancelling any of its
//!   own requests, repeating for the entire chain of transitive dependencies.
//! - Configurable deadlines and deadline propagation: request deadlines default to 10s if
//!   unspecified. The server will automatically cease work when the deadline has passed. Any
//!   requests sent by the server that use the request context will propagate the request deadline.
//!   For example, if a server is handling a request with a 10s deadline, does 2s of work, then
//!   sends a request to another server, that server will see an 8s deadline.
//! - Serde serialization: enabling the `serde1` Cargo feature will make service requests and
//!   responses `Serialize + Deserialize`. It's entirely optional, though: in-memory transports can
//!   be used, as well, so the price of serialization doesn't have to be paid when it's not needed.
//!
//! ## Usage
//! Add to your `Cargo.toml` dependencies:
//!
//! ```toml
//! tarpc = { version = "0.22.0", features = ["full"] }
//! ```
//!
//! The `tarpc::service` attribute expands to a collection of items that form an rpc service.
//! These generated types make it easy and ergonomic to write servers with less boilerplate.
//! Simply implement the generated service trait, and you're off to the races!
//!
//! ## Example
//!
//! For this example, in addition to tarpc, also add two other dependencies to
//! your `Cargo.toml`:
//!
//! ```toml
//! futures = "0.3"
//! tokio = "0.2"
//! ```
//!
//! In the following example, we use an in-process channel for communication between
//! client and server. In real code, you will likely communicate over the network.
//! For a more real-world example, see [example-service](example-service).
//!
//! First, let's set up the dependencies and service definition.
//!
//! ```rust
//! # #![allow(incomplete_features)]
//! # #![feature(generic_associated_types, type_alias_impl_trait)]
//! # extern crate futures;
//!
//! use futures::{
//!     future::{self, Ready},
//!     prelude::*,
//! };
//! use tarpc::{
//!     client, context,
//!     server::{self, Incoming},
//! };
//! use std::io;
//!
//! // This is the service definition. It looks a lot like a trait definition.
//! // It defines one RPC, hello, which takes one arg, name, and returns a String.
//! #[tarpc::service]
//! trait World {
//!     /// Returns a greeting for name.
//!     async fn hello(name: String) -> String;
//! }
//! ```
//!
//! This service definition generates a trait called `World`. Next we need to
//! implement it for our Server struct.
//!
//! ```rust
//! # #![allow(incomplete_features)]
//! # #![feature(generic_associated_types, type_alias_impl_trait)]
//! # extern crate futures;
//! # use futures::{
//! #     future::{self, Ready},
//! #     prelude::*,
//! # };
//! # use tarpc::{
//! #     client, context,
//! #     server::{self, Incoming},
//! # };
//! # use std::io;
//! # // This is the service definition. It looks a lot like a trait definition.
//! # // It defines one RPC, hello, which takes one arg, name, and returns a String.
//! # #[tarpc::service]
//! # trait World {
//! #     /// Returns a greeting for name.
//! #     async fn hello(name: String) -> String;
//! # }
//! // This is the type that implements the generated World trait. It is the business logic
//! // and is used to start the server.
//! #[derive(Clone)]
//! struct HelloServer;
//!
//! #[tarpc::server]
//! impl World for HelloServer {
//!     async fn hello(&mut self, _: &mut context::Context, name: String) -> String {
//!         format!("Hello, {}!", name)
//!     }
//! }
//! ```
//!
//! Lastly let's write our `main` that will start the server. While this example uses an
//! [in-process channel](rpc::transport::channel), tarpc also ships a generic [`serde_transport`]
//! behind the `serde-transport` feature, with additional [TCP](serde_transport::tcp) functionality
//! available behind the `tcp` feature.
//!
//! ```rust
//! # #![allow(incomplete_features)]
//! # #![feature(generic_associated_types, type_alias_impl_trait)]
//! # extern crate futures;
//! # use futures::{
//! #     future::{self, Ready},
//! #     prelude::*,
//! # };
//! # use tarpc::{
//! #     client, context,
//! #     server::{self, Channel},
//! # };
//! # use std::io;
//! # // This is the service definition. It looks a lot like a trait definition.
//! # // It defines one RPC, hello, which takes one arg, name, and returns a String.
//! # #[tarpc::service]
//! # trait World {
//! #     /// Returns a greeting for name.
//! #     async fn hello(name: String) -> String;
//! # }
//! # // This is the type that implements the generated World trait. It is the business logic
//! # // and is used to start the server.
//! # #[derive(Clone)]
//! # struct HelloServer;
//! # #[tarpc::server]
//! # impl World for HelloServer {
//! #     async fn hello(&mut self, _: &mut context::Context, name: String) -> String {
//! #         format!("Hello, {}!", name)
//! #     }
//! # }
//! # #[cfg(not(feature = "tokio1"))]
//! # fn main() {}
//! # #[cfg(feature = "tokio1")]
//! #[tokio::main]
//! async fn main() -> io::Result<()> {
//!     let (client_transport, server_transport) = tarpc::transport::channel::unbounded();
//!
//!     let server = server::channel::new(server_transport);
//!     tokio::spawn(server.execute(HelloServer.serve()));
//!
//!     // WorldClient is generated by the tarpc::service attribute. It has a constructor `new` that
//!     // takes a config and any Transport as input.
//!     let client = WorldClient::new(client::Config::default(), client_transport).spawn()?;
//!
//!     // The client has an RPC method for each RPC defined in the annotated trait. It takes the same
//!     // args as defined, with the addition of a Context, which is always the first arg. The Context
//!     // specifies a deadline and trace information which can be helpful in debugging requests.
//!     let hello = client.hello(context::current(), "Stim".to_string()).await?;
//!
//!     eprintln!("{}", hello);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Service Documentation
//!
//! Use `cargo doc` as you normally would to see the documentation created for all
//! items expanded by a `service!` invocation.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(clippy::type_complexity, incomplete_features)]
#![feature(generic_associated_types, type_alias_impl_trait, or_patterns)]

#[cfg(feature = "serde-transport")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde-transport")))]
pub mod serde_transport;
pub mod trace;

pub mod client;
pub mod context;
pub mod server;
pub mod transport;
pub(crate) mod util;

pub use {client::Client, transport::sealed::Transport};

use anyhow::Context as _;
use std::{fmt::Display, io, task::Poll, time::SystemTime};

/// A message from a client to a server.
#[derive(Debug)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ClientMessage<T> {
    /// A request initiated by a user. The server responds to a request by invoking a
    /// service-provided request handler.  The handler completes with a [`response`](Response), which
    /// the server sends back to the client.
    Request(Request<T>),
    /// A command to cancel an in-flight request, automatically sent by the client when a response
    /// future is dropped.
    ///
    /// When received, the server will immediately cancel the main task (top-level future) of the
    /// request handler for the associated request. Any tasks spawned by the request handler will
    /// not be canceled, because the framework layer does not
    /// know about them.
    Cancel {
        /// Trace context, deadline, and other cross-cutting concerns.
        #[cfg_attr(feature = "serde", serde(default))]
        context: context::Context,
        /// The ID of the request to cancel.
        request_id: u64,
    },
}

/// A request from a client to a server.
#[derive(Clone, Debug)]
#[non_exhaustive]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Request<T> {
    /// Trace context, deadline, and other cross-cutting concerns.
    pub context: context::Context,
    /// Uniquely identifies the request across all requests sent over a single channel.
    pub id: u64,
    /// The request body.
    pub message: T,
}

/// A response from a server to a client.
#[derive(Clone, Debug)]
#[non_exhaustive]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct Response<T> {
    /// Server-side Context. This will not be serialized. It is primarily useful for communicaiton
    /// between the application and transport layers.
    #[cfg_attr(feature = "serde1", serde(skip))]
    pub context: context::Context,
    /// The ID of the request being responded to.
    pub request_id: u64,
    /// The response body, or an error if the request failed.
    pub message: Result<T, ServerError>,
}

/// An error response from a server to a client.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub struct ServerError {
    #[cfg_attr(
        feature = "serde1",
        serde(serialize_with = "util::serde::serialize_io_error_kind_as_u32")
    )]
    #[cfg_attr(
        feature = "serde1",
        serde(deserialize_with = "util::serde::deserialize_io_error_kind_from_u32")
    )]
    /// The type of error that occurred to fail the request.
    pub kind: io::ErrorKind,
    /// A message describing more detail about the error that occurred.
    pub detail: Option<String>,
}

impl From<ServerError> for io::Error {
    fn from(e: ServerError) -> io::Error {
        io::Error::new(e.kind, e.detail.unwrap_or_default())
    }
}

impl<T> Request<T> {
    /// Returns the deadline for this request.
    pub fn deadline(&self) -> &SystemTime {
        &self.context.deadline
    }
}

pub(crate) type PollIo<T> = Poll<Option<io::Result<T>>>;
pub(crate) trait PollContext<T> {
    fn context<C>(self, context: C) -> Poll<Option<anyhow::Result<T>>>
    where
        C: Display + Send + Sync + 'static;

    fn with_context<C, F>(self, f: F) -> Poll<Option<anyhow::Result<T>>>
    where
        C: Display + Send + Sync + 'static,
        F: FnOnce() -> C;
}

impl<T> PollContext<T> for PollIo<T> {
    fn context<C>(self, context: C) -> Poll<Option<anyhow::Result<T>>>
    where
        C: Display + Send + Sync + 'static,
    {
        self.map(|o| o.map(|r| r.context(context)))
    }

    fn with_context<C, F>(self, f: F) -> Poll<Option<anyhow::Result<T>>>
    where
        C: Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map(|o| o.map(|r| r.with_context(f)))
    }
}

/// The main macro that creates RPC services.
///
/// Rpc methods are specified, mirroring trait syntax:
///
/// ```rust
/// # #![allow(incomplete_features)]
/// # #![feature(generic_associated_types, type_alias_impl_trait)]
/// #[tarpc::service]
/// trait Service {
///     /// Say hello
///     async fn hello(name: String) -> String;
/// }
/// ```
///
/// Attributes can be attached to each rpc. These attributes
/// will then be attached to the generated service traits'
/// corresponding `fn`s, as well as to the client stubs' RPCs.
///
/// The following items are expanded in the enclosing module:
///
/// * `trait Service` -- defines the RPC service.
///   * `fn serve` -- turns a service impl into a request handler.
/// * `Client` -- a client stub with a fn for each RPC.
///   * `fn new_stub` -- creates a new Client stub.
pub use tarpc_plugins::service;

/// A utility macro that can be used for RPC server implementations.
///
/// Syntactic sugar to make using async functions in the server implementation
/// easier. It does this by rewriting code like this, which would normally not
/// compile because async functions are disallowed in trait implementations:
///
/// ```rust
/// # #![allow(incomplete_features)]
/// # #![feature(generic_associated_types, type_alias_impl_trait)]
/// # use tarpc::context;
/// # use std::net::SocketAddr;
/// #[tarpc::service]
/// trait World {
///     async fn hello(name: String) -> String;
/// }
///
/// #[derive(Clone)]
/// struct HelloServer(SocketAddr);
///
/// #[tarpc::server]
/// impl World for HelloServer {
///     async fn hello(&mut self, _: &mut context::Context, name: String) -> String {
///         format!("Hello, {}! You are connected from {:?}.", name, self.0)
///     }
/// }
/// ```
///
/// Into code like this, which matches the service trait definition:
///
/// ```rust
/// # #![allow(incomplete_features)]
/// # #![feature(generic_associated_types, type_alias_impl_trait)]
/// # use tarpc::context;
/// # use std::pin::Pin;
/// # use futures::Future;
/// # use std::net::SocketAddr;
/// #[derive(Clone)]
/// struct HelloServer(SocketAddr);
///
/// #[tarpc::service]
/// trait World {
///     async fn hello(name: String) -> String;
/// }
///
/// impl World for HelloServer {
///     // Marker traits are implicitly leaked when applicable.
///     type HelloFut<'a> = impl Future<Output = String> + 'a;
///
///     fn hello<'a>(&'a mut self, _: &'a mut context::Context, name: String) -> Self::HelloFut<'a> {
///         async move {
///             format!("Hello, {}! You are connected from {:?}.", name, self.0)
///         }
///     }
/// }
/// ```
///
/// Note that this won't touch functions unless they have been annotated with
/// `async`, meaning that this should not break existing code.
pub use tarpc_plugins::server;
