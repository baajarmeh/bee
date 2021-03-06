// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::{
    balance::BalanceDiffs,
    conflict::ConflictReason,
    dust::{dust_outputs_max, DUST_THRESHOLD},
    error::Error,
    metadata::WhiteFlagMetadata,
    storage::{self, StorageBackend},
};

use bee_message::{
    input::Input,
    output::{ConsumedOutput, CreatedOutput, Output, OutputId},
    payload::{
        transaction::{Essence, TransactionPayload},
        Payload,
    },
    unlock::UnlockBlock,
    Message, MessageId,
};
use bee_runtime::node::Node;
use bee_tangle::MsTangle;

use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

async fn validate_transaction<B: StorageBackend>(
    storage: &B,
    transaction: &TransactionPayload,
    metadata: &mut WhiteFlagMetadata,
    consumed_outputs: &mut HashMap<OutputId, CreatedOutput>,
    balance_diffs: &mut BalanceDiffs,
) -> Result<ConflictReason, Error> {
    let mut created_amount: u64 = 0;
    let mut consumed_amount: u64 = 0;

    // TODO saturating ? Overflowing ? Checked ?

    // TODO
    if let Essence::Regular(essence) = transaction.essence() {
        let essence_hash = transaction.essence().hash();

        for (index, input) in essence.inputs().iter().enumerate() {
            let (output_id, consumed_output) = if let Input::UTXO(utxo_input) = input {
                let output_id = utxo_input.output_id();

                if metadata.consumed_outputs.contains_key(output_id) {
                    return Ok(ConflictReason::InputUTXOAlreadySpentInThisMilestone);
                }

                if let Some(output) = metadata.created_outputs.get(output_id).cloned() {
                    (output_id, output)
                } else if let Some(output) = storage::fetch_output(storage.deref(), output_id).await? {
                    if !storage::is_output_unspent(storage.deref(), output_id).await? {
                        return Ok(ConflictReason::InputUTXOAlreadySpent);
                    }
                    (output_id, output)
                } else {
                    return Ok(ConflictReason::InputUTXONotFound);
                }
            } else {
                return Err(Error::UnsupportedInputType);
            };

            match consumed_output.inner() {
                Output::SignatureLockedSingle(consumed_output) => {
                    consumed_amount = consumed_amount.saturating_add(consumed_output.amount());
                    balance_diffs.amount_sub(*consumed_output.address(), consumed_output.amount());
                    if consumed_output.amount() < DUST_THRESHOLD {
                        balance_diffs.dust_output_dec(*consumed_output.address());
                    }
                    if !match transaction.unlock_blocks().get(index) {
                        Some(UnlockBlock::Signature(signature)) => {
                            consumed_output.address().verify(&essence_hash, signature)
                        }
                        _ => false,
                    } {
                        return Ok(ConflictReason::InvalidSignature);
                    }
                }
                Output::SignatureLockedDustAllowance(consumed_output) => {
                    consumed_amount = consumed_amount.saturating_add(consumed_output.amount());
                    balance_diffs.amount_sub(*consumed_output.address(), consumed_output.amount());
                    balance_diffs.dust_allowance_sub(*consumed_output.address(), consumed_output.amount());
                    if !match transaction.unlock_blocks().get(index) {
                        Some(UnlockBlock::Signature(signature)) => {
                            consumed_output.address().verify(&essence_hash, signature)
                        }
                        _ => false,
                    } {
                        return Ok(ConflictReason::InvalidSignature);
                    }
                }
                _ => return Err(Error::UnsupportedOutputType),
            }

            consumed_outputs.insert(*output_id, consumed_output);
        }

        for created_output in essence.outputs() {
            match created_output {
                Output::SignatureLockedSingle(created_output) => {
                    created_amount = created_amount.saturating_add(created_output.amount());
                    balance_diffs.amount_add(*created_output.address(), created_output.amount());
                    if created_output.amount() < DUST_THRESHOLD {
                        balance_diffs.dust_output_inc(*created_output.address());
                    }
                }
                Output::SignatureLockedDustAllowance(created_output) => {
                    created_amount = created_amount.saturating_add(created_output.amount());
                    balance_diffs.amount_add(*created_output.address(), created_output.amount());
                    balance_diffs.dust_allowance_add(*created_output.address(), created_output.amount());
                }
                _ => return Err(Error::UnsupportedOutputType),
            }
        }

        if created_amount != consumed_amount {
            return Ok(ConflictReason::InputOutputSumMismatch);
        }
    }

    Ok(ConflictReason::None)
}

#[inline]
async fn on_message<B: StorageBackend>(
    storage: &B,
    message_id: &MessageId,
    message: &Message,
    metadata: &mut WhiteFlagMetadata,
) -> Result<(), Error> {
    metadata.num_referenced_messages += 1;

    let transaction = if let Some(Payload::Transaction(transaction)) = message.payload() {
        transaction
    } else {
        metadata.excluded_no_transaction_messages.push(*message_id);
        return Ok(());
    };

    let transaction_id = transaction.id();
    let essence = transaction.essence();
    let mut consumed_outputs = HashMap::new();
    // TODO bring back
    // let mut consumed_outputs = HashMap::with_capacity(essence.inputs().len());
    let mut balance_diffs = BalanceDiffs::new();

    let conflict = validate_transaction(
        storage,
        &transaction,
        metadata,
        &mut consumed_outputs,
        &mut balance_diffs,
    )
    .await?;

    if conflict != ConflictReason::None {
        metadata.excluded_conflicting_messages.push((*message_id, conflict));
        return Ok(());
    }

    for (address, diff) in balance_diffs.iter() {
        if diff.is_dust_mutating() {
            let mut balance = storage::fetch_balance_or_default(storage.deref(), &address).await?;

            if let Some(diff) = metadata.balance_diffs.get(&address) {
                balance = balance + diff;
            }

            balance = balance + diff;

            if balance.dust_output() as usize > dust_outputs_max(balance.dust_allowance()) {
                metadata
                    .excluded_conflicting_messages
                    .push((*message_id, ConflictReason::InvalidDustAllowance));
                return Ok(());
            }
        }
    }

    metadata.balance_diffs.merge(balance_diffs);

    // TODO
    if let Essence::Regular(essence) = essence {
        for (index, output) in essence.outputs().iter().enumerate() {
            metadata.created_outputs.insert(
                // Unwrap is fine, the index is known to be valid.
                OutputId::new(transaction_id, index as u16).unwrap(),
                CreatedOutput::new(*message_id, output.clone()),
            );
        }
    }

    // TODO output ?
    for (output_id, _) in consumed_outputs {
        metadata
            .consumed_outputs
            .insert(output_id, ConsumedOutput::new(transaction_id, metadata.index));
    }

    metadata.included_messages.push(*message_id);

    Ok(())
}

// TODO make it a tangle method ?
pub(crate) async fn traversal<N: Node>(
    tangle: &MsTangle<N::Backend>,
    storage: &N::Backend,
    mut messages_ids: Vec<MessageId>,
    metadata: &mut WhiteFlagMetadata,
) -> Result<(), Error>
where
    N::Backend: StorageBackend,
{
    let mut visited = HashSet::new();
    messages_ids = messages_ids.into_iter().rev().collect();

    // TODO Tangle get message AND meta at the same time

    while let Some(message_id) = messages_ids.last() {
        let meta = match tangle.get_metadata(message_id).await {
            Some(meta) => meta,
            None => {
                if !tangle.is_solid_entry_point(message_id).await {
                    return Err(Error::MissingMessage(*message_id));
                } else {
                    visited.insert(*message_id);
                    messages_ids.pop();
                    continue;
                }
            }
        };

        if meta.flags().is_confirmed() {
            visited.insert(*message_id);
            messages_ids.pop();
            continue;
        }

        match tangle.get(message_id).await {
            Some(message) => {
                let mut next = None;

                for parent in message.parents() {
                    if !visited.contains(parent) {
                        next.replace(parent);
                        break;
                    }
                }

                match next {
                    Some(next) => messages_ids.push(*next),
                    None => {
                        on_message(storage, message_id, &message, metadata).await?;
                        visited.insert(*message_id);
                        messages_ids.pop();
                    }
                }
            }
            None => {
                if !tangle.is_solid_entry_point(message_id).await {
                    return Err(Error::MissingMessage(*message_id));
                } else {
                    visited.insert(*message_id);
                    messages_ids.pop();
                }
            }
        }
    }

    Ok(())
}
