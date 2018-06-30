use csrf::{AesGcmCsrfProtection, CsrfProtection,
        CSRF_COOKIE_NAME, CSRF_FORM_FIELD};
use data_encoding::{BASE64, BASE64URL_NOPAD};
use rand::prelude::thread_rng;
use rand::Rng;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::uri::Uri;
use rocket::http::Method::{self, *};
use rocket::outcome::Outcome;
use rocket::response::Body::Sized;
use rocket::{Data, Request, Response, Rocket, State};
use std::collections::HashMap;
use std::env;
use std::io::{Cursor, Read};
use std::str::from_utf8;

use csrf_proxy::CsrfProxy;
use csrf_token::CsrfToken;
use path::Path;
use utils::parse_args;


/// Builder for [CsrfFairing](struct.CsrfFairing.html)
///
/// The `CsrfFairingBuilder` type allows for creation and configuration of a [CsrfFairing](struct.CsrfFairing.html), the
/// main struct of this crate.
///
/// # Usage
/// A Builder is created via the [`new`] method. Then you can configure it with others provided
/// methods, and get a [CsrfFairing](struct.CsrfFairing.html) by a call to [`finalize`]
///
/// [`new`]: #method.new
/// [`finalize`]: #method.finalize
///
/// ## Examples
///
/// The following shippet show 'CsrfFairingBuilder' being used to create a fairing protecting all
/// endpoints and redirecting error to `/csrf-violation` and treat them as if they where `GET`
/// request then.
///
/// ```rust
/// #extern crate rocket_csrf
///
/// use rocket_csrf::CsrfFairingBuilder;
/// fn main() {
///     rocket::ignite()
///         .attach(rocket_csrf::CsrfFairingBuilder::new()
///                 .set_default_target("/csrf-violation", rocket::http::Method::Get)
///                 .finish().unwrap())
///         //add your routes, other fairings...
///         .launch();
/// }
/// ```

pub struct CsrfFairingBuilder {
    duration: i64,
    default_target: (String, Method),
    exceptions: Vec<(String, String, Method)>,
    secret: Option<[u8; 32]>,
    auto_insert: bool,
    auto_insert_disable_prefix: Vec<String>,
    auto_insert_max_size: u64,
}

impl CsrfFairingBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        CsrfFairingBuilder {
            duration: 60 * 60,
            default_target: (String::from("/"), Get),
            exceptions: Vec::new(),
            secret: None,
            auto_insert: true,
            auto_insert_disable_prefix: Vec::new(),
            auto_insert_max_size: 16 * 1024,
        }
    }

    /// Set the timeout (in seconds) of CSRF tokens generated by the final Fairing. Default timeout
    /// is one hour.
    pub fn set_timeout(mut self, timeout: i64) -> Self {
        self.duration = timeout;
        self
    }

    /// Set the default route when an invalide request is catched, you may add a <uri> as a segment
    /// or a param to get the percent-encoded original target. You can also set the method of the
    /// route to which you choosed to redirect.
    ///
    /// # Example
    ///
    ///  ```rust
    /// use rocket_csrf::CsrfFairingBuilder;
    /// fn main() {
    ///     rocket::ignite()
    ///         .attach(rocket_csrf::CsrfFairingBuilder::new()
    ///                 .set_default_target("/csrf-violation", rocket::http::Method::Get)
    ///                 .finish().unwrap())
    ///         //add your routes, other fairings...
    ///         .launch();
    /// }

    pub fn set_default_target(mut self, default_target: String, method: Method) -> Self {
        self.default_target = (default_target, method);
        self
    }

    /// Set the list of exceptions which will not be redirected to the default route, removing any
    /// previously added exceptions, to juste add exceptions use [`add_exceptions`] instead. A route may
    /// contain dynamic parts noted as <name>, which will be replaced in the target route.
    /// Note that this is not aware of Rocket's routes, so matching `/something/<dynamic>` while
    /// match against `/something/static`, even if those are different routes for Rocket. To
    /// circunvence this issue, you can add a (not so) exception matching the static route before
    /// the dynamic one, and redirect it to the default target manually.
    ///
    /// [`add_exceptions`]: #method.add_exceptions
    ///
    /// # Example
    ///
    ///  ```rust
    /// use rocket_csrf::CsrfFairingBuilder;
    /// fn main() {
    ///     rocket::ignite()
    ///         .attach(rocket_csrf::CsrfFairingBuilder::new()
    ///                 .set_exceptions(vec![
    ///                     ("/some/path".to_owned(), "/some/path".to_owned(), rocket::http::Method::Post))//don't verify csrf token
    ///                     ("/some/<other>/path".to_owned(), "/csrf-error?where=<other>".to_owned(), rocket::http::Method::Get))
    ///                 ])
    ///                 .finish().unwrap())
    ///         //add your routes, other fairings...
    ///         .launch();
    /// }
    /// ```
    pub fn set_exceptions(mut self, exceptions: Vec<(String, String, Method)>) -> Self {
        self.exceptions = exceptions;
        self
    }
    /// Add the to list of exceptions which will not be redirected to the default route. See
    /// [`set_exceptions`] for more informations on how exceptions work.
    ///
    /// [`set_exceptions`]: #method.set_exceptions
    pub fn add_exceptions(mut self, exceptions: Vec<(String, String, Method)>) -> Self {
        self.exceptions.extend(exceptions);
        self
    }

    /// Set the secret key used to generate secure cryptographic tokens. If not set, rocket_csrf
    /// will attempt to get the secret used by Rocket for it's own private cookies via the
    /// ROCKET_SECRET_KEY environment variable, or will generate a new one at each restart.
    /// Having the secret key set (via this or Rocket environment variable) allow tokens to keep
    /// their validity in case of an application restart.
    ///
    /// # Example
    ///
    ///  ```rust
    /// use rocket_csrf::CsrfFairingBuilder;
    /// fn main() {
    ///     rocket::ignite()
    ///         .attach(rocket_csrf::CsrfFairingBuilder::new()
    ///                 .set_secret([0;32])//don't do this, use trully secret array instead
    ///                 .finish().unwrap())
    ///         //add your routes, other fairings...
    ///         .launch();
    /// }
    /// ```
    pub fn set_secret(mut self, secret: [u8; 32]) -> Self {
        self.secret = Some(secret);
        self
    }

    /// Set if this should modify response to insert tokens automatically in all forms. If true,
    /// this will insert tokens in all forms it encounter, if false, you will have to add them via
    /// [CsrfFairing](struct.CsrfFairing.html), which you may obtain via request guards.
    ///
    pub fn set_auto_insert(mut self, auto_insert: bool) -> Self {
        self.auto_insert = auto_insert;
        self
    }

    /// Set prefixs for which this will not try to add tokens in forms. This has no effect if
    /// auto_insert is set to false. Not having to parse response on paths witch don't need it may
    /// improve performances, but not that only html documents are parsed, so it's not usefull to
    /// use it on routes containing only images or stillsheets.
    pub fn set_auto_insert_disable_prefix(mut self, auto_insert_prefix: Vec<String>) -> Self {
        self.auto_insert_disable_prefix = auto_insert_prefix;
        self
    }

    /// Set the maximum size of a request before it get send chunked. A request will need at most
    /// this additional memory for the buffer used to parse and tokens into forms. This have no
    /// effect if auto_insert is set to false. Default value is 16Kio
    pub fn set_auto_insert_max_chunk_size(mut self, chunk_size: u64) -> Self {
        self.auto_insert_max_size = chunk_size;
        self
    }

    /// Get the fairing from the builder.
    pub fn finalize(self) -> Result<CsrfFairing, ()> {
        let secret = self.secret.unwrap_or_else(|| {
            //use provided secret if one is
            env::vars()
                .find(|(key, _)| key == "ROCKET_SECRET_KEY")
                .and_then(|(_, value)| {
                    let b64 = BASE64.decode(value.as_bytes());
                    if let Ok(b64) = b64 {
                        if b64.len() == 32 {
                            let mut array = [0; 32];
                            array.copy_from_slice(&b64);
                            Some(array)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })//else get secret environment variable
                .unwrap_or_else(|| {
                    eprintln!("[rocket_csrf] No secret key was found, you should consider set one to keep token validity across application restart");
                    thread_rng().gen()
                }) //if environment variable is not set, generate a random secret and print a warning
        });

        let default_target = Path::from(&self.default_target.0);
        let mut hashmap = HashMap::new();
        hashmap.insert("uri", "");
        if default_target.map(&hashmap).is_none() {
            return Err(());
        } //verify if this path is valid as default path, i.e. it have at most one dynamic part which is <uri>
        Ok(CsrfFairing {
            duration: self.duration,
            default_target: (default_target, self.default_target.1),
            exceptions: self
                .exceptions
                .iter()
                .map(|(a, b, m)| (Path::from(&a), Path::from(&b), *m))//TODO verify if source and target are compatible
                .collect(),
            secret,
            auto_insert: self.auto_insert,
            auto_insert_disable_prefix: self.auto_insert_disable_prefix,
            auto_insert_max_size: self.auto_insert_max_size,
        })
    }
}

impl Default for CsrfFairingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Fairing to protect against Csrf attacks.
///
/// The `CsrfFairing` type protect a Rocket instance against Csrf attack by requesting mendatory
/// token on any POST, PUT, DELETE or PATCH request.
/// This is created via a [CsrfFairingBuilder](struct.CsrfFairingBuilder.html), and implement nothing else than the `Fairing` trait.
///
/// [`CsrfFairingBuilder`]: /rocket_csrf/struct.CsrfFairing.html
pub struct CsrfFairing {
    duration: i64,
    default_target: (Path, Method),
    exceptions: Vec<(Path, Path, Method)>,
    secret: [u8; 32],
    auto_insert: bool,
    auto_insert_disable_prefix: Vec<String>,
    auto_insert_max_size: u64,
}

impl Fairing for CsrfFairing {
    fn info(&self) -> Info {
        if self.auto_insert {
            Info {
                name: "CSRF protection",
                kind: Kind::Attach | Kind::Request | Kind::Response,
            }
        } else {
            Info {
                name: "CSRF protection",
                kind: Kind::Attach | Kind::Request,
            }
        }
    }

    fn on_attach(&self, rocket: Rocket) -> Result<Rocket, Rocket> {
        Ok(rocket.manage((AesGcmCsrfProtection::from_key(self.secret), self.duration))) //add the Csrf engine to Rocket's managed state
    }

    fn on_request(&self, request: &mut Request, data: &Data) {
        match request.method() {
            Get | Head | Connect | Options => {
                let _ = request.guard::<CsrfToken>(); //force regeneration of csrf cookies
                return;
            }
            _ => {}
        };

        let (csrf_engine, _) = request
            .guard::<State<(AesGcmCsrfProtection, i64)>>()
            .unwrap()
            .inner();

        let cookie = request
            .cookies()
            .get(CSRF_COOKIE_NAME)
            .and_then(|cookie| BASE64.decode(cookie.value().as_bytes()).ok())
            .and_then(|cookie| csrf_engine.parse_cookie(&cookie).ok()); //get and parse Csrf cookie

        let _ = request.guard::<CsrfToken>(); //force regeneration of csrf cookies

        let token = parse_args(from_utf8(data.peek()).unwrap_or(""))
            .filter(|(key, _)| key == &CSRF_FORM_FIELD)
            .filter_map(|(_, token)| BASE64URL_NOPAD.decode(&token.as_bytes()).ok())
            .filter_map(|token| csrf_engine.parse_token(&token).ok())
            .next(); //get and parse Csrf token

        if let Some(token) = token {
            if let Some(cookie) = cookie {
                if csrf_engine.verify_token_pair(&token, &cookie) {
                    return; //if we got both token and cookie, and they match each other, we do nothing
                }
            }
        }

        //Request reaching here are violating Csrf protection

        for (src, dst, method) in &self.exceptions {
            if let Some(param) = src.extract(&request.uri().to_string()) {
                if let Some(destination) = dst.map(&param) {
                    request.set_uri(destination);
                    request.set_method(*method);
                    return;
                }
            }
        }

        //if request matched no exception, reroute it to default target

        let uri = request.uri().to_string();
        let uri = Uri::percent_encode(&uri);
        let mut param: HashMap<&str, &str> = HashMap::new();
        param.insert("uri", &uri);
        request.set_uri(self.default_target.0.map(&param).unwrap());
        request.set_method(self.default_target.1)
    }

    fn on_response<'a>(&self, request: &Request, response: &mut Response<'a>) {
        if let Some(ct) = response.content_type() {
            if !ct.is_html() {
                return;
            }
        } //if content type is not html, we do nothing

        let uri = request.uri().to_string();
        if self
            .auto_insert_disable_prefix
            .iter()
            .any(|prefix| uri.starts_with(prefix))
        {
            return;
        } //if request is on an ignored prefix, ignore it

        let token = match request.guard::<CsrfToken>() {
            Outcome::Success(t) => t,
            _ => return,
        }; //if we can't get a token, leave request unchanged, we can't do anything anyway

        let body = response.take_body(); //take request body from Rocket
        if body.is_none() {
            return;
        } //if there was no body, leave it that way
        let body = body.unwrap();

        if let Sized(body_reader, len) = body {
            if len <= self.auto_insert_max_size {
                //if this is a small enought body, process the full body
                let mut res = Vec::with_capacity(len as usize);
                CsrfProxy::from(body_reader, &token)
                    .read_to_end(&mut res)
                    .unwrap();
                response.set_sized_body(Cursor::new(res));
            } else {
                //if body is of known but long size, change it to a stream to preserve memory, by encapsulating it into our "proxy" struct
                let body = body_reader;
                response.set_streamed_body(Box::new(CsrfProxy::from(body, &token)));
            }
        } else {
            //if body is of unknown size, encapsulate it into our "proxy" struct
            let body = body.into_inner();
            response.set_streamed_body(Box::new(CsrfProxy::from(body, &token)));
        }
    }
}
