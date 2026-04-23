//! Typed async Git callout builders.

use crate::cx::Cx;
use crate::error::ProviderError;
use crate::http::CalloutFuture;
use crate::omnifs::provider::types::{Callout, CalloutResult, GitOpenRequest, GitRepoInfo};

pub struct Builder<'cx, S> {
    cx: &'cx Cx<S>,
}

impl<'cx, S> Builder<'cx, S> {
    pub fn new(cx: &'cx Cx<S>) -> Self {
        Self { cx }
    }

    pub fn open_repo(
        self,
        cache_key: impl Into<String>,
        clone_url: impl Into<String>,
    ) -> CalloutFuture<'cx, S, GitRepoInfo> {
        CalloutFuture::new(
            self.cx,
            Callout::GitOpenRepo(GitOpenRequest {
                cache_key: cache_key.into(),
                clone_url: clone_url.into(),
            }),
            |result| match result {
                CalloutResult::GitRepoOpened(info) => Ok(info),
                CalloutResult::CalloutError(e) => Err(ProviderError::from_callout_error(&e)),
                _ => Err(ProviderError::internal("unexpected callout result type")),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};
    use std::rc::Rc;

    #[test]
    fn open_repo_yields_git_open_callout() {
        let cx = Cx::new(7, Rc::new(RefCell::new(())));
        let future = cx.git().open_repo(
            "github.com/octocat/Hello-World",
            "git@github.com:octocat/Hello-World.git",
        );
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        assert!(matches!(
            Future::poll(Pin::as_mut(&mut future), &mut context),
            Poll::Pending
        ));

        let Some(Callout::GitOpenRepo(request)) = cx.take_yielded_callout() else {
            panic!("expected git-open-repo callout");
        };
        assert_eq!(request.cache_key, "github.com/octocat/Hello-World");
        assert_eq!(request.clone_url, "git@github.com:octocat/Hello-World.git");
    }
}
