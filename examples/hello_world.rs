#![feature(try_from)]

use clap::{App, Arg};
use futures::future::Future;
use ruma_client::{
    api::r0::{membership::join_room_by_id_or_alias, send::send_message_event},
    Client,
};
use ruma_events::{
    room::message::{MessageEventContent, MessageType, TextMessageEventContent},
    EventType,
};
use tokio::runtime::current_thread;

use std::{
    convert::TryInto,
    io::{self, Write},
};

fn main() {
    let cli_matches = App::new("ruma-client-hello-world")
        .about("Write a message to a channel using the specified credentials")
        .arg(
            Arg::with_name("room_alias")
                .short("r")
                .long("room")
                .value_name("ROOM_ALIAS")
                .help("Target room alias; has to exist")
                .default_value("ruma-client-test-room"),
        )
        .arg(
            Arg::with_name("homeserver")
                .short("s")
                .long("server")
                .value_name("HOMESERVER")
                .help("The room's and user's homeserver; has to be HTTPS")
                .default_value("https://matrix.org"),
        )
        .arg(
            Arg::with_name("message")
                .short("m")
                .long("message")
                .value_name("MSG")
                .help("Whatever you'd like the client to say for you")
                .default_value("Hello, World"),
        )
        .get_matches();

    // Get the username
    let mut user = String::new();
    print!("User: ");
    io::stdout().flush().unwrap();
    io::stdin()
        .read_line(&mut user)
        .expect("Could not prompt username");
    user = user.replace('\n', "");

    // Get the password sudo-style (with echo off)
    let pass = rpassword::prompt_password_stdout("Password: ").unwrap();

    let room_alias = cli_matches.value_of("room_alias").unwrap();
    let homeserver = cli_matches.value_of("homeserver").unwrap();
    let message = cli_matches.value_of("message").unwrap();

    let mut client =
        Client::new_https(homeserver.parse().unwrap()).expect("Could not connect to Matrix");

    // Password is moved into and dropped at the end of log_in()
    client.log_in(&user, pass, None).expect("Could not log in");

    println!("The logged in client: {:#?}", client);

    let fut = join_room_by_id_or_alias::call(
        &client,
        join_room_by_id_or_alias::Request {
            room_id_or_alias: format!("#{}:matrix.org", room_alias)
                .as_str()
                .try_into()
                .unwrap(),
            third_party_signed: None,
        },
    )
    .map(|res| res.room_id)
    .and_then(|room_id| {
        send_message_event::call(
            &client,
            send_message_event::Request {
                room_id: room_id,
                event_type: EventType::RoomMessage,
                txn_id: "1".to_owned(),
                data: MessageEventContent::Text(TextMessageEventContent {
                    body: message.to_owned(),
                    msgtype: MessageType::Text,
                }),
            },
        )
    });

    println!(
        "Top-level result: {:?}",
        current_thread::block_on_all(fut).unwrap()
    );
}
