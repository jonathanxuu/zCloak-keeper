use std::time::Duration;
use keeper_primitives::{U64, Delay, MESSAGE_PARSE_LOG_TARGET, ConfigInstance, MqSender, MqReceiver, monitor::MonitorSender, JsonParse, EventResult, Error};
use crate::KeeperResult;


// todo: get block number in error return
pub async fn task_verify(
    config: &ConfigInstance,
    msg_queue: (&mut MqSender, &mut MqReceiver),
) -> std::result::Result<(), (Option<U64>, Error)> {
    while let Ok(events) = msg_queue.1.recv_timeout(Delay::new(Duration::from_secs(1))).await
    {
        let events = match events {
            Some(a) => a,
            None => continue,
        };

        // parse event from str to ProofEvent
        let inputs = EventResult::try_from_bytes(&*events);
        let inputs = match inputs {
            Ok(r) => r,
            Err(e) => {
                // log error
                log::error!(
						target: MESSAGE_PARSE_LOG_TARGET,
						"event messages in ipfs component wrongly parsed, {:?}",
						e
					);
                return Err((None, e.into()));
            },
        };

        let res = super::query_and_verify(&config.ipfs_client, inputs).await.map_err(|e| (Some(e.0), e.1))?;
        let status = msg_queue.0.send(serde_json::to_vec(&res).map_err(|e| (None, e.into()))?).await;

        match status {
            Ok(_) => {
                // delete events in channel after the events are successfully
                // transformed and pushed into
                // todo: get block number
                events.commit().map_err(|e| (None, e.into()))?;
            },
            Err(e) => {
                log::error!("in task2 send to queue error:{:?}", e);
                return Err((None, e.into()));
                continue
            },
        }
    }

    Ok(())
}