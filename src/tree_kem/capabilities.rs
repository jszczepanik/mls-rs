use tls_codec_derive::{TlsDeserialize, TlsSerialize, TlsSize};

use crate::{
    cipher_suite::{CipherSuite, MaybeCipherSuite},
    credential::{CredentialType, CREDENTIAL_TYPE_BASIC, CREDENTIAL_TYPE_X509},
    extension::ExtensionType,
    group::proposal::ProposalType,
    ProtocolVersion,
};

#[derive(
    Clone,
    PartialEq,
    Debug,
    TlsDeserialize,
    TlsSerialize,
    TlsSize,
    serde::Deserialize,
    serde::Serialize,
)]
pub struct Capabilities {
    #[tls_codec(with = "crate::tls::DefVec")]
    pub protocol_versions: Vec<ProtocolVersion>,
    #[tls_codec(with = "crate::tls::DefVec")]
    pub cipher_suites: Vec<MaybeCipherSuite>,
    #[tls_codec(with = "crate::tls::DefVec")]
    pub extensions: Vec<ExtensionType>,
    #[tls_codec(with = "crate::tls::DefVec")]
    pub proposals: Vec<ProposalType>,
    #[tls_codec(with = "crate::tls::DefVec")]
    pub credentials: Vec<CredentialType>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            protocol_versions: vec![ProtocolVersion::Mls10],
            cipher_suites: CipherSuite::all().map(MaybeCipherSuite::from).collect(),
            extensions: Default::default(),
            proposals: Default::default(),
            credentials: vec![CREDENTIAL_TYPE_BASIC, CREDENTIAL_TYPE_X509],
        }
    }
}