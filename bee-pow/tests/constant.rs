// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use bee_pow::{
    providers::{ConstantBuilder, MinerBuilder, Provider, ProviderBuilder},
    score::compute_pow_score,
};
use bee_test::rand::bytes::rand_bytes;

#[test]
fn constant() {
    let miner = MinerBuilder::new().with_num_workers(4).finish();
    let mut bytes = rand_bytes(256);
    let constant = ConstantBuilder::new()
        .with_value(miner.nonce(&bytes[0..248], 4000f64, None).unwrap())
        .finish();
    let nonce = constant.nonce(&bytes[0..248], 4000f64, None).unwrap();
    bytes[248..].copy_from_slice(&nonce.to_le_bytes());

    assert!(compute_pow_score(&bytes) >= 4000f64);
}
