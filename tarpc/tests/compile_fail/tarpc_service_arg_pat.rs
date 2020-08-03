#![allow(incomplete_features)]
#![feature(generic_associated_types)]

#[tarpc::service]
trait World {
    async fn pat((a, b): (u8, u32));
}

fn main() {}
