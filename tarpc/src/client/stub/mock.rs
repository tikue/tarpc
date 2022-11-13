use crate::{
    client::{stub::Stub, RpcError},
    context, ServerError,
};
use futures::future;
use std::{collections::HashMap, hash::Hash, io};

/// A mock stub that returns user-specified responses.
pub struct Mock<Req, Resp> {
    responses: HashMap<Req, Resp>,
}

impl<Req, Resp> Mock<Req, Resp>
where
    Req: Eq + Hash,
{
    /// Returns a new mock, mocking the specified (request, response) pairs.
    pub fn new<const N: usize>(responses: [(Req, Resp); N]) -> Self {
        Self {
            responses: HashMap::from(responses),
        }
    }
}

impl<Req, Resp> Stub for Mock<Req, Resp>
where
    Req: Eq + Hash,
    Resp: Clone,
{
    type Req = Req;
    type Resp = Resp;
    type RespFut<'a> = future::Ready<Result<Resp, RpcError>>
        where Self: 'a;

    fn call<'a>(
        &'a self,
        _: context::Context,
        _: &'static str,
        request: Self::Req,
    ) -> Self::RespFut<'a> {
        future::ready(
            self.responses
                .get(&request)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| {
                    Err(RpcError::Server(ServerError {
                        kind: io::ErrorKind::NotFound,
                        detail: "mock (request, response) entry not found".into(),
                    }))
                }),
        )
    }
}
