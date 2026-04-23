use omnifs_sdk::Cx;
use omnifs_sdk::prelude::*;
use serde::Deserialize;

use crate::http_ext::GithubHttpExt;
use crate::types::OwnerName;
use crate::{OwnerKind, Result, State};

#[derive(Debug, Deserialize)]
struct UserProfile {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct OrganizationProfile {}

#[derive(Debug, Deserialize)]
struct RepoListing {
    name: String,
}

pub(crate) async fn fetch_owner_repos(
    cx: &Cx<State>,
    owner: &OwnerName,
    kind: OwnerKind,
) -> Result<Vec<String>> {
    const MAX_PAGES: u32 = 50;
    const PAGE_SIZE: usize = 100;
    const CONCURRENCY: u32 = 5;

    let scope = match kind {
        OwnerKind::User => "users",
        OwnerKind::Org => "orgs",
    };
    let base = format!("/{scope}/{owner}/repos?per_page={PAGE_SIZE}&sort=updated");

    // Fetch first page synchronously to size the result and detect trivial cases.
    let first: Vec<RepoListing> = cx.github_json(format!("{base}&page=1")).await?;
    if first.len() < PAGE_SIZE {
        return Ok(first.into_iter().map(|r| r.name).collect());
    }

    let mut names: Vec<String> = first.into_iter().map(|r| r.name).collect();
    let mut next_page = 2u32;

    while next_page <= MAX_PAGES {
        let batch_end = (next_page + CONCURRENCY - 1).min(MAX_PAGES);
        let requests = (next_page..=batch_end)
            .map(|page| cx.github_json::<Vec<RepoListing>>(format!("{base}&page={page}")));

        for batch in join_all(requests).await {
            let repos = batch?;
            let done = repos.len() < PAGE_SIZE;
            names.extend(repos.into_iter().map(|r| r.name));
            if done {
                return Ok(names);
            }
        }

        next_page = batch_end + 1;
    }

    Ok(names)
}

pub(crate) async fn resolve_owner_kind(
    cx: &Cx<State>,
    owner: &OwnerName,
) -> Result<Option<OwnerKind>> {
    match cx
        .github_json::<UserProfile>(format!("/users/{owner}"))
        .await
    {
        Ok(profile) => {
            return Ok(Some(if profile.kind == "Organization" {
                OwnerKind::Org
            } else {
                OwnerKind::User
            }));
        },
        Err(error) if matches!(error.kind(), ProviderErrorKind::NotFound) => {},
        Err(error) => return Err(error),
    }

    match cx
        .github_json::<OrganizationProfile>(format!("/orgs/{owner}"))
        .await
    {
        Ok(_) => Ok(Some(OwnerKind::Org)),
        Err(error) if matches!(error.kind(), ProviderErrorKind::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}
