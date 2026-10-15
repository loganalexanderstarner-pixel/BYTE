//! The offline switch: while it's on, nothing in BYTE reaches the internet.
//!
//! The check sits in the DNS resolvers every internet client uses
//! (`tools::fetch::web_client`, the cloud client, the model lab, connectors),
//! so a feature that forgets to ask still can't get out. The local engine
//! (127.0.0.1) doesn't resolve names and keeps working.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, AppResult};

static OFFLINE: AtomicBool = AtomicBool::new(false);

pub const MESSAGE: &str = "BYTE is offline. Turn the offline switch off (Settings → Privacy) to use the internet.";

pub fn is_offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

pub fn set(on: bool) {
    OFFLINE.store(on, Ordering::Relaxed);
}

/// Err when offline; for work that should say so before trying.
pub fn guard() -> AppResult<()> {
    if is_offline() {
        Err(AppError::msg(MESSAGE))
    } else {
        Ok(())
    }
}

/// The system resolver, refusing every name while offline. For internet
/// clients that may reach any address (the cloud can be on a private network).
pub struct OfflineAwareResolver;

impl reqwest::dns::Resolve for OfflineAwareResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            if is_offline() {
                return Err(MESSAGE.into());
            }
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A client builder for the internet: refuses every connection while offline.
pub fn client_builder() -> reqwest::ClientBuilder {
    guarded(reqwest::Client::builder().dns_resolver(std::sync::Arc::new(OfflineAwareResolver)))
}

/// Adds the offline check to a client's connections. The resolver alone isn't
/// enough: through a proxy, the proxy looks up the site's name, not BYTE.
pub fn guarded(b: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    b.connector_layer(OfflineLayer { flag: &OFFLINE })
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Fails connections while `flag` is set (the app's switch; tests use their own).
#[derive(Clone)]
pub struct OfflineLayer {
    flag: &'static AtomicBool,
}

impl<S> tower_layer::Layer<S> for OfflineLayer {
    type Service = OfflineConnect<S>;
    fn layer(&self, inner: S) -> Self::Service {
        OfflineConnect(inner, self.flag)
    }
}

#[derive(Clone)]
pub struct OfflineConnect<S>(S, &'static AtomicBool);

impl<S, R> tower_service::Service<R> for OfflineConnect<S>
where
    S: tower_service::Service<R, Error = BoxError>,
    S::Future: Send + 'static,
    S::Response: 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<S::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), BoxError>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        if self.1.load(Ordering::Relaxed) {
            return Box::pin(async { Err(BoxError::from(MESSAGE)) });
        }
        Box::pin(self.0.call(req))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The app-wide switch isn't flipped here: tests run side by side, and
    // the others would lose their (local mock) connections.
    static TEST_SWITCH: AtomicBool = AtomicBool::new(false);

    async fn try_get(url: &str) -> Result<reqwest::Response, reqwest::Error> {
        let client = reqwest::Client::builder().connector_layer(OfflineLayer { flag: &TEST_SWITCH }).build().unwrap();
        client.get(url).send().await
    }

    #[tokio::test]
    async fn the_switch_refuses_connections_even_through_a_proxy() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::any()).respond_with(wiremock::ResponseTemplate::new(200)).mount(&server).await;
        TEST_SWITCH.store(true, Ordering::Relaxed);
        let e = try_get(&server.uri()).await.unwrap_err();
        assert!(format!("{e:?}").contains("offline"), "{e:?}");
        // A proxy is a connection too.
        let proxied = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(server.uri()).unwrap())
            .connector_layer(OfflineLayer { flag: &TEST_SWITCH })
            .build()
            .unwrap();
        assert!(proxied.get("http://example.com/").send().await.is_err());
        TEST_SWITCH.store(false, Ordering::Relaxed);
        assert_eq!(try_get(&server.uri()).await.unwrap().status(), 200);
    }

    #[test]
    fn the_message_says_how_to_go_back_online() {
        assert!(MESSAGE.contains("Settings → Privacy"));
        assert!(!is_offline(), "tests never leave the app switch on");
        assert!(guard().is_ok());
    }
}
