#![allow(clippy::nonstandard_macro_braces)]
#![allow(clippy::enum_variant_names)]

#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(test)]
macro_rules! hex {
    ($input:literal) => {
        hex::decode($input).expect("invalid hex value")
    };
}

#[cfg(test)]
macro_rules! load_test_cases {
    ($name:ident, $generate:expr) => {{
        #[cfg(target_arch = "wasm32")]
        {
            let _ = $generate;
            serde_json::from_slice(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/test_data/",
                stringify!($name),
                ".json"
            )))
            .unwrap()
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/test_data/",
                stringify!($name),
                ".json"
            );
            if !std::path::Path::new(path).exists() {
                $generate(path);
            }
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        }
    }};
}

#[macro_use]
pub mod cipher_suite;
pub mod client;
pub mod client_config;
pub mod credential;
pub mod extension;
mod group;
mod hash_reference;
pub mod key_package;
mod protocol_version;
mod psk;
pub mod session;
pub mod signer;
mod tree_kem;
pub mod x509;

#[cfg(feature = "benchmark")]
pub mod tls;

#[cfg(not(feature = "benchmark"))]
mod tls;

pub use ferriscrypt;
pub use group::{
    proposal::{AddProposal, Proposal, RemoveProposal, UpdateProposal},
    GroupContext,
};
pub use protocol_version::ProtocolVersion;
pub use tls_codec;

pub mod time;
