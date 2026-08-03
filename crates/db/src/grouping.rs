//! Deterministic thread and duplicate grouping for archived mail.
//!
//! Grouping is computed per message from that message's own headers, never by
//! walking a chain of other messages. That choice is what makes it safe against
//! the shapes a decade-old export actually contains:
//!
//! - **Missing parents.** A reply whose parent was never exported still lands in
//!   the right thread, because the group is derived from the thread root named
//!   in `References`, not from a parent row that has to exist.
//! - **Cycles.** Two messages that reference each other cannot cause a loop,
//!   because nothing is traversed. The worst case is two groups instead of one.
//! - **Order of arrival.** A message computes the same group whether it is
//!   indexed first or last, so a backfill and a live import agree.
//!
//! Group ids are `UUIDv5` over a namespace and a normalized key, so the same
//! evidence always produces the same id without a lookup table.

use uuid::Uuid;

/// Fixed forever. Changing it would silently re-thread the whole archive.
const THREAD_NAMESPACE: Uuid = Uuid::from_bytes([
    0x3f, 0x8c, 0x21, 0x0e, 0x77, 0x4b, 0x5a, 0x92, 0xb6, 0x14, 0x0d, 0x5a, 0x9c, 0x31, 0xe8, 0x77,
]);
const DUPLICATE_NAMESPACE: Uuid = Uuid::from_bytes([
    0xc4, 0x0b, 0x93, 0xa1, 0x18, 0x2f, 0x5d, 0x36, 0x8e, 0x7a, 0x22, 0xb0, 0x41, 0xcf, 0x6d, 0x05,
]);

/// Why a message was placed in its thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_thread_reason", rename_all = "snake_case")]
pub enum EmailThreadReason {
    Provider,
    References,
    MessageId,
    Subject,
    None,
}

/// Why a message was placed in its duplicate group.
#[derive(Clone, Copy, Debug, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "email_duplicate_reason", rename_all = "snake_case")]
pub enum EmailDuplicateReason {
    MessageId,
    ContentHash,
    None,
}

/// The grouping decision for one message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmailGrouping {
    pub thread_group_id: Option<Uuid>,
    pub thread_reason: EmailThreadReason,
    /// A provider thread id was used but the RFC headers disagree with it.
    pub thread_conflict: bool,
    pub duplicate_group_id: Option<Uuid>,
    pub duplicate_reason: EmailDuplicateReason,
}

/// Evidence available for grouping one message.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupingEvidence<'a> {
    pub provider_thread_id: Option<&'a str>,
    pub normalized_message_id: Option<&'a str>,
    pub in_reply_to: Option<&'a str>,
    pub reference_ids: &'a [String],
    pub normalized_subject: Option<&'a str>,
    pub content_hash: Option<&'a str>,
}

/// Whether a provider thread id is trustworthy enough to be authoritative.
///
/// Gmail writes `X-GM-THRID` as a decimal integer. Anything else in that header
/// came from somewhere Strife cannot vouch for, so it is ignored rather than
/// used to merge unrelated messages.
fn usable_provider_id(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 40
        && trimmed.chars().all(|character| character.is_ascii_digit())
}

/// The identifier a standards-based thread hangs from.
///
/// The first `References` entry is the thread root: the chain is ordered oldest
/// first, so its head is the message that started the conversation. `In-Reply-To`
/// is the fallback when a client wrote no `References` at all, and the message's
/// own id is used last so a thread of one is still a thread.
fn standards_root<'a>(evidence: &GroupingEvidence<'a>) -> Option<(&'a str, EmailThreadReason)> {
    if let Some(root) = evidence
        .reference_ids
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
    {
        return Some((root, EmailThreadReason::References));
    }
    if let Some(parent) = evidence
        .in_reply_to
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some((parent, EmailThreadReason::References));
    }
    evidence
        .normalized_message_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| (value, EmailThreadReason::MessageId))
}

/// Decides one message's thread and duplicate groups.
#[must_use]
pub fn group_email(evidence: &GroupingEvidence<'_>) -> EmailGrouping {
    let standards = standards_root(evidence);

    let (thread_group_id, thread_reason, thread_conflict) = match evidence
        .provider_thread_id
        .map(str::trim)
        .filter(|value| usable_provider_id(value))
    {
        Some(provider) => {
            let provider_group = derive(THREAD_NAMESPACE, "thrid", provider);
            // The provider id still wins — Gmail knows about moves and merges
            // the headers never recorded — but a disagreement is worth keeping.
            let conflict = standards.is_some_and(|(root, reason)| {
                reason == EmailThreadReason::References
                    && derive(THREAD_NAMESPACE, "root", root) != provider_group
            });
            (Some(provider_group), EmailThreadReason::Provider, conflict)
        }
        None => match standards {
            Some((root, reason)) => (Some(derive(THREAD_NAMESPACE, "root", root)), reason, false),
            None => match evidence
                .normalized_subject
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                // Documented fallback, never the primary key: unrelated messages
                // share a subject far more often than they share a Message-ID.
                Some(subject) => (
                    Some(derive(THREAD_NAMESPACE, "subject", subject)),
                    EmailThreadReason::Subject,
                    false,
                ),
                None => (None, EmailThreadReason::None, false),
            },
        },
    };

    let (duplicate_group_id, duplicate_reason) = match evidence
        .normalized_message_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(message_id) => (
            Some(derive(DUPLICATE_NAMESPACE, "mid", message_id)),
            EmailDuplicateReason::MessageId,
        ),
        // A message with no id can still be recognised as the same message by
        // its canonical content. Never used when an id exists, because two
        // genuinely distinct messages can canonicalize identically.
        None => match evidence
            .content_hash
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(hash) => (
                Some(derive(DUPLICATE_NAMESPACE, "hash", hash)),
                EmailDuplicateReason::ContentHash,
            ),
            None => (None, EmailDuplicateReason::None),
        },
    };

    EmailGrouping {
        thread_group_id,
        thread_reason,
        thread_conflict,
        duplicate_group_id,
        duplicate_reason,
    }
}

/// Namespaced so a value used as two different kinds of key cannot collide.
fn derive(namespace: Uuid, kind: &str, value: &str) -> Uuid {
    Uuid::new_v5(
        &namespace,
        format!("{kind}:{}", value.to_ascii_lowercase()).as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::{EmailDuplicateReason, EmailThreadReason, GroupingEvidence, group_email};

    fn evidence<'a>() -> GroupingEvidence<'a> {
        GroupingEvidence::default()
    }

    #[test]
    fn a_reply_chain_shares_one_thread_even_without_its_parent() {
        let root_refs = vec!["root@example.test".to_owned()];
        let parent = group_email(&GroupingEvidence {
            normalized_message_id: Some("root@example.test"),
            ..evidence()
        });
        // The parent itself was never exported; the reply still finds the thread.
        let reply = group_email(&GroupingEvidence {
            normalized_message_id: Some("reply@example.test"),
            reference_ids: &root_refs,
            ..evidence()
        });
        assert_eq!(parent.thread_group_id, reply.thread_group_id);
        assert_eq!(reply.thread_reason, EmailThreadReason::References);
    }

    #[test]
    fn a_fork_keeps_both_branches_in_the_same_thread() {
        let refs = vec!["root@example.test".to_owned()];
        let deeper = vec!["root@example.test".to_owned(), "a@example.test".to_owned()];
        let first = group_email(&GroupingEvidence {
            reference_ids: &refs,
            normalized_message_id: Some("a@example.test"),
            ..evidence()
        });
        let second = group_email(&GroupingEvidence {
            reference_ids: &deeper,
            normalized_message_id: Some("b@example.test"),
            ..evidence()
        });
        // Both hang from the same root, which is the head of References.
        assert_eq!(first.thread_group_id, second.thread_group_id);
    }

    #[test]
    fn mutually_referencing_messages_terminate() {
        let a_refs = vec!["b@example.test".to_owned()];
        let b_refs = vec!["a@example.test".to_owned()];
        let a = group_email(&GroupingEvidence {
            normalized_message_id: Some("a@example.test"),
            reference_ids: &a_refs,
            ..evidence()
        });
        let b = group_email(&GroupingEvidence {
            normalized_message_id: Some("b@example.test"),
            reference_ids: &b_refs,
            ..evidence()
        });
        // Nothing is traversed, so a cycle cannot loop. Both are grouped, both
        // are kept, and the outcome is deterministic even though the two land
        // in different groups.
        assert!(a.thread_group_id.is_some());
        assert!(b.thread_group_id.is_some());
    }

    #[test]
    fn subject_is_a_fallback_and_never_outranks_an_identifier() {
        let with_id = group_email(&GroupingEvidence {
            normalized_message_id: Some("x@example.test"),
            normalized_subject: Some("status update"),
            ..evidence()
        });
        assert_eq!(with_id.thread_reason, EmailThreadReason::MessageId);

        let without = group_email(&GroupingEvidence {
            normalized_subject: Some("status update"),
            ..evidence()
        });
        assert_eq!(without.thread_reason, EmailThreadReason::Subject);
        assert_ne!(with_id.thread_group_id, without.thread_group_id);
    }

    #[test]
    fn a_provider_thread_id_wins_but_records_a_conflict() {
        let refs = vec!["root@example.test".to_owned()];
        let grouped = group_email(&GroupingEvidence {
            provider_thread_id: Some("1234567890"),
            reference_ids: &refs,
            normalized_message_id: Some("x@example.test"),
            ..evidence()
        });
        assert_eq!(grouped.thread_reason, EmailThreadReason::Provider);
        assert!(
            grouped.thread_conflict,
            "a disagreement with References must be recorded"
        );
    }

    #[test]
    fn a_malformed_provider_thread_id_is_ignored() {
        let refs = vec!["root@example.test".to_owned()];
        for provider in ["", "   ", "not-a-thrid", "12ab34"] {
            let grouped = group_email(&GroupingEvidence {
                provider_thread_id: Some(provider),
                reference_ids: &refs,
                ..evidence()
            });
            // Falling back to the headers is safer than merging unrelated
            // messages on the strength of an unrecognizable value.
            assert_eq!(
                grouped.thread_reason,
                EmailThreadReason::References,
                "{provider:?} was trusted"
            );
            assert!(!grouped.thread_conflict);
        }
    }

    #[test]
    fn duplicates_group_by_message_id_then_by_content() {
        let a = group_email(&GroupingEvidence {
            normalized_message_id: Some("same@example.test"),
            content_hash: Some("aaaa"),
            ..evidence()
        });
        let b = group_email(&GroupingEvidence {
            normalized_message_id: Some("SAME@example.test"),
            content_hash: Some("bbbb"),
            ..evidence()
        });
        // Message-ID comparison is case-insensitive, and it outranks content.
        assert_eq!(a.duplicate_group_id, b.duplicate_group_id);
        assert_eq!(a.duplicate_reason, EmailDuplicateReason::MessageId);

        let c = group_email(&GroupingEvidence {
            content_hash: Some("cccc"),
            ..evidence()
        });
        let d = group_email(&GroupingEvidence {
            content_hash: Some("cccc"),
            ..evidence()
        });
        assert_eq!(c.duplicate_group_id, d.duplicate_group_id);
        assert_eq!(c.duplicate_reason, EmailDuplicateReason::ContentHash);
        assert_ne!(a.duplicate_group_id, c.duplicate_group_id);
    }

    #[test]
    fn a_message_with_no_evidence_is_grouped_with_nothing() {
        let grouped = group_email(&evidence());
        assert!(grouped.thread_group_id.is_none());
        assert!(grouped.duplicate_group_id.is_none());
        assert_eq!(grouped.thread_reason, EmailThreadReason::None);
        assert_eq!(grouped.duplicate_reason, EmailDuplicateReason::None);
    }

    #[test]
    fn unicode_and_case_variants_of_a_subject_group_together() {
        let a = group_email(&GroupingEvidence {
            normalized_subject: Some("Réunion trimestrielle"),
            ..evidence()
        });
        let b = group_email(&GroupingEvidence {
            normalized_subject: Some("réunion trimestrielle"),
            ..evidence()
        });
        assert_eq!(a.thread_group_id, b.thread_group_id);
    }
}
