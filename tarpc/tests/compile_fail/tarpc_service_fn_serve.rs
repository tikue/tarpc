#![allow(incomplete_features)]
#![feature(generic_associated_types, type_alias_impl_trait)]

#[tarpc::service]
trait World {
    async fn serve();
}

fn main() {}
