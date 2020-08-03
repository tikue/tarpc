#![allow(incomplete_features)]
#![feature(generic_associated_types)]

#[tarpc::service]
trait World {
    async fn serve();
}

fn main() {}
