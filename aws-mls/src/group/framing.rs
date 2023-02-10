use super::proposal::Proposal;
use super::*;
use crate::protocol_version::ProtocolVersion;
use std::io::{Read, Write};
use tls_codec::{Deserialize, Serialize, Size};
use tls_codec_derive::{TlsDeserialize, TlsSerialize, TlsSize};
use zeroize::Zeroize;

#[derive(Copy, Clone, Debug, PartialEq, Eq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u8)]
pub enum ContentType {
    Application = 1,
    Proposal = 2,
    Commit = 3,
}

impl From<&Content> for ContentType {
    fn from(content: &Content) -> Self {
        match content {
            Content::Application(_) => ContentType::Application,
            Content::Proposal(_) => ContentType::Proposal,
            Content::Commit(_) => ContentType::Commit,
        }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    TlsDeserialize,
    TlsSerialize,
    TlsSize,
    serde::Deserialize,
    serde::Serialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u8)]
pub enum Sender {
    #[tls_codec(discriminant = 1)]
    Member(u32),
    External(u32),
    NewMemberCommit,
    NewMemberProposal,
}

impl From<LeafIndex> for Sender {
    fn from(leaf_index: LeafIndex) -> Self {
        Sender::Member(*leaf_index)
    }
}

impl From<u32> for Sender {
    fn from(leaf_index: u32) -> Self {
        Sender::Member(leaf_index)
    }
}

#[derive(Clone, Debug, PartialEq, TlsDeserialize, TlsSerialize, TlsSize, Zeroize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[zeroize(drop)]
pub(crate) struct ApplicationData(#[tls_codec(with = "crate::tls::ByteVec")] Vec<u8>);

impl From<Vec<u8>> for ApplicationData {
    fn from(data: Vec<u8>) -> Self {
        Self(data)
    }
}

impl TryFrom<ApplicationData> for Event {
    type Error = GroupError;

    fn try_from(data: ApplicationData) -> Result<Self, Self::Error> {
        Ok(Event::ApplicationMessage(data.0.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u8)]
pub(crate) enum Content {
    #[tls_codec(discriminant = 1)]
    Application(ApplicationData),
    Proposal(Proposal),
    Commit(Commit),
}

impl Content {
    pub fn content_type(&self) -> ContentType {
        self.into()
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub(crate) struct MLSPlaintext {
    pub content: MLSContent,
    pub auth: MLSContentAuthData,
    pub membership_tag: Option<MembershipTag>,
}

impl Size for MLSPlaintext {
    fn tls_serialized_len(&self) -> usize {
        self.content.tls_serialized_len()
            + self.auth.tls_serialized_len()
            + self
                .membership_tag
                .as_ref()
                .map_or(0, |tag| tag.tls_serialized_len())
    }
}

impl Serialize for MLSPlaintext {
    fn tls_serialize<W: Write>(&self, writer: &mut W) -> Result<usize, tls_codec::Error> {
        Ok(self.content.tls_serialize(writer)?
            + self.auth.tls_serialize(writer)?
            + self
                .membership_tag
                .as_ref()
                .map_or(Ok(0), |tag| tag.tls_serialize(writer))?)
    }
}

impl Deserialize for MLSPlaintext {
    fn tls_deserialize<R: Read>(bytes: &mut R) -> Result<Self, tls_codec::Error> {
        let content = MLSContent::tls_deserialize(bytes)?;
        let auth = MLSContentAuthData::tls_deserialize(bytes, content.content_type())?;

        let membership_tag = match content.sender {
            Sender::Member(_) => Some(MembershipTag::tls_deserialize(bytes)?),
            _ => None,
        };

        Ok(Self {
            content,
            auth,
            membership_tag,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MLSCiphertextContent {
    pub content: Content,
    pub auth: MLSContentAuthData,
    pub padding: Vec<u8>,
}

impl Size for MLSCiphertextContent {
    fn tls_serialized_len(&self) -> usize {
        let content_len_without_type = match &self.content {
            Content::Application(c) => c.tls_serialized_len(),
            Content::Proposal(c) => c.tls_serialized_len(),
            Content::Commit(c) => c.tls_serialized_len(),
        };

        // Padding has arbitrary size
        content_len_without_type + self.auth.tls_serialized_len() + self.padding.len()
    }
}

impl Serialize for MLSCiphertextContent {
    fn tls_serialize<W: Write>(&self, writer: &mut W) -> Result<usize, tls_codec::Error> {
        let len = match &self.content {
            Content::Application(c) => c.tls_serialize(writer),
            Content::Proposal(c) => c.tls_serialize(writer),
            Content::Commit(c) => c.tls_serialize(writer),
        }?;

        // Padding has arbitrary size
        Ok(len + self.auth.tls_serialize(writer)? + writer.write(&self.padding)?)
    }
}

impl MLSCiphertextContent {
    pub(crate) fn tls_deserialize<R: Read>(
        bytes: &mut R,
        content_type: ContentType,
    ) -> Result<Self, tls_codec::Error> {
        let content = match content_type {
            ContentType::Application => {
                Content::Application(ApplicationData::tls_deserialize(bytes)?)
            }
            ContentType::Proposal => Content::Proposal(Proposal::tls_deserialize(bytes)?),
            ContentType::Commit => Content::Commit(Commit::tls_deserialize(bytes)?),
        };

        let auth = MLSContentAuthData::tls_deserialize(bytes, content.content_type())?;

        let mut padding = Vec::new();
        bytes.read_to_end(&mut padding)?;

        if padding.iter().any(|&i| i != 0u8) {
            return Err(tls_codec::Error::DecodingError(
                "non-zero padding bytes discovered".to_string(),
            ));
        }

        Ok(Self {
            content,
            auth,
            padding,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, TlsDeserialize, TlsSerialize, TlsSize)]
pub struct MLSCiphertextContentAAD {
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub group_id: Vec<u8>,
    pub epoch: u64,
    pub content_type: ContentType,
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub authenticated_data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct MLSCiphertext {
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub group_id: Vec<u8>,
    pub epoch: u64,
    pub content_type: ContentType,
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub authenticated_data: Vec<u8>,
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub encrypted_sender_data: Vec<u8>,
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub ciphertext: Vec<u8>,
}

impl From<&MLSCiphertext> for MLSCiphertextContentAAD {
    fn from(ciphertext: &MLSCiphertext) -> Self {
        Self {
            group_id: ciphertext.group_id.clone(),
            epoch: ciphertext.epoch,
            content_type: ciphertext.content_type,
            authenticated_data: ciphertext.authenticated_data.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct MLSMessage {
    pub(crate) version: ProtocolVersion,
    pub(crate) payload: MLSMessagePayload,
}

#[allow(dead_code)]
impl MLSMessage {
    pub(crate) fn new(version: ProtocolVersion, payload: MLSMessagePayload) -> MLSMessage {
        Self { version, payload }
    }

    #[inline(always)]
    pub(crate) fn into_plaintext(self) -> Option<MLSPlaintext> {
        match self.payload {
            MLSMessagePayload::Plain(plaintext) => Some(plaintext),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn into_ciphertext(self) -> Option<MLSCiphertext> {
        match self.payload {
            MLSMessagePayload::Cipher(ciphertext) => Some(ciphertext),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn into_welcome(self) -> Option<Welcome> {
        match self.payload {
            MLSMessagePayload::Welcome(welcome) => Some(welcome),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn into_group_info(self) -> Option<GroupInfo> {
        match self.payload {
            MLSMessagePayload::GroupInfo(info) => Some(info),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn into_key_package(self) -> Option<KeyPackage> {
        match self.payload {
            MLSMessagePayload::KeyPackage(kp) => Some(kp),
            _ => None,
        }
    }

    pub fn wire_format(&self) -> WireFormat {
        match self.payload {
            MLSMessagePayload::Plain(_) => WireFormat::Plain,
            MLSMessagePayload::Cipher(_) => WireFormat::Cipher,
            MLSMessagePayload::Welcome(_) => WireFormat::Welcome,
            MLSMessagePayload::GroupInfo(_) => WireFormat::GroupInfo,
            MLSMessagePayload::KeyPackage(_) => WireFormat::KeyPackage,
        }
    }

    pub fn epoch(&self) -> Option<u64> {
        match &self.payload {
            MLSMessagePayload::Plain(p) => Some(p.content.epoch),
            MLSMessagePayload::Cipher(c) => Some(c.epoch),
            MLSMessagePayload::GroupInfo(gi) => Some(gi.group_context.epoch),
            _ => None,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, tls_codec::Error> {
        Self::tls_deserialize(&mut &*bytes)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, tls_codec::Error> {
        self.tls_serialize_detached()
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[repr(u16)]
pub(crate) enum MLSMessagePayload {
    #[tls_codec(discriminant = 1)]
    Plain(MLSPlaintext),
    Cipher(MLSCiphertext),
    Welcome(Welcome),
    GroupInfo(GroupInfo),
    KeyPackage(KeyPackage),
}

impl From<MLSPlaintext> for MLSMessagePayload {
    fn from(m: MLSPlaintext) -> Self {
        Self::Plain(m)
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, TlsDeserialize, TlsSerialize, TlsSize,
)]
#[repr(u16)]
pub enum WireFormat {
    Plain = 1,
    Cipher,
    Welcome,
    GroupInfo,
    KeyPackage,
}

impl From<ControlEncryptionMode> for WireFormat {
    fn from(mode: ControlEncryptionMode) -> Self {
        match mode {
            ControlEncryptionMode::Plaintext => WireFormat::Plain,
            ControlEncryptionMode::Encrypted(_) => WireFormat::Cipher,
        }
    }
}

#[derive(Clone, Debug, PartialEq, TlsDeserialize, TlsSerialize, TlsSize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub(crate) struct MLSContent {
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub group_id: Vec<u8>,
    pub epoch: u64,
    pub sender: Sender,
    #[tls_codec(with = "crate::tls::ByteVec")]
    pub authenticated_data: Vec<u8>,
    pub content: Content,
}

impl MLSContent {
    pub fn content_type(&self) -> ContentType {
        self.content.content_type()
    }
}

#[cfg(test)]
pub(crate) mod test_utils {

    use crate::group::test_utils::random_bytes;

    use super::*;

    pub(crate) fn get_test_auth_content(test_content: Vec<u8>) -> MLSAuthenticatedContent {
        MLSAuthenticatedContent {
            wire_format: WireFormat::Plain,
            content: MLSContent {
                group_id: Vec::new(),
                epoch: 0,
                sender: Sender::Member(1),
                authenticated_data: Vec::new(),
                content: Content::Application(test_content.into()),
            },
            auth: MLSContentAuthData {
                signature: MessageSignature::empty(),
                confirmation_tag: None,
            },
        }
    }

    pub(crate) fn get_test_ciphertext_content() -> MLSCiphertextContent {
        MLSCiphertextContent {
            content: Content::Application(random_bytes(1024).into()),
            auth: MLSContentAuthData {
                signature: MessageSignature::from(random_bytes(128)),
                confirmation_tag: None,
            },
            padding: vec![],
        }
    }

    impl AsRef<[u8]> for ApplicationData {
        fn as_ref(&self) -> &[u8] {
            &self.0
        }
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use crate::group::framing::test_utils::get_test_ciphertext_content;

    use super::*;

    #[test]
    fn test_mls_ciphertext_content_tls_encoding() {
        let mut ciphertext_content = get_test_ciphertext_content();
        ciphertext_content.padding = vec![0u8; 128];

        let encoded = ciphertext_content.tls_serialize_detached().unwrap();
        let decoded = MLSCiphertextContent::tls_deserialize(
            &mut &*encoded,
            (&ciphertext_content.content).into(),
        )
        .unwrap();

        assert_eq!(ciphertext_content, decoded);
    }

    #[test]
    fn test_mls_ciphertext_content_non_zero_padding_error() {
        let mut ciphertext_content = get_test_ciphertext_content();
        ciphertext_content.padding = vec![1u8; 128];

        let encoded = ciphertext_content.tls_serialize_detached().unwrap();
        let decoded = MLSCiphertextContent::tls_deserialize(
            &mut &*encoded,
            (&ciphertext_content.content).into(),
        );

        assert_matches!(decoded, Err(tls_codec::Error::DecodingError(e)) if e == "non-zero padding bytes discovered");
    }
}
