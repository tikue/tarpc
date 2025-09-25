use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::{
    future::Future,
    marker::PhantomData,
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};
use tarpc::{
    client::{self, stub::Stub, RpcError},
    context::{self, Context},
    serde_transport,
    server::{BaseChannel, Channel as _, Serve},
    ChannelError, ClientMessage, RequestName, ServerError, Transport,
};
use tokio_serde::formats::Json;

#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
pub enum DispatchError {
    #[error("service '{0}' not found")]
    ServiceNotFound(String),
    #[error("could not encode the response: {0}")]
    EncodeResponse(String),
    #[error("could not decode the request: {0}")]
    DecodeRequest(String),
}

#[tarpc::service]
pub trait MultiServe {
    async fn call(
        service_name: String,
        request: ByteBuf,
    ) -> Result<Result<ByteBuf, ServerError>, DispatchError>;
}

#[derive(Clone)]
pub struct MultiService<Service, Types> {
    service_name: String,
    service: Service,
    other_services: Types,
}

impl<S> MultiService<S, ()> {
    pub fn new(service_name: &str, service: S) -> Self {
        Self {
            service_name: service_name.into(),
            service,
            other_services: (),
        }
    }
}

pub trait Register {
    type Registry<Tail>;

    fn register<Tail>(self, tail: Tail) -> Self::Registry<Tail>;
}

impl<Head, Tail> MultiService<Head, Tail> {
    pub fn push_front<S>(self, service_name: &str, service: S) -> MultiService<S, Self> {
        MultiService {
            service_name: service_name.into(),
            service,
            other_services: self,
        }
    }

    pub fn with_services<S: Register>(self, services: S) -> S::Registry<Self> {
        services.register(self)
    }
}

pub struct ServiceRegistry;

impl ServiceRegistry {
    pub fn with_services<S: Register>(self, services: S) -> S::Registry<()> {
        services.register(())
    }
}

pub struct Service<'a, S> {
    name: &'a str,
    service: S,
}

macro_rules! prepend_to_registry_type {
    ($S:ty : $R:ty) => {
        MultiService<$S, $R>
    };
    ($S0:ty, $($S:ty),+ : $R:ty) => {
        MultiService<$S0, prepend_to_registry_type!($($S),+ : $R)>
    };
}

macro_rules! with_services_impl {
    ($s:ident : $S:ident) => {
        type Registry<Tail> = MultiService<$S, Tail>;
        fn register<Tail>(self, tail: Tail) -> Self::Registry<Tail> {
            let ($s,) = self;
            MultiService {
                service_name: $s.name.into(),
                service: $s.service,
                other_services: tail,
            }
        }
    };
    ($s0:ident : $S0:ident, $($s:ident : $S:ident),+) => {
        type Registry<Tail> = prepend_to_registry_type!($S0, $($S),+ : Tail);
        fn register<Tail>(self, tail: Tail) -> Self::Registry<Tail> {
            let ($s0, $($s),+) = self;
            ($($s,)+).register(tail).push_front($s0.name, $s0.service)
        }
    };
}

macro_rules! with_services {
    ($($s:ident : $S:ident),+) => {
        impl<$($S),+> Register for ($(Service<'_, $S>,)+) {
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

fn deserialize<T>(bytes: ByteBuf) -> Result<T, bincode::Error>
where
    T: for<'a> Deserialize<'a>,
{
    bincode::deserialize(&*bytes)
}

fn serialize<T>(t: T) -> Result<ByteBuf, bincode::Error>
where
    T: Serialize,
{
    let bytes = bincode::serialize(&t)?;
    Ok(ByteBuf::from(bytes))
}

impl MultiServe for () {
    async fn call(
        self,
        _ctx: Context,
        service_name: String,
        _request: ByteBuf,
    ) -> Result<Result<ByteBuf, ServerError>, DispatchError> {
        Err(DispatchError::ServiceNotFound(service_name))
    }
}

impl<Service: Serve + Clone, Types: MultiServe> MultiServe for MultiService<Service, Types>
where
    Service::Req: for<'a> Deserialize<'a>,
    Service::Resp: Serialize,
{
    async fn call(
        self,
        ctx: Context,
        service_name: String,
        request: ByteBuf,
    ) -> Result<Result<ByteBuf, ServerError>, DispatchError> {
        if service_name == self.service_name {
            let request: Service::Req =
                deserialize(request).map_err(|e| DispatchError::DecodeRequest(e.to_string()))?;
            let response: Result<Service::Resp, ServerError> =
                self.service.clone().serve(ctx, request).await;
            let serialized_response = match response {
                Ok(resp) => Ok(
                    serialize(resp).map_err(|e| DispatchError::EncodeResponse(e.to_string()))?
                ),
                Err(e) => Err(e),
            };
            Ok(serialized_response)
        } else {
            self.other_services.call(ctx, service_name, request).await
        }
    }
}

async fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(fut);
}

struct MultiClientShim<S, Request, Response> {
    service_name: String,
    inner: MultiServeClient<S>,
    _ghosts: PhantomData<fn(Request) -> Response>,
}

impl<Request, Response>
    MultiClientShim<client::Channel<MultiServeRequest, MultiServeResponse>, Request, Response>
{
    fn new<T>(service_name: &str, config: client::Config, transport: T) -> Self
    where
        T: Transport<ClientMessage<MultiServeRequest>, tarpc::Response<MultiServeResponse>>
            + Send
            + 'static,
    {
        let inner = MultiServeClient::new(config, transport).spawn();
        Self {
            service_name: service_name.into(),
            inner,
            _ghosts: PhantomData,
        }
    }
}

impl<S, Request, Response> Stub for MultiClientShim<S, Request, Response>
where
    Request: Serialize + RequestName,
    Response: for<'a> Deserialize<'a>,
    S: Stub<Req = MultiServeRequest, Resp = MultiServeResponse>,
{
    type Req = Request;
    type Resp = Response;

    async fn call(&self, ctx: Context, request: Self::Req) -> Result<Self::Resp, RpcError> {
        let bytes =
            serialize(request).map_err(|e| RpcError::Channel(ChannelError::Write(Arc::new(e))))?;
        let response = self
            .inner
            .call(ctx, self.service_name.clone(), bytes)
            .await?;
        match response {
            Ok(Ok(resp)) => {
                deserialize(resp).map_err(|e| RpcError::Channel(ChannelError::Read(Arc::new(e))))
            }
            Ok(Err(e @ ServerError { .. })) => Err(RpcError::Server(e)),
            Err(e) => Err(RpcError::Send(Box::new(e))),
        }
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

#[tokio::main]
async fn main() {
    let server_addr = (IpAddr::V6(Ipv6Addr::LOCALHOST), 13241);
    tokio::spawn(async move {
        let mut listener = serde_transport::tcp::listen(&server_addr, Json::default)
            .await
            .unwrap();
        let transport = listener.next().await.unwrap().unwrap();
        let channel = BaseChannel::with_defaults(transport);
        let registry = ServiceRegistry.with_services((
            Service {
                name: "Hello",
                service: HelloServer.serve(),
            },
            Service {
                name: "Goobye",
                service: GoodbyeServer.serve(),
            },
        ));
        channel.execute(registry.serve()).for_each(spawn).await;
    });

    let transport = serde_transport::tcp::connect(server_addr, Json::default)
        .await
        .unwrap();
    let multi_client = MultiClientShim::new("Hello", Default::default(), transport);
    let hello_client = HelloClient::from(multi_client);

    let hello = hello_client
        .hello(context::current(), "Tim".into())
        .await
        .unwrap();
    println!("{hello}");
}
