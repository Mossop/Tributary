//! Lists Phabricator revisions: those waiting for my review, my own that
//! need work, and my own that are ready to land.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use chrono::DateTime;
use serde::Deserialize;
use serde_json::json;

use super::{
    Field, FieldKind, FieldOption, ItemKind, QuickAction, Source, SourceContext, SourceError,
    SourceItem, SourceType,
};
use crate::accounts::{Accounts, PHABRICATOR};

pub mod conduit;

use conduit::{Conduit, ConduitError, Project, Reviewer, Revision, User};

pub const ICON: &str = "https://phabricator.services.mozilla.com/file/data/kcrevgcjzkxey3mpl4ww/PHID-FILE-v7mjs2z5uqokqat33d4p/favicon";
const LANDO_URL: &str = "https://lando.moz.tools";

/// Reviewer statuses meaning a review is still wanted. `-older` means the
/// review was of an earlier diff.
const PENDING: &[&str] = &[
    "added",
    "blocking",
    "commented",
    "accepted-older",
    "rejected-older",
];

/// Which revisions to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum List {
    /// Waiting for my review, directly or through one of my groups.
    Review,
    /// Mine, needing more work.
    Work,
    /// Mine, accepted.
    Land,
}

impl List {
    const ALL: [(List, &str, &str); 3] = [
        (List::Review, "review", "Patches to review"),
        (List::Work, "work", "Patches to work on"),
        (List::Land, "land", "Patches ready to land"),
    ];
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhabricatorConfig {
    pub account: String,
    pub list: List,
}

pub fn source_type() -> SourceType {
    SourceType::new(
        "phabricator",
        "Phabricator",
        vec![
            Field {
                key: "account",
                label: "Phabricator account",
                kind: FieldKind::Account,
                required: true,
                placeholder: None,
                help: None,
                provider: Some(PHABRICATOR),
            },
            Field {
                key: "list",
                label: "Show",
                kind: FieldKind::Select,
                required: true,
                placeholder: None,
                help: None,
                provider: None,
            },
        ],
        |config, context| {
            let config: PhabricatorConfig = serde_json::from_value(config.clone())
                .map_err(|_| anyhow!("Choose what to show"))?;
            Ok(Arc::new(PhabricatorSource::new(context, config)?))
        },
    )
    .with_options(|_, _, _| {
        Box::pin(async {
            Ok(List::ALL
                .iter()
                .map(|(_, value, label)| FieldOption {
                    value: value.to_string(),
                    label: label.to_string(),
                })
                .collect())
        })
    })
}

/// Why a revision needs my review.
#[derive(Debug, PartialEq)]
pub enum ReviewReason {
    /// I'm a reviewer myself.
    Direct,
    /// One of my groups is, and no other member of it is a reviewer.
    Group(String),
}

/// Whether `revision` is waiting for my review. `groups` maps each of my
/// groups to its members.
pub fn review_reason(
    revision: &Revision,
    me: &str,
    groups: &HashMap<String, HashSet<String>>,
) -> Option<ReviewReason> {
    if revision.fields.author_phid == me {
        return None;
    }
    let reviewers = &revision.attachments.reviewers.reviewers;
    let pending = |reviewer: &Reviewer| PENDING.contains(&reviewer.status.as_str());
    if let Some(mine) = reviewers.iter().find(|r| r.phid == me) {
        return pending(mine).then_some(ReviewReason::Direct);
    }
    reviewers.iter().filter(|r| pending(r)).find_map(|group| {
        let members = groups.get(&group.phid)?;
        // Someone else from the group is reviewing it themselves.
        let covered = reviewers.iter().any(|other| {
            other.phid != me && other.status != "resigned" && members.contains(&other.phid)
        });
        (!covered).then(|| ReviewReason::Group(group.phid.clone()))
    })
}

pub struct PhabricatorSource {
    http: reqwest::Client,
    accounts: Arc<Accounts>,
    account: String,
    list: List,
}

impl PhabricatorSource {
    pub fn new(context: &SourceContext, config: PhabricatorConfig) -> anyhow::Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder().user_agent("tributary").build()?,
            accounts: context.accounts.clone(),
            account: config.account,
            list: config.list,
        })
    }

    async fn items(&self) -> anyhow::Result<Vec<SourceItem>> {
        let (account, token) = self
            .accounts
            .token(&self.account, PHABRICATOR, "Phabricator")
            .await?;
        let api_url = conduit::api_url(account.api_url.as_deref());
        let conduit = Conduit::new(self.http.clone(), &api_url, &token);
        let result = async {
            let me = conduit.whoami().await?.phid;
            match self.list {
                List::Review => reviews(&conduit, &me).await,
                List::Work => {
                    let statuses = ["draft", "needs-revision", "changes-planned"];
                    authored(&conduit, &me, &statuses, false).await
                }
                List::Land => {
                    let lando = api_url == conduit::DEFAULT_API_URL;
                    authored(&conduit, &me, &["accepted"], lando).await
                }
            }
        }
        .await;
        match result {
            Err(ConduitError::Rejected) => {
                self.accounts.mark_revoked(&self.account);
                bail!(
                    "Phabricator account {} needs reconnecting in settings",
                    account.name
                )
            }
            result => Ok(result?),
        }
    }
}

fn item(revision: &Revision, description: String) -> SourceItem {
    SourceItem {
        id: format!("D{}", revision.id),
        kind: ItemKind::Patch,
        title: revision.fields.title.clone(),
        url: revision.fields.uri.clone(),
        icon: None,
        modified: DateTime::from_timestamp(revision.fields.date_modified, 0).unwrap_or_default(),
        description: Some(description),
        due: None,
        actions: Vec::new(),
    }
}

/// Revisions waiting for my review.
async fn reviews(conduit: &Conduit, me: &str) -> Result<Vec<SourceItem>, ConduitError> {
    let projects: Vec<Project> = conduit
        .search(
            "project.search",
            json!({
                "constraints": { "members": [me] },
                "attachments": { "members": true },
            }),
        )
        .await?;
    let groups: HashMap<String, HashSet<String>> = projects
        .iter()
        .map(|p| {
            let members = p.attachments.members.members.iter();
            (p.phid.clone(), members.map(|m| m.phid.clone()).collect())
        })
        .collect();
    let group_names: HashMap<&str, String> = projects
        .iter()
        .map(|p| {
            let name = match &p.fields.slug {
                Some(slug) => format!("#{slug}"),
                None => p.fields.name.clone(),
            };
            (p.phid.as_str(), name)
        })
        .collect();

    let mut reviewers = vec![me.to_string()];
    reviewers.extend(groups.keys().cloned());
    let revisions: Vec<Revision> = conduit
        .search(
            "differential.revision.search",
            json!({
                "constraints": { "reviewerPHIDs": reviewers, "statuses": ["needs-review"] },
                "attachments": { "reviewers": true },
                "order": "updated",
            }),
        )
        .await?;
    let wanted: Vec<(Revision, ReviewReason)> = revisions
        .into_iter()
        .filter_map(|revision| {
            let reason = review_reason(&revision, me, &groups)?;
            Some((revision, reason))
        })
        .collect();

    let authors = user_names(conduit, wanted.iter().map(|(r, _)| &r.fields.author_phid)).await?;
    Ok(wanted
        .iter()
        .map(|(revision, reason)| {
            let author = &revision.fields.author_phid;
            let author = authors.get(author).unwrap_or(author);
            let mut description = format!("D{} · by {author}", revision.id);
            if let ReviewReason::Group(group) = reason {
                let group = group_names.get(group.as_str()).unwrap_or(group);
                description.push_str(&format!(" · for {group}"));
            }
            item(revision, description)
        })
        .collect())
}

/// My revisions with one of `statuses`, optionally with a quick action to
/// open them in Lando.
async fn authored(
    conduit: &Conduit,
    me: &str,
    statuses: &[&str],
    lando: bool,
) -> Result<Vec<SourceItem>, ConduitError> {
    let revisions: Vec<Revision> = conduit
        .search(
            "differential.revision.search",
            json!({
                "constraints": { "authorPHIDs": [me], "statuses": statuses },
                "order": "updated",
            }),
        )
        .await?;
    Ok(revisions
        .iter()
        .map(|revision| authored_item(revision, lando))
        .collect())
}

/// The item for one of my revisions, with a link to land it in Lando if
/// `lando`.
fn authored_item(revision: &Revision, lando: bool) -> SourceItem {
    let description = format!("D{} · {}", revision.id, revision.fields.status.name);
    let mut item = item(revision, description);
    if lando {
        item.actions.push(QuickAction {
            id: "lando".to_string(),
            label: "Open in Lando".to_string(),
            icon: "symbol:flight_takeoff".to_string(),
            url: Some(format!("{LANDO_URL}/D{}/", revision.id)),
        });
    }
    item
}

/// Usernames for user PHIDs.
async fn user_names<'a>(
    conduit: &Conduit,
    phids: impl Iterator<Item = &'a String>,
) -> Result<HashMap<String, String>, ConduitError> {
    let phids: HashSet<&String> = phids.collect();
    if phids.is_empty() {
        return Ok(HashMap::new());
    }
    let users: Vec<User> = conduit
        .search("user.search", json!({ "constraints": { "phids": phids } }))
        .await?;
    Ok(users
        .into_iter()
        .map(|user| (user.phid, user.fields.username))
        .collect())
}

#[async_trait]
impl Source for PhabricatorSource {
    fn icon(&self) -> String {
        ICON.to_string()
    }

    async fn fetch(&self) -> Result<Vec<SourceItem>, SourceError> {
        self.items()
            .await
            .map_err(|error| SourceError::Message(format!("{error:#}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit::{Reviewers, RevisionAttachments, RevisionFields, RevisionStatus};

    const ME: &str = "PHID-USER-me";
    const GROUP: &str = "PHID-PROJ-group";

    fn revision(author: &str, reviewers: &[(&str, &str)]) -> Revision {
        Revision {
            id: 1,
            fields: RevisionFields {
                title: "Title".into(),
                uri: "https://phab.test/D1".into(),
                author_phid: author.into(),
                status: RevisionStatus {
                    value: "needs-review".into(),
                    name: "Needs Review".into(),
                },
                date_modified: 0,
            },
            attachments: RevisionAttachments {
                reviewers: Reviewers {
                    reviewers: reviewers
                        .iter()
                        .map(|(phid, status)| Reviewer {
                            phid: phid.to_string(),
                            status: status.to_string(),
                        })
                        .collect(),
                },
            },
        }
    }

    fn groups() -> HashMap<String, HashSet<String>> {
        let members = [ME, "PHID-USER-colleague"].map(String::from);
        HashMap::from([(GROUP.to_string(), members.into_iter().collect())])
    }

    fn reason(author: &str, reviewers: &[(&str, &str)]) -> Option<ReviewReason> {
        review_reason(&revision(author, reviewers), ME, &groups())
    }

    #[test]
    fn direct_reviews_until_reviewed() {
        let other = "PHID-USER-author";
        for status in PENDING {
            assert_eq!(reason(other, &[(ME, status)]), Some(ReviewReason::Direct));
        }
        for status in ["accepted", "rejected", "resigned"] {
            assert_eq!(reason(other, &[(ME, status)]), None, "{status}");
        }
        // Reviewing directly takes precedence over a group request.
        assert_eq!(reason(other, &[(ME, "accepted"), (GROUP, "added")]), None);
    }

    #[test]
    fn group_reviews_unless_another_member_is_reviewing() {
        let other = "PHID-USER-author";
        assert_eq!(
            reason(other, &[(GROUP, "added")]),
            Some(ReviewReason::Group(GROUP.into()))
        );
        assert_eq!(
            reason(
                other,
                &[(GROUP, "blocking"), ("PHID-USER-colleague", "added")]
            ),
            None
        );
        // A member who resigned doesn't count.
        assert_eq!(
            reason(
                other,
                &[(GROUP, "added"), ("PHID-USER-colleague", "resigned")]
            ),
            Some(ReviewReason::Group(GROUP.into()))
        );
        // Nor does someone outside the group.
        assert_eq!(
            reason(other, &[(GROUP, "added"), ("PHID-USER-stranger", "added")]),
            Some(ReviewReason::Group(GROUP.into()))
        );
        // The group already reviewed it.
        assert_eq!(reason(other, &[(GROUP, "accepted")]), None);
        // Groups I'm not in.
        assert_eq!(reason(other, &[("PHID-PROJ-other", "added")]), None);
    }

    #[test]
    fn links_to_lando() {
        let mut accepted = revision(ME, &[]);
        accepted.id = 42;
        accepted.fields.status.name = "Accepted".into();
        let item = authored_item(&accepted, true);
        assert_eq!(item.id, "D42");
        assert_eq!(item.description.as_deref(), Some("D42 · Accepted"));
        assert_eq!(
            item.actions,
            [QuickAction {
                id: "lando".into(),
                label: "Open in Lando".into(),
                icon: "symbol:flight_takeoff".into(),
                url: Some("https://lando.moz.tools/D42/".into()),
            }]
        );
        assert!(authored_item(&accepted, false).actions.is_empty());
    }

    #[test]
    fn skips_my_own_revisions() {
        assert_eq!(reason(ME, &[(ME, "added")]), None);
        assert_eq!(reason(ME, &[(GROUP, "added")]), None);
    }
}
