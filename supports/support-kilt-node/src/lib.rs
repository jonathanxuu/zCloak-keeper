use codec::{Decode, Encode};
use primitive_types::H256;
use scale_info::TypeInfo;
use subxt::ClientBuilder;

#[derive(Clone, Copy, Decode, Debug, Encode, Eq, Ord, PartialEq, PartialOrd, TypeInfo)]
pub enum DidEncryptionKey {
	/// An X25519 public key.
	X25519([u8; 32]),
}

#[subxt::subxt(runtime_metadata_path = "kilt_metadata.scale")]
pub mod kilt {
	#[subxt(substitute_type = "did::did_details::DidEncryptionKey")]
	use crate::DidEncryptionKey;
}

const _: () = {
	use kilt::runtime_types::polkadot_parachain::primitives::Id;

	impl PartialEq for Id {
		fn eq(&self, other: &Self) -> bool {
			self.0 == other.0
		}
	}

	impl Eq for Id {}

	impl PartialOrd for Id {
		fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
			self.0.partial_cmp(&other.0)
		}
	}

	impl Ord for Id {
		fn cmp(&self, other: &Self) -> std::cmp::Ordering {
			self.0.cmp(&other.0)
		}
	}
};

pub async fn query_attestation(url: &str, root_hash: H256) -> anyhow::Result<bool> {
	let api = ClientBuilder::new()
		.set_url(url)
		.build()
		.await?
		.to_runtime_api::<kilt::RuntimeApi<kilt::DefaultConfig>>();

	log::info!("------- query attestation ");

	let mut times = 0;
	const MAX_RETRY_TIMES: usize = 5;
	let maybe_attestation_details = loop {
		match api.storage().attestation().attestations(root_hash, None).await {
			Ok(details) => break details,
			Err(e) => {
				match e {
					subxt::Error::Rpc(ref rpc_err) => match rpc_err {
						jsonrpsee_types::Error::RequestTimeout =>
							if times < MAX_RETRY_TIMES {
								times += 1;
								log::warn!(
									"query kilt storage timeout, retry {:}/{:}",
									times,
									MAX_RETRY_TIMES
								);
								continue
							},
						_ => {},
					},
					_ => {},
				}
				return Err(e)?
			},
		}
	};

	// not revoked by kyc agent
	let is_valid = maybe_attestation_details.map_or_else(|| false, |detail| !detail.revoked);

	Ok(is_valid)
}
