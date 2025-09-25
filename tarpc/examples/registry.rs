// TODO
// [x] make generic over Async{Read,Write}
// [x] make generic over stream / sink
// [x] make helper functions for the client-side negotiation
// [x] make helper functions for the server-side negotiation
// [x] service discovery
// [ ] make spawn work

use bytes::{BufMut, Bytes, BytesMut};
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use std::io;
use std::net::{IpAddr, Ipv6Addr};
use std::pin::Pin;
use std::pin::pin;
use tarpc::{
    context::{self, Context},
    serde_transport,
    server::{BaseChannel, Channel as _, Serve},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_serde::formats::Json;
use tokio_util::codec::{Decoder, Framed, length_delimited::LengthDelimitedCodec};

#[derive(Clone, thiserror::Error, Debug, Serialize, Deserialize)]
#[error("service '{service_name:?}' not found")]
pub struct ServiceNotFound {
    pub service_name: BytesMut,
}

#[derive(thiserror::Error, Debug)]
pub enum HandshakeError<R> {
    #[error("failed to upgrade to RPC transport: {service_not_found}")]
    Upgrade {
        service_not_found: ServiceNotFound,
        runner: R,
    },
    #[error("could not encode the upgrade response: {0}")]
    EncodeResponse(#[from] bincode::error::EncodeError),
    #[error("an io error occurred")]
    Io(#[from] io::Error),
}

#[allow(async_fn_in_trait)]
pub trait Run<Transport> {
    async fn run<S: Serve + Clone + Send + 'static>(self, stream: Pin<&mut Transport>, serve: S)
    where
        S::Req: for<'a> Deserialize<'a> + Send + 'static,
        S::Resp: Serialize + Send + 'static;
}

#[allow(async_fn_in_trait)]
pub trait Handshake {
    async fn upgrade<S, R>(
        &mut self,
        service_name: BytesMut,
        stream: Pin<&mut S>,
        run_fn: R,
    ) -> Result<(), HandshakeError<R>>
    where
        S: TryStream<Ok = BytesMut> + Sink<Bytes, Error = io::Error>,
        R: Run<S>;
}

#[derive(Clone)]
pub struct MultiService<Service, Types> {
    service_name: String,
    service: Service,
    other_services: Types,
}

pub struct NilService;

pub trait ServiceList {
    type AppendRegistry<S>: ServiceList;
    fn append_service<S>(self, service_name: String, service: S) -> Self::AppendRegistry<S>;

    type ConcatRegistry<R>: ServiceList
    where
        R: ServiceList;
    fn concat<R>(self, registry: R) -> Self::ConcatRegistry<R>
    where
        R: ServiceList;

    fn for_each(&self, f: impl FnMut(&str));

    /// Register the given tuple of services.
    ///
    /// `services` supports tuples up to size 20.
    ///
    /// # Example
    ///
    /// ```rs
    /// use tokio_util::codec::length_delimited::LengthDelimitedCodec;
    /// use std::io::cursor::Cursor;
    /// # #[tarpc::service]
    /// # pub trait Hello {
    /// #     async fn hello(name: String) -> String;
    /// # }
    /// #
    /// # #[derive(Clone)]
    /// # struct HelloServer;
    /// #
    /// # impl Hello for HelloServer {
    /// #     async fn hello(self, _: Context, name: String) -> String {
    /// #         format!("Hello, {name}")
    /// #     }
    /// # }
    /// # let registry = ServiceRegistry::with_services((
    /// #     Service { name: "hello2", service: HelloServer.serve() },
    /// # ));
    ///
    /// let registry = registry.with_services((Service {
    ///     name: "Hello",
    ///     service: HelloServer.serve(),
    /// },));
    /// registry.upgrade("Hello".into(),
    ///                  Pin::new(&mut LengthDelimitedCodec::new().framed(Cursor::new(Vec::new()))),
    ///                  FramedJsonRunner).await.unwrap();
    /// ```
    fn with_services<S: ServiceTuple>(self, services: S) -> S::Registry<Self>
    where
        Self: Sized,
    {
        services.register(self)
    }
}

impl<Head, Tail> ServiceList for MultiService<Head, Tail>
where
    Tail: ServiceList,
{
    type AppendRegistry<S> = MultiService<Head, Tail::AppendRegistry<S>>;

    fn append_service<S>(self, service_name: String, service: S) -> Self::AppendRegistry<S> {
        MultiService {
            service_name: self.service_name,
            service: self.service,
            other_services: self.other_services.append_service(service_name, service),
        }
    }

    type ConcatRegistry<R>
        = MultiService<Head, Tail::ConcatRegistry<R>>
    where
        R: ServiceList;

    fn concat<R>(self, registry: R) -> Self::ConcatRegistry<R>
    where
        R: ServiceList,
    {
        MultiService {
            service_name: self.service_name,
            service: self.service,
            other_services: self.other_services.concat(registry),
        }
    }

    fn for_each(&self, mut f: impl FnMut(&str)) {
        f(&self.service_name);
        self.other_services.for_each(f);
    }
}

impl ServiceList for NilService {
    type AppendRegistry<S> = MultiService<S, Self>;

    fn append_service<S>(self, service_name: String, service: S) -> Self::AppendRegistry<S> {
        MultiService {
            service_name,
            service,
            other_services: Self,
        }
    }

    type ConcatRegistry<R>
        = R
    where
        R: ServiceList;

    fn concat<R>(self, registry: R) -> Self::ConcatRegistry<R>
    where
        R: ServiceList,
    {
        registry
    }

    fn for_each(&self, _: impl FnMut(&str)) {}
}

impl<S> MultiService<S, NilService> {
    pub fn new(service_name: &str, service: S) -> Self {
        Self {
            service_name: service_name.into(),
            service,
            other_services: NilService,
        }
    }

    pub fn push_back<S2>(
        self,
        service_name: &str,
        service: S2,
    ) -> MultiService<S, MultiService<S2, NilService>> {
        MultiService {
            service_name: self.service_name,
            service: self.service,
            other_services: MultiService {
                service_name: service_name.into(),
                service,
                other_services: NilService,
            },
        }
    }
}

/// A tuple of services to register with a registry.
pub trait ServiceTuple {
    /// A registry that concatenates an existing registry with the services in `self`.
    type Registry<Registry>: ServiceList
    where
        Registry: ServiceList;

    /// Registers the services in `self` with `registry`, returning a new registry containing all
    /// services.
    fn register<Registry>(self, registry: Registry) -> Self::Registry<Registry>
    where
        Registry: ServiceList;
}

#[tarpc::service]
pub trait ServiceDiscovery {
    async fn list_services() -> Vec<String>;
}

#[derive(Clone)]
pub struct ServiceRegistry;

#[derive(Clone)]
pub struct ServiceNames(Vec<String>);

impl ServiceNames {
    fn collect<R: ServiceList>(registry: &R) -> Self {
        let mut names = vec![];
        registry.for_each(|name| names.push(name.into()));
        Self(names)
    }
}

impl ServiceDiscovery for ServiceNames {
    async fn list_services(self, _: Context) -> Vec<String> {
        self.0
    }
}

impl ServiceRegistry {
    /// Register the given tuple of services.
    ///
    /// `services` supports tuples up to size 20.
    ///
    /// # Example
    ///
    /// ```rs
    /// use tokio_util::codec::length_delimited::LengthDelimitedCodec;
    /// use std::io::cursor::Cursor;
    /// # #[tarpc::service]
    /// # pub trait Hello {
    /// #     async fn hello(name: String) -> String;
    /// # }
    /// #
    /// # #[derive(Clone)]
    /// # struct HelloServer;
    /// #
    /// # impl Hello for HelloServer {
    /// #     async fn hello(self, _: Context, name: String) -> String {
    /// #         format!("Hello, {name}")
    /// #     }
    /// # }
    /// let registry = ServiceRegistry::with_services((
    ///     Service { name: "Hello", service: HelloServer.serve() },
    /// ));
    /// registry.upgrade("Hello".into(),
    ///                  Pin::new(&mut LengthDelimitedCodec::new().framed(Cursor::new(Vec::new()))),
    ///                  FramedJsonRunner).await.unwrap();
    /// ```
    pub fn with_services<S: ServiceTuple>(services: S) -> S::Registry<NilService> {
        services.register(NilService)
    }
}

pub struct Service<S> {
    name: String,
    service: S,
}

impl<S> Service<S> {
    pub fn new(name: &str, service: S) -> Self {
        Service {
            name: name.into(),
            service,
        }
    }
}

macro_rules! with_services_impl {
    ($s:ident : $S:ident) => {
        type Registry<R> = <R as ServiceList>::AppendRegistry<$S>
        where R: ServiceList;

        fn register<Registry>(self, registry: Registry) -> Self::Registry<Registry>
        where Registry: ServiceList
        {
            let ($s,) = self;
            registry.append_service($s.name.into(), $s.service)
        }
    };
    ($s0:ident : $S0:ident, $($s:ident : $S:ident),+) => {
        type Registry<R> = <($(Service<$S>,)+) as ServiceTuple>::Registry<<R as ServiceList>::AppendRegistry<$S0>>
        where R: ServiceList;

        fn register<Registry>(self, registry: Registry) -> Self::Registry<Registry>
        where Registry: ServiceList
        {
            let ($s0, $($s),+) = self;
            let registry = registry.append_service($s0.name.into(), $s0.service);
            ($($s,)+).register(registry)
        }
    };
}

macro_rules! with_services {
    ($($s:ident : $S:ident),+) => {
        impl<$($S),+> ServiceTuple for ($(Service<$S>,)+) {
            with_services_impl!($($s:$S),+);
        }
    };
}

with_services!(s0: S0);
with_services!(s0: S0, s1: S1);
with_services!(s0: S0, s1: S1, s2: S2);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15, s16: S16);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15, s16: S16, s17: S17);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15, s16: S16, s17: S17, s18: S18);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15, s16: S16, s17: S17, s18: S18, s19: S19);
with_services!(s0: S0, s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6, s7: S7, s8: S8, s9: S9, s10:
    S10, s11: S11, s12: S12, s13: S13, s14: S14, s15: S15, s16: S16, s17: S17, s18: S18, s19: S19,
    s20: S20);

fn serialize<T>(t: &T) -> Result<Bytes, bincode::error::EncodeError>
where
    T: Serialize,
{
    let mut bytes = BytesMut::new().writer();
    bincode::serde::encode_into_std_write(t, &mut bytes, bincode::config::standard())?;
    Ok(bytes.into_inner().freeze())
}

async fn send_response<S, R>(
    sink: Pin<&mut S>,
    r: Result<(), ServiceNotFound>,
) -> Result<(), HandshakeError<R>>
where
    S: TryStream<Ok = BytesMut> + Sink<Bytes, Error = io::Error>,
{
    let mut sink = Box::pin(sink);
    let resp = serialize(&r)?;
    sink.send(resp).await?;
    Ok(())
}

impl Handshake for NilService {
    async fn upgrade<S, R>(
        &mut self,
        service_name: BytesMut,
        stream: Pin<&mut S>,
        runner: R,
    ) -> Result<(), HandshakeError<R>>
    where
        S: TryStream<Ok = BytesMut> + Sink<Bytes, Error = io::Error>,
        R: Run<S>,
    {
        let service_not_found = ServiceNotFound { service_name };
        send_response(stream, Err(service_not_found.clone())).await?;
        Err(HandshakeError::Upgrade {
            service_not_found,
            runner,
        })
    }
}

impl<Service: Serve + Clone + Send + 'static, Types: Handshake> Handshake
    for MultiService<Service, Types>
where
    Service::Req: for<'a> Deserialize<'a> + Send,
    Service::Resp: Serialize + Send,
{
    async fn upgrade<S, R>(
        &mut self,
        service_name: BytesMut,
        mut stream: Pin<&mut S>,
        run_fn: R,
    ) -> Result<(), HandshakeError<R>>
    where
        S: TryStream<Ok = BytesMut> + Sink<Bytes, Error = io::Error>,
        R: Run<S>,
    {
        if service_name == self.service_name {
            let ok: Result<(), ServiceNotFound> = Ok(());
            send_response(stream.as_mut(), ok).await?;
            run_fn.run(stream, self.service.clone()).await;
            Ok(())
        } else {
            self.other_services
                .upgrade(service_name, stream, run_fn)
                .await
        }
    }
}

impl<Service: Serve + Clone + Send + 'static, Types> MultiService<Service, Types>
where
    Service::Req: for<'a> Deserialize<'a> + Send,
    Service::Resp: Serialize + Send,
    Self: ServiceList<AppendRegistry<ServeServiceDiscovery<ServiceNames>>: Handshake>,
{
    pub async fn negotiate<S: AsyncRead + AsyncWrite, R>(
        self,
        discovery: &str,
        stream: S,
        mut runner: R,
    ) -> Result<(), HandshakeError<R>>
    where
        R: Run<Framed<S, LengthDelimitedCodec>>,
    {
        let names = ServiceNames::collect(&self);
        let mut with_discovery = self.append_service(discovery.into(), names.serve());
        let mut codec = pin!(LengthDelimitedCodec::new().framed(stream));
        loop {
            let service_name = match codec.next().await {
                Some(Ok(bytes)) => bytes,
                Some(Err(e)) => break Err(HandshakeError::Io(e)),
                None => {
                    break Err(HandshakeError::Io(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "",
                    )));
                }
            };
            match with_discovery
                .upgrade(service_name, codec.as_mut(), runner)
                .await
            {
                Err(HandshakeError::Upgrade {
                    service_not_found: ServiceNotFound { service_name },
                    runner: r,
                }) => {
                    println!("Client tried connecting to {service_name:?}; nacking and waiting...");
                    runner = r;
                }
                Err(e) => break Err(e),
                Ok(()) => {
                    println!("Finished serving on transport.");
                    break Ok(());
                }
            }
        }
    }
}

/// Runs services using json serialization on a framed transport.
pub struct FramedJsonRunner;

impl<S> Run<Framed<S, LengthDelimitedCodec>> for FramedJsonRunner
where
    S: AsyncRead + AsyncWrite,
{
    async fn run<Serv: Serve + Clone + Send + 'static>(
        self,
        stream: Pin<&mut Framed<S, LengthDelimitedCodec>>,
        serve: Serv,
    ) where
        Serv::Req: for<'a> Deserialize<'a> + Send + 'static,
        Serv::Resp: Serialize + Send + 'static,
    {
        let transport = serde_transport::new(stream, Json::default());
        let mut requests = Box::pin(BaseChannel::with_defaults(transport).requests());
        while let Some(Ok(request)) = requests.next().await {
            request.execute(serve.clone()).await;
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NegotiateError {
    #[error("the server disconnected unexpectedly")]
    Disconnect,
    #[error("failed to decode the server response: {0}")]
    DecodeResponse(#[from] bincode::error::DecodeError),
    #[error("an I/O error occurred: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    ServiceNotFound(#[from] ServiceNotFound),
}

pub async fn negotiate<S: AsyncRead + AsyncWrite>(
    stream: S,
    service_name: String,
) -> Result<(), NegotiateError> {
    let mut codec = pin!(LengthDelimitedCodec::new().framed(stream));
    codec.send(service_name.into()).await?;
    match codec.next().await {
        Some(resp) => {
            let (response, _): (Result<(), ServiceNotFound>, _) =
                bincode::serde::decode_from_slice(&resp?, bincode::config::standard())?;
            Ok(response?)
        }
        None => Err(NegotiateError::Io(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "",
        ))),
    }
}

// ------------------------------- Test Services --------------------------------------------------

#[tarpc::service]
pub trait Hello {
    async fn hello(name: String) -> String;
}

#[tarpc::service]
pub trait Goodbye {
    async fn goodbye(name: String) -> String;
}

#[tarpc::service]
pub trait Add {
    async fn add(x: i32, y: i32) -> i32;
}

#[tarpc::service]
pub trait Subtract {
    async fn subtract(x: i32, y: i32) -> i32;
}

#[derive(Clone)]
struct HelloServer;

impl Hello for HelloServer {
    async fn hello(self, _: Context, name: String) -> String {
        format!("Hello, {name}")
    }
}

#[derive(Clone)]
struct GoodbyeServer;

impl Goodbye for GoodbyeServer {
    async fn goodbye(self, _: Context, name: String) -> String {
        format!("Goodbye, {name}")
    }
}

#[derive(Clone)]
struct AddServer;

impl Add for AddServer {
    async fn add(self, _: Context, x: i32, y: i32) -> i32 {
        x + y
    }
}

#[derive(Clone)]
struct SubtractServer;

impl Subtract for SubtractServer {
    async fn subtract(self, _: Context, x: i32, y: i32) -> i32 {
        x - y
    }
}

#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind((IpAddr::V6(Ipv6Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let server_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let registry = ServiceRegistry::with_services((
                Service::new("Hello", HelloServer.serve()),
                Service::new("Goodbye", GoodbyeServer.serve()),
            ));
            let registry = registry.with_services((
                Service::new("Add", AddServer.serve()),
                Service::new("Subtract", SubtractServer.serve()),
            ));
            tokio::spawn(async move {
                if let Err(e) = registry
                    .negotiate("discovery", stream, FramedJsonRunner)
                    .await
                {
                    eprintln!("{}", e);
                }
            });
        }
    });
    let mut discovery = TcpStream::connect(server_addr).await.unwrap();
    negotiate(&mut discovery, "discovery".into()).await.unwrap();
    let discovery = serde_transport::Transport::from((discovery, Json::default()));
    let discovery = ServiceDiscoveryClient::new(Default::default(), discovery).spawn();
    let services = discovery.list_services(context::current()).await.unwrap();
    println!("Registered services: {services:?}");

    let mut stream = TcpStream::connect(server_addr).await.unwrap();
    let negotiation = negotiate(&mut stream, "Goodness me".into()).await;
    println!("{}", negotiation.expect_err("Expected upgrade error"));

    negotiate(&mut stream, "Hello".into()).await.unwrap();
    let transport = serde_transport::Transport::from((stream, Json::default()));
    let hello_client = HelloClient::new(Default::default(), transport).spawn();

    let hello = hello_client
        .hello(context::current(), "Tim".into())
        .await
        .unwrap();
    println!("{hello}");

    let hello = hello_client
        .hello(context::current(), "Steph".into())
        .await
        .unwrap();
    println!("{hello}");
}
