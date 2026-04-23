//! HTTP callout result extraction and typed async HTTP builders.

use crate::cx::Cx;
use crate::error::{ProviderError, Result};
use crate::omnifs::provider::types::{Callout, CalloutResult, Header, HttpRequest, HttpResponse};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

pub struct Builder<'cx, S> {
    cx: &'cx Cx<S>,
}

impl<'cx, S> Builder<'cx, S> {
    pub fn new(cx: &'cx Cx<S>) -> Self {
        Self { cx }
    }

    pub fn get(self, url: impl Into<String>) -> Request<'cx, S> {
        Request {
            cx: self.cx,
            method: "GET".to_string(),
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }
}

pub struct Request<'cx, S> {
    cx: &'cx Cx<S>,
    method: String,
    url: String,
    headers: Vec<Header>,
    body: Option<Vec<u8>>,
}

impl<'cx, S> Request<'cx, S> {
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn send(self) -> CalloutFuture<'cx, S, HttpResponse> {
        CalloutFuture::new(
            self.cx,
            Callout::Fetch(HttpRequest {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
            }),
            |result| match result {
                CalloutResult::HttpResponse(resp) if resp.status < 400 => Ok(resp),
                CalloutResult::HttpResponse(resp) => {
                    Err(ProviderError::from_http_status(resp.status))
                },
                CalloutResult::CalloutError(e) => Err(ProviderError::from_callout_error(&e)),
                _ => Err(ProviderError::internal("unexpected callout result type")),
            },
        )
    }

    pub fn send_body(self) -> CalloutFuture<'cx, S, Vec<u8>> {
        CalloutFuture::new(
            self.cx,
            Callout::Fetch(HttpRequest {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
            }),
            |result| match result {
                CalloutResult::HttpResponse(resp) if resp.status < 400 => Ok(resp.body),
                CalloutResult::HttpResponse(resp) => {
                    Err(ProviderError::from_http_status(resp.status))
                },
                CalloutResult::CalloutError(e) => Err(ProviderError::from_callout_error(&e)),
                _ => Err(ProviderError::internal("unexpected callout result type")),
            },
        )
    }
}

/// Future that yields a single callout and resolves with a typed result.
pub struct CalloutFuture<'cx, S, T> {
    cx: &'cx Cx<S>,
    callout: Option<Callout>,
    extract: fn(CalloutResult) -> Result<T>,
}

impl<'cx, S, T> CalloutFuture<'cx, S, T> {
    pub(crate) fn new(
        cx: &'cx Cx<S>,
        callout: Callout,
        extract: fn(CalloutResult) -> Result<T>,
    ) -> Self {
        Self {
            cx,
            callout: Some(callout),
            extract,
        }
    }
}

impl<S, T> Future for CalloutFuture<'_, S, T> {
    type Output = Result<T>;

    fn poll(mut self: Pin<&mut Self>, _ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(callout) = self.callout.take() {
            self.cx.push_yielded(callout);
            return Poll::Pending;
        }

        if let Some(result) = self.cx.pop_delivered() {
            let extract = self.extract;
            return Poll::Ready(extract(result));
        }

        Poll::Pending
    }
}
