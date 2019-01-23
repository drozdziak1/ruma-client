//! Crate `ruma_client` is a [Matrix](https://matrix.org/) client library.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]
#![feature(try_from)]

/// Matrix client-server API endpoints.
pub mod api;

mod error;
mod session;

use std::{convert::TryInto, str::FromStr};

use futures::{
    future::{Future, FutureFrom, IntoFuture},
    stream::{self, Stream},
};
use hyper::{
    client::{connect::Connect, HttpConnector},
    Client as HyperClient, Uri,
};
#[cfg(feature = "hyper-tls")]
use hyper_tls::HttpsConnector;
#[cfg(feature = "hyper-tls")]
use native_tls::Error as NativeTlsError;
use ruma_api::Endpoint;
use tokio::runtime::current_thread;
use url::Url;

use crate::api::r0::session::login;

pub use crate::{error::Error, session::Session};

/// A client for the Matrix client-server API.
#[derive(Debug)]
pub struct Client<C>
where
    C: Connect,
{
    hyper: HyperClient<C>,
    homeserver_url: Url,
    /// The current Matrix session credentials
    pub session: Option<Session>,
}

impl Client<HttpConnector> {
    /// Creates a new client for making HTTP requests to the given homeserver.
    pub fn new(homeserver_url: Url) -> Self {
        Client {
            homeserver_url,
            hyper: HyperClient::builder().keep_alive(false).build_http(),
            session: None,
        }
    }
}

#[cfg(feature = "tls")]
impl Client<HttpsConnector<HttpConnector>> {
    /// Creates a new client for making HTTPS requests to the given homeserver.
    pub fn new_https(homeserver_url: Url) -> Result<Self, NativeTlsError> {
        let connector = HttpsConnector::new(4)?;

        Ok(Client {
            homeserver_url,
            hyper: { HyperClient::builder().keep_alive(false).build(connector) },
            session: None,
        })
    }
}

impl<C> Client<C>
where
    C: Connect + 'static,
{
    /// Creates a new client using the given `hyper::Client`.
    ///
    /// This allows the user to configure the details of HTTP as desired.
    pub fn new_custom(hyper_client: HyperClient<C>, homeserver_url: Url) -> Self {
        Self {
            homeserver_url,
            hyper: hyper_client,
            session: None,
        }
    }

    /// Log in with a username and password.
    ///
    /// In contrast to api::r0::session::login::call(), this method stores the
    /// session data returned by the endpoint in this client, instead of
    /// returning it.
    pub fn log_in<'a>(
        &'a mut self,
        user: &str,
        password: String,
        device_id: Option<String>,
    ) -> Result<&'a mut Self, Error> {
        let fut = login::call(
            self,
            login::Request {
                address: None,
                login_type: login::LoginType::Password,
                medium: None,
                device_id,
                password,
                user: user.to_owned(),
            },
        )
        .map(|response| {
            Some(Session {
                access_token: response.access_token,
                user_id: response.user_id,
                device_id: response.device_id,
            })
        });

        self.session = current_thread::block_on_all(fut)?;

        Ok(self)
    }

    /// Register as a guest. In contrast to api::r0::account::register::call(),
    /// this method stores the session data returned by the endpoint in this
    /// client, instead of returning it.
    pub fn register_guest<'a>(&'a mut self) -> Result<&'a mut Self, Error> {
        use crate::api::r0::account::register;

        let fut = register::call(
            self,
            register::Request {
                auth: None,
                bind_email: None,
                device_id: None,
                initial_device_display_name: None,
                kind: Some(register::RegistrationKind::Guest),
                password: None,
                username: None,
            },
        )
        .map(|response| {
            Some(Session {
                access_token: response.access_token,
                user_id: response.user_id,
                device_id: response.device_id,
            })
        });

        self.session = current_thread::block_on_all(fut)?;

        Ok(self)
    }

    /// Register as a new user on this server.
    ///
    /// In contrast to api::r0::account::register::call(), this method stores
    /// the session data returned by the endpoint in this client, instead of
    /// returning it.
    ///
    /// The username is the local part of the returned user_id. If it is
    /// omitted from this request, the server will generate one.
    pub fn register_user<'a>(
        &'a mut self,
        username: Option<String>,
        password: String,
    ) -> Result<&'a mut Self, Error> {
        use crate::api::r0::account::register;

        let fut = register::call(
            self,
            register::Request {
                auth: None,
                bind_email: None,
                device_id: None,
                initial_device_display_name: None,
                kind: Some(register::RegistrationKind::User),
                password: Some(password),
                username,
            },
        )
        .map(|response| {
            Some(Session {
                access_token: response.access_token,
                user_id: response.user_id,
                device_id: response.device_id,
            })
        });

        self.session = current_thread::block_on_all(fut)?;

        Ok(self)
    }

    /// Convenience method that represents repeated calls to the sync_events endpoint as a stream.
    ///
    /// If the since parameter is None, the first Item might take a significant time to arrive and
    /// be deserialized, because it contains all events that have occured in the whole lifetime of
    /// the logged-in users account and are visible to them.
    pub fn sync(
        &self,
        filter: Option<api::r0::sync::sync_events::Filter>,
        since: Option<String>,
        set_presence: bool,
    ) -> impl Stream<Item = api::r0::sync::sync_events::Response, Error = Error> + '_ {
        use crate::api::r0::sync::sync_events;

        let set_presence = if set_presence {
            None
        } else {
            Some(sync_events::SetPresence::Offline)
        };

        stream::unfold(since, move |since| {
            let client = self.clone();
            Some(
                sync_events::call(
                    &client.clone(),
                    sync_events::Request {
                        filter: filter.clone(),
                        since,
                        full_state: None,
                        set_presence: set_presence.clone(),
                        timeout: None,
                    },
                )
                .map(|res| {
                    let next_batch_clone = res.next_batch.clone();
                    (res, Some(next_batch_clone))
                }),
            )
        })
    }

    /// Makes a request to a Matrix API endpoint.
    pub(crate) fn request<E>(
        &self,
        request: <E as Endpoint>::Request,
    ) -> impl Future<Item = E::Response, Error = Error>
    where
        E: Endpoint,
    {
        let session_opt = self.session.clone();
        let mut url = self.homeserver_url.clone();
        let hyper_client = self.hyper.clone();

        request
            .try_into()
            .map_err(Error::from)
            .into_future()
            .and_then(move |hyper_request| {
                {
                    let uri = hyper_request.uri();

                    url.set_path(uri.path());
                    url.set_query(uri.query());

                    if E::METADATA.requires_authentication {
                        if let Some(session) = session_opt {
                            url.query_pairs_mut()
                                .append_pair("access_token", &session.access_token.clone());
                        } else {
                            return Err(Error::AuthenticationRequired);
                        }
                    }
                }

                Uri::from_str(url.as_ref())
                    .map(move |uri| (uri, hyper_request))
                    .map_err(Error::from)
            })
            .and_then(move |(uri, mut hyper_request)| {
                *hyper_request.uri_mut() = uri;

                hyper_client.request(hyper_request).map_err(Error::from)
            })
            .and_then(|hyper_response| {
                E::Response::future_from(hyper_response).map_err(Error::from)
            })
    }
}
