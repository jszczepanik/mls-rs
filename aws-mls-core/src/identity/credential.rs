use std::ops::Deref;

use serde_with::serde_as;
use tls_codec_derive::{TlsDeserialize, TlsSerialize, TlsSize};

use crate::tls::{ByteVec, DefVec};

use super::{BasicCredential, CertificateChain};

#[derive(
    Debug,
    PartialEq,
    Eq,
    Hash,
    Clone,
    Copy,
    TlsSize,
    TlsSerialize,
    TlsDeserialize,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
/// Wrapper type representing a credential type identifier along with default
/// values defined by the MLS RFC.
pub struct CredentialType(u16);

impl CredentialType {
    /// Basic identity.
    pub const BASIC: CredentialType = CredentialType(1);
    /// X509 Certificate Identity.
    pub const X509: CredentialType = CredentialType(2);

    pub const fn new(raw_value: u16) -> Self {
        CredentialType(raw_value)
    }

    pub const fn raw_value(&self) -> u16 {
        self.0
    }
}

impl From<u16> for CredentialType {
    fn from(value: u16) -> Self {
        CredentialType(value)
    }
}

impl Deref for CredentialType {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[serde_as]
#[derive(
    Clone,
    Debug,
    TlsDeserialize,
    TlsSerialize,
    TlsSize,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
/// Custom user created credential type.
///
/// # Warning
///
/// In order to use a custom credential within an MLS group, a supporting
/// [`IdentityProvider`](crate::IdentityProvider) must be created that can
/// authenticate the credential.
pub struct CustomCredential {
    pub(crate) credential_type: CredentialType,
    #[tls_codec(with = "crate::tls::ByteVec")]
    #[serde_as(as = "crate::serde::vec_u8_as_base64::VecAsBase64")]
    pub(crate) data: Vec<u8>,
}

impl CustomCredential {
    /// Create a new custom credential with opaque data.
    ///
    /// # Warning
    ///
    /// Using any of the constants defined within [`CredentialType`] will
    /// result in unspecified behavior.
    pub fn new(credential_type: CredentialType, data: Vec<u8>) -> CustomCredential {
        CustomCredential {
            credential_type,
            data,
        }
    }

    /// Unique credential type to identify this custom credential.
    pub fn credential_type(&self) -> CredentialType {
        self.credential_type
    }

    /// Opaque data representing this custom credential.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
/// A MLS credential used to authenticate a group member.
pub enum Credential {
    /// Basic identifier-only credential.
    ///
    /// # Warning
    ///
    /// Basic credentials are inherently insecure since they can not be
    /// properly validated. It is not recommended to use [`BasicCredential`]
    /// in production applications.
    Basic(BasicCredential),
    /// X.509 Certificate chain.
    X509(CertificateChain),
    /// User provided custom credential.
    Custom(CustomCredential),
}

impl Credential {
    /// Credential type of the underlying credential.
    pub fn credential_type(&self) -> CredentialType {
        match self {
            Credential::Basic(_) => CredentialType::BASIC,
            Credential::X509(_) => CredentialType::X509,
            Credential::Custom(c) => c.credential_type,
        }
    }

    /// Convert this enum into a [`BasicCredential`]
    ///
    /// Returns `None` if this credential is any other type.
    pub fn as_basic(&self) -> Option<&BasicCredential> {
        match self {
            Credential::Basic(basic) => Some(basic),
            _ => None,
        }
    }

    /// Convert this enum into a [`CertificateChain`]
    ///
    /// Returns `None` if this credential is any other type.
    pub fn as_x509(&self) -> Option<&CertificateChain> {
        match self {
            Credential::X509(chain) => Some(chain),
            _ => None,
        }
    }

    /// Convert this enum into a [`CustomCredential`]
    ///
    /// Returns `None` if this credential is any other type.
    pub fn as_custom(&self) -> Option<&CustomCredential> {
        match self {
            Credential::Custom(custom) => Some(custom),
            _ => None,
        }
    }
}

impl tls_codec::Size for Credential {
    fn tls_serialized_len(&self) -> usize {
        let inner_len = match self {
            Credential::Basic(c) => c.tls_serialized_len(),
            Credential::X509(c) => DefVec::tls_serialized_len(c),
            Credential::Custom(c) => ByteVec::tls_serialized_len(&c.data),
        };

        self.credential_type().tls_serialized_len() + inner_len
    }
}

impl tls_codec::Serialize for Credential {
    fn tls_serialize<W: std::io::Write>(&self, writer: &mut W) -> Result<usize, tls_codec::Error> {
        let type_len = self.credential_type().tls_serialize(writer)?;

        let inner_len = match self {
            Credential::Basic(c) => c.tls_serialize(writer),
            Credential::X509(c) => DefVec::tls_serialize(c, writer),
            Credential::Custom(c) => {
                if c.credential_type.raw_value() <= 2 {
                    return Err(tls_codec::Error::EncodingError(
                        "custom credential types can not be set to defined values of 0-2"
                            .to_string(),
                    ));
                }

                ByteVec::tls_serialize(&c.data, writer)
            }
        }?;

        Ok(type_len + inner_len)
    }
}

impl tls_codec::Deserialize for Credential {
    fn tls_deserialize<R: std::io::Read>(bytes: &mut R) -> Result<Self, tls_codec::Error>
    where
        Self: Sized,
    {
        let credential_type = CredentialType::tls_deserialize(bytes)?;

        Ok(match credential_type {
            CredentialType::BASIC => Credential::Basic(BasicCredential::tls_deserialize(bytes)?),
            CredentialType::X509 => Credential::X509(CertificateChain::tls_deserialize(bytes)?),
            custom => Credential::Custom(CustomCredential {
                credential_type: custom,
                data: ByteVec::tls_deserialize(bytes)?,
            }),
        })
    }
}

/// Trait that provides a conversion between an underlying credential type and
/// the [`Credential`] enum.
pub trait MlsCredential: Sized {
    /// Conversion error type.
    type Error;

    /// Credential type represented by this type.
    fn credential_type() -> CredentialType;

    /// Function to convert this type into a [`Credential`] enum.
    fn into_credential(self) -> Result<Credential, Self::Error>;
}