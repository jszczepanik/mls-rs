use std::convert::Infallible;

use crate::{util::credential_to_chain, CertificateChain, X509IdentityError};
use async_trait::async_trait;
use aws_mls_core::{
    crypto::SignaturePublicKey,
    group::RosterUpdate,
    identity::{CredentialType, IdentityProvider, IdentityWarning},
    time::MlsTime,
};

#[cfg(test)]
use mockall::automock;

#[cfg_attr(test, automock(type Error = crate::test_utils::TestError;))]
/// X.509 certificate unique identity trait.
pub trait X509IdentityExtractor {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Produce a unique value to represent a certificate chain within an MLS
    /// group.
    fn identity(&self, certificate_chain: &CertificateChain) -> Result<Vec<u8>, Self::Error>;

    /// Determine if `successor` is controlled by the same entity as
    /// `predecessor`.
    fn valid_successor(
        &self,
        predecessor: &CertificateChain,
        successor: &CertificateChain,
    ) -> Result<bool, Self::Error>;
}

#[cfg_attr(test, automock(type Error = crate::test_utils::TestError;))]
/// X.509 certificate validation trait.
pub trait X509CredentialValidator {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Validate a certificate chain.
    ///
    /// If `timestamp` is set to `None` then expiration checks should be skipped.
    fn validate_chain(
        &self,
        chain: &CertificateChain,
        timestamp: Option<MlsTime>,
    ) -> Result<SignaturePublicKey, Self::Error>;
}

#[cfg_attr(test, automock(type Error = crate::test_utils::TestError;))]
/// X.509 certificate identity warnings.
pub trait X509WarningProvider {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Produce any custom warnings that are created due to a [`RosterUpdate`]
    fn identity_warnings(&self, update: &RosterUpdate)
        -> Result<Vec<IdentityWarning>, Self::Error>;
}

#[derive(Debug, Clone, Copy, Default)]
/// A warning provider that never produces a warning.
pub struct NoOpWarningProvider;

impl NoOpWarningProvider {
    /// Create a new no-op warning provider.
    pub fn new() -> NoOpWarningProvider {
        NoOpWarningProvider
    }
}

impl X509WarningProvider for NoOpWarningProvider {
    type Error = Infallible;

    fn identity_warnings(
        &self,
        _update: &RosterUpdate,
    ) -> Result<Vec<IdentityWarning>, Self::Error> {
        Ok(vec![])
    }
}

#[derive(Debug)]
#[non_exhaustive]
/// A customizable generic X.509 certificate identity provider.
///
/// This provider forwards its individual [`IdentityProvider`]
/// behavior to its generic sub-components.
///
/// Only X509 credentials are supported by this provider.
pub struct X509IdentityProvider<IE, V, IEP> {
    pub identity_extractor: IE,
    pub validator: V,
    pub warning_provider: IEP,
}

impl<IE, V, IEP> X509IdentityProvider<IE, V, IEP>
where
    IE: X509IdentityExtractor,
    V: X509CredentialValidator,
    IEP: X509WarningProvider,
{
    /// Create a new identity provider.
    pub fn new(identity_extractor: IE, validator: V, warning_provider: IEP) -> Self {
        Self {
            identity_extractor,
            validator,
            warning_provider,
        }
    }

    /// Determine if a certificate is valid based on the behavior of the
    /// underlying validator provided.
    pub fn validate(
        &self,
        signing_identity: &aws_mls_core::identity::SigningIdentity,
        timestamp: Option<aws_mls_core::time::MlsTime>,
    ) -> Result<(), X509IdentityError> {
        let chain = credential_to_chain(&signing_identity.credential)?;

        let leaf_public_key = self
            .validator
            .validate_chain(&chain, timestamp)
            .map_err(|e| X509IdentityError::ChainValidationError(e.into()))?;

        if leaf_public_key != signing_identity.signature_key {
            return Err(X509IdentityError::SignatureKeyMismatch);
        }

        Ok(())
    }

    /// Determine the unique identity of a certificate based on the behavior
    /// of the underlying identity extractor provided.
    pub fn identity(
        &self,
        signing_id: &aws_mls_core::identity::SigningIdentity,
    ) -> Result<Vec<u8>, X509IdentityError> {
        self.identity_extractor
            .identity(&credential_to_chain(&signing_id.credential)?)
            .map_err(|e| X509IdentityError::IdentityExtractorError(e.into()))
    }

    /// Determine if `successor` is controlled by the same entity as
    /// `predecessor` based on the behavior of the underlying identity
    /// extractor provided.
    pub fn valid_successor(
        &self,
        predecessor: &aws_mls_core::identity::SigningIdentity,
        successor: &aws_mls_core::identity::SigningIdentity,
    ) -> Result<bool, X509IdentityError> {
        self.identity_extractor
            .valid_successor(
                &credential_to_chain(&predecessor.credential)?,
                &credential_to_chain(&successor.credential)?,
            )
            .map_err(|e| X509IdentityError::IdentityExtractorError(e.into()))
    }

    /// Supported credential types.
    ///
    /// Only [`CredentialType::X509`] is supported.
    pub fn supported_types(&self) -> Vec<aws_mls_core::identity::CredentialType> {
        vec![CredentialType::X509]
    }

    /// Gather identity warnings based on the rules provided by the underlying
    /// warning provider.
    pub fn identity_warnings(
        &self,
        update: &aws_mls_core::group::RosterUpdate,
    ) -> Result<Vec<IdentityWarning>, X509IdentityError> {
        self.warning_provider
            .identity_warnings(update)
            .map_err(|e| X509IdentityError::IdentityEventProviderError(e.into()))
    }
}

#[async_trait]
impl<IE, V, IEP> IdentityProvider for X509IdentityProvider<IE, V, IEP>
where
    IE: X509IdentityExtractor + Send + Sync,
    V: X509CredentialValidator + Send + Sync,
    IEP: X509WarningProvider + Send + Sync,
{
    type Error = X509IdentityError;

    async fn validate(
        &self,
        signing_identity: &aws_mls_core::identity::SigningIdentity,
        timestamp: Option<MlsTime>,
    ) -> Result<(), Self::Error> {
        self.validate(signing_identity, timestamp)
    }

    async fn identity(
        &self,
        signing_id: &aws_mls_core::identity::SigningIdentity,
    ) -> Result<Vec<u8>, Self::Error> {
        self.identity(signing_id)
    }

    async fn valid_successor(
        &self,
        predecessor: &aws_mls_core::identity::SigningIdentity,
        successor: &aws_mls_core::identity::SigningIdentity,
    ) -> Result<bool, Self::Error> {
        self.valid_successor(predecessor, successor)
    }

    fn supported_types(&self) -> Vec<CredentialType> {
        self.supported_types()
    }

    async fn identity_warnings(
        &self,
        update: &aws_mls_core::group::RosterUpdate,
    ) -> Result<Vec<IdentityWarning>, Self::Error> {
        self.identity_warnings(update)
    }
}

#[cfg(test)]
mod tests {
    use aws_mls_core::{crypto::SignaturePublicKey, identity::CredentialType, time::MlsTime};

    use crate::{
        test_utils::{
            test_certificate_chain, test_signing_identity, test_signing_identity_with_chain,
            TestError,
        },
        MockX509CredentialValidator, MockX509IdentityExtractor, MockX509WarningProvider,
        X509IdentityError, X509IdentityProvider,
    };

    use assert_matches::assert_matches;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn test_setup<F>(
        mut mock_setup: F,
    ) -> X509IdentityProvider<
        MockX509IdentityExtractor,
        MockX509CredentialValidator,
        MockX509WarningProvider,
    >
    where
        F: FnMut(
            &mut MockX509IdentityExtractor,
            &mut MockX509CredentialValidator,
            &mut MockX509WarningProvider,
        ),
    {
        let mut identity_extractor = MockX509IdentityExtractor::new();
        let mut validator = MockX509CredentialValidator::new();
        let mut event_provider = MockX509WarningProvider::new();

        mock_setup(&mut identity_extractor, &mut validator, &mut event_provider);

        X509IdentityProvider::new(identity_extractor, validator, event_provider)
    }

    #[test]
    fn test_supported_types() {
        let test_provider = test_setup(|_, _, _| ());

        assert_eq!(
            test_provider.supported_types(),
            vec![CredentialType::new(2)]
        )
    }

    #[test]
    fn test_successful_validation() {
        let chain = test_certificate_chain();

        let test_signing_identity = test_signing_identity_with_chain(chain.clone());

        let test_timestamp = MlsTime::now();

        let test_provider = test_setup(|_, validator, _| {
            let validation_result = test_signing_identity.signature_key.clone();

            validator
                .expect_validate_chain()
                .once()
                .with(
                    mockall::predicate::eq(chain.clone()),
                    mockall::predicate::eq(Some(test_timestamp)),
                )
                .return_once_st(|_, _| Ok(validation_result));
        });

        test_provider
            .validate(&test_signing_identity, Some(test_timestamp))
            .unwrap();
    }

    #[test]
    fn test_signing_identity_key_mismatch() {
        let test_signing_identity = test_signing_identity();

        let test_provider = test_setup(|_, validator, _| {
            let validation_result = SignaturePublicKey::from(vec![42u8; 32]);

            validator
                .expect_validate_chain()
                .return_once_st(|_, _| Ok(validation_result));
        });

        assert_matches!(
            test_provider.validate(&test_signing_identity, None),
            Err(X509IdentityError::SignatureKeyMismatch)
        );
    }

    #[test]
    fn test_failing_validation() {
        let test_provider = test_setup(|_, validator, _| {
            validator
                .expect_validate_chain()
                .return_once_st(|_, _| Err(TestError));
        });

        assert_matches!(
            test_provider.validate(&test_signing_identity(), None),
            Err(X509IdentityError::ChainValidationError(_))
        )
    }
}