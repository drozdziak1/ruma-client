#![feature(conservative_impl_trait)]

extern crate futures;
extern crate ruma_client;
extern crate ruma_events;
extern crate tokio_core;
extern crate url;

use futures::future::Future;
use futures::stream::Stream;
use ruma_events::collections::all::RoomEvent;
use ruma_events::room::message::{MessageEvent, MessageEventContent, TextMessageEventContent};
use tokio_core::reactor::{Core as TokioCore, Handle as TokioHandle};
use url::Url;

// from https://stackoverflow.com/a/43992218/1592377
#[macro_export]
macro_rules! clone {
    (@param _) => ( _ );
    (@param $x:ident) => ( $x );
    ($($n:ident),+ => move || $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move || $body
        }
    );
    ($($n:ident),+ => move |$($p:tt),+| $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move |$(clone!(@param $p),)+| $body
        }
    );
}

fn log_messages(
    tokio_handle: &TokioHandle,
    homeserver_url: Url,
    username: String,
    password: String,
) -> impl Future<Item = (), Error = ruma_client::Error> + 'static {
    let client = ruma_client::Client::https(tokio_handle, homeserver_url, None).unwrap();

    client.log_in(username, password).and_then(
        clone!(client => move |_| {
            //                            v Skip initial sync reponse
            client.sync(None, None, false).skip(1).for_each(|res| {
                // Only look at rooms the user hasn't left yet
                for (room_id, room) in res.rooms.join {
                    for event in room.timeline.events {
                        // Filter out the text messages
                        if let RoomEvent::RoomMessage(MessageEvent {
                            content: MessageEventContent::Text(
                                TextMessageEventContent {
                                    body: msg_body,
                                    ..
                                }
                            ),
                            user_id,
                            ..
                        }) = event {
                            println!("{:?} in {:?}: {}", user_id, room_id, msg_body);
                        }
                    }
                }

                Ok(())
            })
        }),
    )
}

fn main() {
    let username = "your_username".to_owned();
    let password = "your_password".to_owned();

    let mut core = TokioCore::new().unwrap();
    let handle = core.handle();
    let server = Url::parse("https://matrix.org/").unwrap();

    core.run(log_messages(&handle, server, username, password)).unwrap();
}
