// Copyright 2016 Google Inc. All Rights Reserved.
//
// Licensed under the MIT License, <LICENSE or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed except according to those terms.

use WireError;
use bincode::serde::DeserializeError;
use futures::{self, Future};
use protocol::Proto;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use tokio_core::net::TcpStream;
use tokio_proto::BindClient as ProtoBindClient;
use tokio_proto::multiplex::Multiplex;
use tokio_service::Service;

type WireResponse<Resp, E> = Result<Result<Resp, WireError<E>>, DeserializeError>;
type ResponseFuture<Req, Resp, E> = futures::Map<<BindClient<Req, Resp, E> as Service>::Future,
                                        fn(WireResponse<Resp, E>) -> Result<Resp, ::Error<E>>>;

cfg_if! {
    if #[cfg(feature = "tls")] {
        use tokio_tls::TlsStream;
        use native_tls::TlsConnector;

        type BindClient<Req, Resp, E> =
            <Proto<Req, Result<Resp, WireError<E>>> as
                ProtoBindClient<Multiplex, TlsStream<TcpStream>>>::BindClient;

            /// TLS context
            pub struct TlsClientContext {
                /// Domain to connect to
                pub domain: String,
                /// TLS connector
                pub tls_connector: TlsConnector,
            }

            impl TlsClientContext {
                /// Try to make a new `TlsClientContext`, providing the domain the client will
                /// connect to.
                pub fn try_new<S: Into<String>>(domain: S)
                    -> Result<TlsClientContext, ::native_tls::Error> {
                    Ok(TlsClientContext {
                        domain: domain.into(),
                        tls_connector: TlsConnector::builder()?.build()?,
                    })
                }
            }
    } else {
        type BindClient<Req, Resp, E> =
            <Proto<Req, Result<Resp, WireError<E>>> as
                ProtoBindClient<Multiplex, TcpStream>>::BindClient;
    }
}

/// A client that impls `tokio_service::Service` that writes and reads bytes.
///
/// Typically, this would be combined with a serialization pre-processing step
/// and a deserialization post-processing step.
pub struct Client<Req, Resp, E>
    where Req: Serialize + 'static,
          Resp: Deserialize + 'static,
          E: Deserialize + 'static
{
    inner: BindClient<Req, Resp, E>,
}

impl<Req, Resp, E> Clone for Client<Req, Resp, E>
    where Req: Serialize + 'static,
          Resp: Deserialize + 'static,
          E: Deserialize + 'static
{
    fn clone(&self) -> Self {
        Client { inner: self.inner.clone() }
    }
}

impl<Req, Resp, E> Service for Client<Req, Resp, E>
    where Req: Serialize + Sync + Send + 'static,
          Resp: Deserialize + Sync + Send + 'static,
          E: Deserialize + Sync + Send + 'static
{
    type Request = Req;
    type Response = Result<Resp, ::Error<E>>;
    type Error = io::Error;
    type Future = ResponseFuture<Req, Resp, E>;

    fn call(&mut self, request: Self::Request) -> Self::Future {
        self.inner.call(request).map(Self::map_err)
    }
}

impl<Req, Resp, E> Client<Req, Resp, E>
    where Req: Serialize + 'static,
          Resp: Deserialize + 'static,
          E: Deserialize + 'static
{
    fn new(inner: BindClient<Req, Resp, E>) -> Self
        where Req: Serialize + Sync + Send + 'static,
              Resp: Deserialize + Sync + Send + 'static,
              E: Deserialize + Sync + Send + 'static
    {
        Client { inner: inner }
    }

    fn map_err(resp: WireResponse<Resp, E>) -> Result<Resp, ::Error<E>> {
        resp.map(|r| r.map_err(::Error::from))
            .map_err(::Error::ClientDeserialize)
            .and_then(|r| r)
    }
}

impl<Req, Resp, E> fmt::Debug for Client<Req, Resp, E>
    where Req: Serialize + 'static,
          Resp: Deserialize + 'static,
          E: Deserialize + 'static
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Client {{ .. }}")
    }
}

/// Exposes a trait for connecting asynchronously to servers.
pub mod future {
    use future::REMOTE;
    use futures::{self, Async, Future};
    use protocol::Proto;
    use serde::{Deserialize, Serialize};
    use std::io;
    use std::marker::PhantomData;
    use std::net::SocketAddr;
    use super::Client;
    use tokio_core::net::TcpStream;
    use tokio_core::reactor;
    use tokio_proto::BindClient;

    cfg_if! {
        if #[cfg(feature = "tls")] {
            use super::TlsClientContext;
            use tokio_core::net::TcpStreamNew;
            use tokio_tls::{ConnectAsync, TlsStream, TlsConnectorExt};
            use errors::native2io;

            /// Types that can connect to a server asynchronously.
            pub trait Connect<'a>: Sized {
                /// The type of the future returned when calling `connect`.
                type ConnectFut: Future<Item = Self, Error = io::Error> + 'static;

                /// The type of the future returned when calling `connect_with`.
                type ConnectWithFut: Future<Item = Self, Error = io::Error> + 'a;
                /// Connects to a server located at the given address, using a remote to the default
                /// reactor.
                fn connect(addr: &SocketAddr, tls_client_cx: TlsClientContext) -> Self::ConnectFut {
                    Self::connect_remotely(addr, &REMOTE, tls_client_cx)
                }

                /// Connects to a server located at the given address, using the given reactor
                /// remote.
                fn connect_remotely(addr: &SocketAddr,
                                    remote: &reactor::Remote,
                                    tls_client_cx: TlsClientContext)
                    -> Self::ConnectFut;

                /// Connects to a server located at the given address, using the given reactor
                /// handle.
                fn connect_with(addr: &SocketAddr,
                                handle: &'a reactor::Handle,
                                tls_client_cx: TlsClientContext)
                    -> Self::ConnectWithFut;
            }

            /// A future that resolves to a `Client` or an `io::Error`.
            pub struct ConnectWithFuture<'a, Req, Resp, E> {
                inner: futures::Map<
                    futures::AndThen<futures::Join<TcpStreamNew,
                    futures::future::FutureResult<TlsClientContext, io::Error>>,
                    futures::MapErr<ConnectAsync<TcpStream>,
                    fn(::native_tls::Error) -> io::Error>,
                    TlsConnectFn>,
                    MultiplexConnect<'a, Req, Resp, E>>,
            }

            struct TlsConnectFn;

            impl FnOnce<(((TcpStream, TlsClientContext),))> for TlsConnectFn {
                type Output = futures::MapErr<ConnectAsync<TcpStream>,
                    fn(::native_tls::Error) -> io::Error>;

                extern "rust-call" fn call_once(self,
                        ((tcp, tls_client_cx),): ((TcpStream, TlsClientContext),))
                                                 -> Self::Output {
                                                     tls_client_cx.tls_connector
                                                         .connect_async(&tls_client_cx.domain, tcp)
                                                         .map_err(native2io)
                                                 }
            }
        } else {
            /// Types that can connect to a server asynchronously.
            pub trait Connect<'a>: Sized {
                /// The type of the future returned when calling `connect`.
                type ConnectFut: Future<Item = Self, Error = io::Error> + 'static;

                /// The type of the future returned when calling `connect_with`.
                type ConnectWithFut: Future<Item = Self, Error = io::Error> + 'a;

                /// Connects to a server located at the given address, using a remote to the default
                /// reactor.
                fn connect(addr: &SocketAddr) -> Self::ConnectFut {
                    Self::connect_remotely(addr, &REMOTE)
                }

                /// Connects to a server located at the given address, using the given reactor
                /// remote.
                fn connect_remotely(addr: &SocketAddr, remote: &reactor::Remote)
                    -> Self::ConnectFut;

                /// Connects to a server located at the given address, using the given reactor
                /// handle.
                fn connect_with(addr: &SocketAddr, handle: &'a reactor::Handle)
                    -> Self::ConnectWithFut;
            }

            /// A future that resolves to a `Client` or an `io::Error`.
            pub struct ConnectWithFuture<'a, Req, Resp, E> {
                inner: futures::Map<::tokio_core::net::TcpStreamNew,
                    MultiplexConnect<'a, Req, Resp, E>>,
            }
        }
    }

    /// A future that resolves to a `Client` or an `io::Error`.
    pub struct ConnectFuture<Req, Resp, E>
        where Req: Serialize + 'static,
              Resp: Deserialize + 'static,
              E: Deserialize + 'static
    {
        inner: futures::Oneshot<io::Result<Client<Req, Resp, E>>>,
    }

    impl<Req, Resp, E> Future for ConnectFuture<Req, Resp, E>
        where Req: Serialize + 'static,
              Resp: Deserialize + 'static,
              E: Deserialize + 'static
    {
        type Item = Client<Req, Resp, E>;
        type Error = io::Error;

        fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
            // Ok to unwrap because we ensure the oneshot is always completed.
            match self.inner.poll().unwrap() {
                Async::Ready(Ok(client)) => Ok(Async::Ready(client)),
                Async::Ready(Err(err)) => Err(err),
                Async::NotReady => Ok(Async::NotReady),
            }
        }
    }

    impl<'a, Req, Resp, E> Future for ConnectWithFuture<'a, Req, Resp, E>
        where Req: Serialize + Sync + Send + 'static,
              Resp: Deserialize + Sync + Send + 'static,
              E: Deserialize + Sync + Send + 'static
    {
        type Item = Client<Req, Resp, E>;
        type Error = io::Error;

        fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
            self.inner.poll()
        }
    }

    struct MultiplexConnect<'a, Req, Resp, E>(&'a reactor::Handle, PhantomData<(Req, Resp, E)>);

    impl<'a, Req, Resp, E> MultiplexConnect<'a, Req, Resp, E> {
        fn new(handle: &'a reactor::Handle) -> Self {
            MultiplexConnect(handle, PhantomData)
        }
    }

    cfg_if! {
        if #[cfg(feature = "tls")] {
            impl<'a, Req, Resp, E> FnOnce<(TlsStream<TcpStream>,)>
                for MultiplexConnect<'a, Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                type Output = Client<Req, Resp, E>;

                extern "rust-call" fn call_once(self,
                                                (tcp,): (TlsStream<TcpStream>,))
                                                -> Client<Req, Resp, E> {
                    Client::new(Proto::new().bind_client(self.0, tcp))
                }
            }

            impl<'a, Req, Resp, E> Connect<'a> for Client<Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                type ConnectFut = ConnectFuture<Req, Resp, E>;
                type ConnectWithFut = ConnectWithFuture<'a, Req, Resp, E>;

                fn connect_remotely(addr: &SocketAddr, remote: &reactor::Remote,
                                    tls_client_cx: TlsClientContext) -> Self::ConnectFut {
                        let addr = *addr;
                        let (tx, rx) = futures::oneshot();
                        remote.spawn(move |handle| {
                            let handle2 = handle.clone();
                            TcpStream::connect(&addr, handle)
                                .and_then(move |socket| {
                                    tls_client_cx.tls_connector
                                        .connect_async(&tls_client_cx.domain, socket)
                                        .map_err(native2io)
                                })
                            .map(move |tcp| Client::new(Proto::new().bind_client(&handle2, tcp)))
                                .then(move |result| {
                                    tx.complete(result);
                                    Ok(())
                                })
                        });
                        ConnectFuture { inner: rx }
                }

                fn connect_with(addr: &SocketAddr, handle: &'a reactor::Handle,
                                tls_client_cx: TlsClientContext) -> Self::ConnectWithFut {
                        ConnectWithFuture {
                            inner: TcpStream::connect(addr, handle)
                                .join(futures::finished(tls_client_cx))
                                .and_then(TlsConnectFn)
                                .map(MultiplexConnect::new(handle)),
                        }
                }
            }
        } else {
            impl<'a, Req, Resp, E> FnOnce<(TcpStream,)> for MultiplexConnect<'a, Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                type Output = Client<Req, Resp, E>;

                extern "rust-call" fn call_once(self, (tcp,): (TcpStream,))
                                                               -> Client<Req, Resp, E> {
                    Client::new(Proto::new().bind_client(self.0, tcp))
                }
            }

            impl<'a, Req, Resp, E> Connect<'a> for Client<Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                type ConnectFut = ConnectFuture<Req, Resp, E>;
                type ConnectWithFut = ConnectWithFuture<'a, Req, Resp, E>;

                fn connect_remotely(addr: &SocketAddr, remote: &reactor::Remote)
                    -> Self::ConnectFut {
                    let addr = *addr;
                    let (tx, rx) = futures::oneshot();
                    remote.spawn(move |handle| {
                        let handle2 = handle.clone();
                        TcpStream::connect(&addr, handle)
                            .map(move |tcp| Client::new(Proto::new().bind_client(&handle2, tcp)))
                            .then(move |result| {
                                tx.complete(result);
                                Ok(())
                            })
                    });
                    ConnectFuture { inner: rx }
                }

                fn connect_with(addr: &SocketAddr, handle: &'a reactor::Handle)
                    -> Self::ConnectWithFut {
                    ConnectWithFuture {
                        inner: TcpStream::connect(addr, handle).map(MultiplexConnect::new(handle)),
                    }
                }
            }
        }
    }
}

/// Exposes a trait for connecting synchronously to servers.
pub mod sync {
    use futures::Future;
    use serde::{Deserialize, Serialize};
    use std::io;
    use std::net::ToSocketAddrs;
    use super::Client;

    cfg_if! {
        if #[cfg(feature = "tls")] {
            /// Types that can connect to a server synchronously.
            pub trait Connect: Sized {
                /// Connects to a server located at the given address.
                fn connect<A>(addr: A, tls_client_cx: ::TlsClientContext)
                    -> Result<Self, io::Error> where A: ToSocketAddrs;
            }

            impl<Req, Resp, E> Connect for Client<Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                fn connect<A>(addr: A, tls_client_cx: ::TlsClientContext) -> Result<Self, io::Error>
                    where A: ToSocketAddrs
                {
                    let addr = if let Some(a) = addr.to_socket_addrs()?.next() {
                        a
                    } else {
                        return Err(io::Error::new(io::ErrorKind::AddrNotAvailable,
                                                  "`ToSocketAddrs::to_socket_addrs` returned an \
                                                   empty iterator."));
                    };
                    <Self as super::future::Connect>::connect(&addr, tls_client_cx).wait()
                }
            }
        } else {
            /// Types that can connect to a server synchronously.
            pub trait Connect: Sized {
                /// Connects to a server located at the given address.
                fn connect<A>(addr: A) -> Result<Self, io::Error> where A: ToSocketAddrs;
            }

            impl<Req, Resp, E> Connect for Client<Req, Resp, E>
                where Req: Serialize + Sync + Send + 'static,
                      Resp: Deserialize + Sync + Send + 'static,
                      E: Deserialize + Sync + Send + 'static
            {
                fn connect<A>(addr: A) -> Result<Self, io::Error>
                    where A: ToSocketAddrs
                {
                    let addr = if let Some(a) = addr.to_socket_addrs()?.next() {
                        a
                    } else {
                        return Err(io::Error::new(io::ErrorKind::AddrNotAvailable,
                                                  "`ToSocketAddrs::to_socket_addrs` returned an \
                                                  empty iterator."));
                    };
                    <Self as super::future::Connect>::connect(&addr).wait()
                }
            }
        }
    }
}
