use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;
use serde::de::DeserializeOwned;

use crate::Result;
use crate::State;
use crate::{API_BASE, parse_model};

pub(crate) trait GithubHttpExt {
    fn github_get(&self, path: impl AsRef<str>) -> Request<'_, State>;
    fn github_json_request(&self, path: impl AsRef<str>) -> Request<'_, State>;
    fn github_json<T>(
        &self,
        path: impl AsRef<str>,
    ) -> impl core::future::Future<Output = Result<T>>
    where
        T: DeserializeOwned;
}

impl GithubHttpExt for Cx<State> {
    fn github_get(&self, path: impl AsRef<str>) -> Request<'_, State> {
        self.http()
            .get(format!("{API_BASE}{}", path.as_ref()))
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    fn github_json_request(&self, path: impl AsRef<str>) -> Request<'_, State> {
        self.github_get(path)
            .header("Accept", "application/vnd.github+json")
    }

    async fn github_json<T>(&self, path: impl AsRef<str>) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let body = self.github_json_request(path).send_body().await?;
        parse_model(&body)
    }
}
