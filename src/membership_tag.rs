use crate::confirmation_tag::ConfirmationTag;
use crate::epoch::EpochKeySchedule;
use crate::framing::MLSPlaintext;
use crate::group::GroupContext;
use crate::message_signature::{MLSPlaintextTBS, MessageSignature};
use ferriscrypt::hmac::{HMacError, Key, Tag};
use ferriscrypt::Signer;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MembershipTagError {
    #[error(transparent)]
    HMacError(#[from] HMacError),
    #[error(transparent)]
    SerializationError(#[from] bincode::Error),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct MLSPlaintextTBM {
    tbs: MLSPlaintextTBS,
    signature: MessageSignature,
    confirmation_tag: Option<ConfirmationTag>,
}

impl MLSPlaintextTBM {
    pub fn from_plaintext(
        plaintext: &MLSPlaintext,
        group_context: &GroupContext,
    ) -> MLSPlaintextTBM {
        MLSPlaintextTBM {
            tbs: MLSPlaintextTBS::from_plaintext(plaintext, group_context),
            signature: plaintext.signature.clone(),
            confirmation_tag: plaintext.confirmation_tag.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MembershipTag(Tag);

impl Deref for MembershipTag {
    type Target = Tag;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Tag> for MembershipTag {
    fn from(m: Tag) -> Self {
        Self(m)
    }
}

impl MembershipTag {
    pub(crate) fn create(
        plaintext: &MLSPlaintext,
        group_context: &GroupContext,
        key_schedule: &EpochKeySchedule,
    ) -> Result<Self, MembershipTagError> {
        let plaintext_tbm = MLSPlaintextTBM::from_plaintext(plaintext, group_context);
        let serialized_tbm = bincode::serialize(&plaintext_tbm)?;

        let hmac_key = Key::new(
            &key_schedule.membership_key,
            key_schedule.cipher_suite.hash_function(),
        )?;

        let tag = hmac_key.sign(&serialized_tbm)?;

        Ok(MembershipTag(tag))
    }

    pub(crate) fn matches(
        &self,
        plaintext: &MLSPlaintext,
        group_context: &GroupContext,
        key_schedule: &EpochKeySchedule,
    ) -> Result<bool, MembershipTagError> {
        let local = MembershipTag::create(plaintext, group_context, key_schedule)?;
        Ok(&local == self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ciphersuite::CipherSuite;
    use crate::epoch::test_utils::get_test_epoch_key_schedule;
    use crate::framing::test_utils::get_test_plaintext;
    use crate::group::test_utils::get_test_group_context;

    #[test]
    fn test_membership_tag_matching() {
        for cipher_suite in CipherSuite::all() {
            let context_a = get_test_group_context(1);
            let context_b = get_test_group_context(2);
            let plaintext_a = get_test_plaintext(b"hello".to_vec());
            let plaintext_b = get_test_plaintext(b"world".to_vec());

            let epoch_a =
                get_test_epoch_key_schedule(cipher_suite, b"membership_key_a".to_vec(), vec![]);

            let epoch_b =
                get_test_epoch_key_schedule(cipher_suite, b"membership_key_b".to_vec(), vec![]);

            let tag = MembershipTag::create(&plaintext_a, &context_a, &epoch_a).unwrap();

            assert!(tag.matches(&plaintext_a, &context_a, &epoch_a).unwrap());

            assert!(!tag.matches(&plaintext_b, &context_a, &epoch_a).unwrap(),);

            assert!(!tag.matches(&plaintext_a, &context_b, &epoch_a).unwrap());

            assert!(!tag.matches(&plaintext_a, &context_a, &epoch_b).unwrap());
        }
    }
}
