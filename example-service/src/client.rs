// Copyright 2018 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

#![feature(async_await)]

use clap::{App, Arg};
use std::io;
use tarpc::{client, context};

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> io::Result<()> {
    let flags = App::new("Hello Client")
        .version("0.1")
        .author("Tim <tikue@google.com>")
        .about("Say hello!")
        .arg(
            Arg::with_name("server_addr")
                .long("server_addr")
                .value_name("ADDRESS")
                .help("Sets the server address to connect to.")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("name")
                .short("n")
                .long("name")
                .value_name("STRING")
                .help("Sets the name to say hello to.")
                .required(true)
                .takes_value(true),
        )
        .get_matches();

    let server_addr = flags.value_of("server_addr").unwrap();
    let server_addr = server_addr
        .parse()
        .unwrap_or_else(|e| panic!(r#"--server_addr value "{}" invalid: {}"#, server_addr, e));

    let name = flags.value_of("name").unwrap().into();

    let transport = json_transport::connect(&server_addr).await?;

    // new_stub is generated by the service! macro. Like Server, it takes a config and any
    // Transport as input, and returns a Client, also generated by the macro.
    // by the service mcro.
    let mut client = service::new_stub(client::Config::default(), transport).await?;

    // The client has an RPC method for each RPC defined in service!. It takes the same args
    // as defined, with the addition of a Context, which is always the first arg. The Context
    // specifies a deadline and trace information which can be helpful in debugging requests.
    let hello = client.hello(context::current(), name).await?;

    println!("{}", hello);

    Ok(())
}