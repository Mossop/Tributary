//! Reference identifiers, which the dashboard uses to group related items.
//!
//! An item's references name what it is about, including the item itself:
//! a bug is `bugzilla:<id>`, a Phabricator revision `phab:D<id>` and a
//! GitHub issue or pull request `github:<owner>/<repo>:<number>` (they share
//! a number space). `bugzilla:` refers to Mozilla's Bugzilla.

use std::sync::LazyLock;

use regex::Regex;

pub fn bugzilla(id: impl std::fmt::Display) -> String {
    format!("bugzilla:{id}")
}

/// A Phabricator revision, by its number without the `D`.
pub fn phab(id: impl std::fmt::Display) -> String {
    format!("phab:D{id}")
}

/// A GitHub issue or pull request in `owner/repo`.
pub fn github(repo: &str, number: impl std::fmt::Display) -> String {
    format!("github:{repo}:{number}")
}

static BUGZILLA_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bhttps?://(?:bugzilla\.mozilla\.org/(?:show_bug\.cgi\?(?:[^\s#]*&)?id=)?|bugzil\.la/)(\d+)\b",
    )
    .unwrap()
});
static PHAB_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bhttps?://phabricator\.services\.mozilla\.com/D(\d+)\b").unwrap()
});
static GITHUB_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bhttps?://github\.com/([\w.-]+/[\w.-]+)/(?:issues|pull)/(\d+)\b").unwrap()
});
/// GitHub's closing keywords followed by `#N`, `owner/repo#N` or an issue
/// URL.
static GITHUB_CLOSING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?):?\s+(?:([\w.-]+/[\w.-]+)?#(\d+)|https?://github\.com/([\w.-]+/[\w.-]+)/issues/(\d+))\b",
    )
    .unwrap()
});

/// Adds `reference` unless it's already there.
pub fn push(references: &mut Vec<String>, reference: String) {
    if !references.contains(&reference) {
        references.push(reference);
    }
}

/// References found as links in free text, e.g. a task's notes.
pub fn in_text(text: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    for c in BUGZILLA_URL.captures_iter(text) {
        found.push((c.get(0).unwrap().start(), bugzilla(&c[1])));
    }
    for c in PHAB_URL.captures_iter(text) {
        found.push((c.get(0).unwrap().start(), phab(&c[1])));
    }
    for c in GITHUB_URL.captures_iter(text) {
        found.push((c.get(0).unwrap().start(), github(&c[1], &c[2])));
    }
    found.sort_by_key(|(start, _)| *start);
    let mut references = Vec::new();
    for (_, reference) in found {
        push(&mut references, reference);
    }
    references
}

/// The GitHub issues a pull request in `repo` says it closes, e.g.
/// "Fixes #12".
pub fn github_closing(text: &str, repo: &str) -> Vec<String> {
    let mut references = Vec::new();
    for c in GITHUB_CLOSING.captures_iter(text) {
        let reference = match (c.get(1), c.get(2), c.get(3), c.get(4)) {
            (other, Some(number), _, _) => {
                github(other.map_or(repo, |m| m.as_str()), number.as_str())
            }
            (_, _, Some(other), Some(number)) => github(other.as_str(), number.as_str()),
            _ => continue,
        };
        push(&mut references, reference);
    }
    references
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bugzilla_links() {
        assert_eq!(
            in_text(
                "See https://bugzilla.mozilla.org/show_bug.cgi?id=123 and \
                 http://bugzilla.mozilla.org/456, https://bugzil.la/789."
            ),
            ["bugzilla:123", "bugzilla:456", "bugzilla:789"]
        );
        assert_eq!(
            in_text("https://bugzilla.mozilla.org/show_bug.cgi?format=default&id=5#c3"),
            ["bugzilla:5"]
        );
    }

    #[test]
    fn finds_phabricator_and_github_links() {
        assert_eq!(
            in_text(
                "Patch: https://phabricator.services.mozilla.com/D42 for \
                 https://github.com/owner/repo/issues/7 (also https://github.com/o/r.js/pull/8)"
            ),
            ["phab:D42", "github:owner/repo:7", "github:o/r.js:8"]
        );
    }

    #[test]
    fn ignores_lookalikes_and_repeats() {
        assert!(
            in_text(
                "bug 123, https://bugzilla.example.com/123, https://phabricator.example.com/D1, \
                 https://github.com/owner/repo"
            )
            .is_empty()
        );
        assert_eq!(
            in_text("https://bugzil.la/1 https://bugzilla.mozilla.org/1"),
            ["bugzilla:1"]
        );
    }

    #[test]
    fn finds_closed_github_issues() {
        let text = "Fixes #12. closes other/repo#3, Resolved: https://github.com/a/b/issues/4\n\
                    fix #12 again; relates to #99";
        assert_eq!(
            github_closing(text, "owner/repo"),
            [
                "github:owner/repo:12",
                "github:other/repo:3",
                "github:a/b:4"
            ]
        );
        for keyword in [
            "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
        ] {
            assert_eq!(
                github_closing(&format!("{keyword} #1"), "o/r"),
                ["github:o/r:1"],
                "{keyword}"
            );
        }
        assert!(github_closing("See #5 and prefix #6", "o/r").is_empty());
    }
}
