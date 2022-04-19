use std::collections::BTreeMap;

use secp256k1::SecretKey;
use web3::signing::{Key, SecretKeyRef};

use keeper_primitives::{
	moonbeam::{
		self, ProofEvent, Events, IS_FINISHED, MOONBEAM_LISTENED_EVENT, MOONBEAM_LOG_TARGET,
		MOONBEAM_SCAN_SPAN, MOONBEAM_TRANSACTION_CONFIRMATIONS, SUBMIT_STATUS_QUERY,
		SUBMIT_VERIFICATION,
	},
	Address, Contract, Http, MoonbeamClient, Result as KeeperResult, VerifyResult, Web3Options,
	U64,
};

pub use task::{task_scan, task_submit};
mod task;

// scan moonbeam events
pub async fn scan_events(
	mut start: U64,
	best: U64,
	client: &MoonbeamClient,
	proof_contract: &Contract<Http>,
) -> KeeperResult<(Option<Events>, U64)> {
	// if start > best, reset `start` pointer to best
	if start > best {
		log::warn!(
			target: MOONBEAM_LOG_TARGET,
			"scan moonbeam start block is higher than current best! start_block={}, best_block:{}",
			start,
			best
		);
		start = best;
	}
	let span: U64 = MOONBEAM_SCAN_SPAN.into();
	let end = if start + span > best { best } else { start + span };

	log::info!(
		target: MOONBEAM_LOG_TARGET,
		"try to scan moonbeam log from block [{:}] - [{:}] | best:{:}",
		start,
		end,
		best
	);
	// parse event
	let r = moonbeam::utils::events::<_, ProofEvent>(
		client.eth(),
		proof_contract,
		MOONBEAM_LISTENED_EVENT,
		Some(start),
		Some(end),
	)
	.await;

	// if event parse error, return Err(start) and output error log
	let res = match r {
		Ok(events) => events,
		Err(err) => {
			log::error!(
				target: MOONBEAM_LOG_TARGET,
				"Moonbeam Scan Err: Event parse error. {:?}",
				err
			);
			return Err((Some(start), err.into()))
		},
	};

	let hit = res.len();

	if hit != 0 {
		let mut result = vec![];
		for (mut proof_event, log) in res {
			let number = log.block_number;
			// warn
			if number.is_none() {
				log::warn!(
					target: MOONBEAM_LOG_TARGET,
					"Moonbeam log blocknumber should not be None"
				);
			}
			// complete proof event
			proof_event.set_block_number(number);

			result.push(proof_event.clone());
			log::info!(
				target: MOONBEAM_LOG_TARGET,
				"event in block {:?} contains data owner: {:} | request hash: {:} | root hash: {:} | program hash is {:} | calc output {:?} have been recorded",
				number,
				hex::encode(proof_event.data_owner()),
				hex::encode(proof_event.request_hash()),
				hex::encode(proof_event.root_hash()),
				hex::encode(proof_event.program_hash()),
				proof_event.raw_outputs()
			);

			log::info!(
				"scan from [{:}] - [{:}] | hit:[{:}] | in blocks: {:?}",
				start,
				end,
				hit,
				number
			);
		}

		Ok((Some(result), end))
	} else {
		Ok((None, end))
	}
}

pub async fn submit_txs(
	contract: &Contract<Http>,
	keeper_pri: SecretKey,
	keeper_address: Address,
	res: Vec<VerifyResult>,
) -> std::result::Result<(), (Option<U64>, keeper_primitives::moonbeam::Error)> {
	for v in res {
		log::info!(target: MOONBEAM_LOG_TARGET, "IsPassed before submit is {}", v.is_passed);
		// TODO: read multiple times?
		let has_submitted: bool = contract
			.query(
				SUBMIT_STATUS_QUERY,
				(keeper_address, v.data_owner, v.request_hash),
				None,
				Web3Options::default(),
				None,
			)
			.await.map_err(|e| (v.number, e.into()))?;

		let is_finished: bool = contract
			.query(IS_FINISHED, (v.data_owner, v.request_hash), None, Web3Options::default(), None)
			.await.map_err(|e| (v.number, e.into()))?;

		log::info!(
			target: MOONBEAM_LOG_TARGET,
			"hasSubmitted result for request hash [{:?}] is {}, isFinished result is {:}",
			hex::encode(v.request_hash),
			has_submitted,
			is_finished
		);

		if !has_submitted && !is_finished {
			let r = contract
				.signed_call_with_confirmations(
					SUBMIT_VERIFICATION,
					(
						v.data_owner,
						v.request_hash,
						v.c_type,
						v.root_hash,
						v.is_passed,
						v.attester,
						v.calc_output,
					),
					{
						// todo: auto adjust options here
						let mut options = Web3Options::default();
						options.gas = Some(1000000.into());
						options
					},
					MOONBEAM_TRANSACTION_CONFIRMATIONS,
					&keeper_pri,
				)
				.await;

			match r {
				Ok(r) => {
					log::info!(
						target: MOONBEAM_LOG_TARGET,
						"Successfully submit verification|tx:{:}|data owner:{:}|root_hash:{:}|is_passed: {:}|attester: {:}",
						r.transaction_hash,
						v.data_owner,
						hex::encode(v.root_hash),
						v.is_passed,
						hex::encode(v.attester),
					)
				},
				Err(e) => {
					log::error!(
						target: MOONBEAM_LOG_TARGET,
						"Error submit verification |data owner:{:}|root_hash:{:}|request_hash: {:}, err: {:?}",
						v.data_owner,
						hex::encode(v.root_hash),
						hex::encode(v.request_hash),
						e
					);
					return Err((v.number, e.into()))
				},
			}
		}
	}

	Ok(())
}
