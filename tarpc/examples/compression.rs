#![allow(incomplete_features)]
#![feature(generic_associated_types)]

use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use futures::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use log::info;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::{io, io::Read, io::Write};
use tarpc::rpc::context::HasContext;
use tarpc::{
    client, context,
    serde_transport::tcp,
    server::{BaseChannel, Channel},
};
use tokio_serde::formats::Bincode;

/// Type of compression that should be enabled on the request. The transport is free to ignore this.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Deserialize, Serialize)]
pub enum CompressionAlgorithm {
    Deflate,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum CompressedMessage<T> {
    Uncompressed(T),
    Compressed {
        algorithm: CompressionAlgorithm,
        payload: ByteBuf,
    },
}

async fn compress<T>(message: T) -> io::Result<CompressedMessage<T>>
where
    T: Serialize + HasContext,
{
    if let Some(CompressionAlgorithm::Deflate) = message.context().extensions.get() {
        info!("Sending compressed.");
        let message = serialize(message)?;
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&message).unwrap();
        let compressed = encoder.finish()?;
        Ok(CompressedMessage::Compressed {
            algorithm: CompressionAlgorithm::Deflate,
            payload: ByteBuf::from(compressed),
        })
    } else {
        info!("Sending uncompressed.");
        Ok(CompressedMessage::Uncompressed(message))
    }
}

#[derive(Clone, Copy, Debug)]
struct SerializedSize(u64);

async fn decompress<T>(message: CompressedMessage<T>) -> io::Result<T>
where
    for<'a> T: Serialize + Deserialize<'a>,
    T: HasContext,
{
    let (mut message, serialized_size) = match message {
        CompressedMessage::Compressed { algorithm, payload } => {
            if algorithm != CompressionAlgorithm::Deflate {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Compression algorithm {:?} not supported", algorithm),
                ));
            }
            let payload_len = payload.len();
            let mut deflater = DeflateDecoder::new(payload.as_slice());
            let mut payload = ByteBuf::new();
            deflater.read_to_end(&mut payload)?;
            let mut message: T = deserialize(payload)?;
            // Serialize on the way back
            message
                .context_mut()
                .extensions
                .insert(CompressionAlgorithm::Deflate);
            (message, payload_len as u64)
        }
        CompressedMessage::Uncompressed(message) => {
            let serialized_size = serialized_size(&message)?;
            (message, serialized_size)
        }
    };
    message
        .context_mut()
        .extensions
        .insert(SerializedSize(serialized_size));
    Ok(message)
}

fn serialized_size<T: Serialize>(t: &T) -> io::Result<u64> {
    bincode::serialized_size(&t).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn serialize<T: Serialize>(t: T) -> io::Result<ByteBuf> {
    bincode::serialize(&t)
        .map(ByteBuf::from)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn deserialize<D>(message: ByteBuf) -> io::Result<D>
where
    for<'a> D: Deserialize<'a>,
{
    bincode::deserialize(message.as_ref()).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn add_compression<In, Out>(
    transport: impl Stream<Item = io::Result<CompressedMessage<In>>>
        + Sink<CompressedMessage<Out>, Error = io::Error>,
) -> impl Stream<Item = io::Result<In>> + Sink<Out, Error = io::Error>
where
    for<'a> In: Deserialize<'a>,
    In: Serialize + HasContext,
    Out: Serialize + HasContext,
{
    transport.with(compress).and_then(decompress)
}

#[tarpc::service]
pub trait World {
    async fn hello(name: String) -> String;
}

#[derive(Clone, Debug)]
struct HelloServer;

#[tarpc::server]
impl World for HelloServer {
    async fn hello(self, ctx: &mut context::Context, name: String) -> String {
        let serialized_size = ctx
            .extensions
            .get::<SerializedSize>()
            .copied()
            .map(|s| s.0)
            .unwrap_or(0);
        if ctx.extensions.get::<CompressionAlgorithm>().is_none() && serialized_size >= 80 {
            info!("Oops! Request was big; compressing response.");
            ctx.extensions.insert(CompressionAlgorithm::Deflate);
        }
        format!(
            "Hey, {}! You were {} bytes on the wire.",
            name, serialized_size
        )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut incoming = tcp::listen("localhost:0", Bincode::default).await?;
    let addr = incoming.local_addr();
    tokio::spawn(async move {
        let transport = incoming.next().await.unwrap().unwrap();
        BaseChannel::with_defaults(add_compression(transport))
            .respond_with(HelloServer.serve())
            .execute()
            .await;
    });

    let transport = tcp::connect(addr, Bincode::default()).await?;
    let mut client =
        WorldClient::new(client::Config::default(), add_compression(transport)).spawn()?;

    println!(
        "{}",
        client
            .hello(
                context::current(),
                "heeeeeeeeeeeeeelllllllloooooooooooooooooooooooooooo".into()
            )
            .await?
    );

    let mut context = context::current();
    context.extensions.insert(CompressionAlgorithm::Deflate);

    println!(
        "{}",
        client
            .hello(
                context,
                "heeeeeeeeeeeeeelllllllloooooooooooooooooooooooooooo".into()
            )
            .await?
    );

    Ok(())
}
