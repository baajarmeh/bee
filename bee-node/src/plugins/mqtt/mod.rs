// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

mod config;
mod manager;
mod topics;

use manager::MqttManager;
use topics::*;

use bee_common::shutdown_stream::ShutdownStream;
use bee_common_pt2::{event::Bus, node::Node, worker::Worker};
use bee_protocol::event::{LatestMilestoneChanged, LatestSolidMilestoneChanged, MpsMetricsUpdated};

use async_trait::async_trait;
use futures::stream::StreamExt;
use log::{debug, error, warn};
use tokio::sync::mpsc;

use std::{any::Any, convert::Infallible};

#[derive(Default)]
pub struct Mqtt;

fn topic_handler<N, E, P, F>(node: &mut N, topic: &'static str, f: F)
where
    N: Node,
    E: Any + Clone + Send,
    P: Into<Vec<u8>>,
    F: 'static + Fn(&E) -> P + Send,
{
    let bus = node.resource::<Bus>();
    let manager = node.resource::<MqttManager>();
    let (tx, rx) = mpsc::unbounded_channel();

    node.spawn::<Mqtt, _, _>(|shutdown| async move {
        debug!("Mqtt {} topic handler running.", topic);

        let mut receiver = ShutdownStream::new(shutdown, rx);

        while let Some(event) = receiver.next().await {
            manager.send(topic, f(&event));
        }

        debug!("Mqtt {} topic handler stopped.", topic);
    });

    bus.add_listener::<Mqtt, _, _>(move |event: &E| {
        if let Err(_) = tx.send((*event).clone()) {
            warn!("Sending event to mqtt {} topic handler failed.", topic)
        }
    });
}

#[async_trait]
impl<N: Node> Worker<N> for Mqtt {
    type Config = ();
    type Error = Infallible;

    async fn start(node: &mut N, _config: Self::Config) -> Result<Self, Self::Error> {
        // TODO conf
        match MqttManager::new(&config::MqttConfigBuilder::new().finish()) {
            Ok(manager) => {
                // TODO log connected
                node.register_resource(manager);

                topic_handler(node, "bee", |_event: &MpsMetricsUpdated| "TEST");
                topic_handler(node, TOPIC_MILESTONES_LATEST, |_event: &LatestMilestoneChanged| "");
                topic_handler(node, TOPIC_MILESTONES_SOLID, |_event: &LatestSolidMilestoneChanged| "");
                // topic_handler(node, _TOPIC_MESSAGES, |_event: &_| {});
                // topic_handler(node, _TOPIC_MESSAGES_REFERENCED, |_event: &_| {});
                // topic_handler(node, _TOPIC_MESSAGES_INDEXATION, |_event: &_| {});
                // topic_handler(node, _TOPIC_MESSAGES_METADATA, |_event: &_| {});
                // topic_handler(node, _TOPIC_OUTPUTS, |_event: &_| {});
                // topic_handler(node, _TOPIC_ADDRESSES_OUTPUTS, |_event: &_| {});
                // topic_handler(node, _TOPIC_ADDRESSES_ED25519_OUTPUT, |_event: &_| {});
            }
            Err(e) => {
                error!("Creating mqtt manager failed {:?}.", e);
            }
        }

        Ok(Self::default())
    }

    async fn stop(self, _node: &mut N) -> Result<(), Self::Error> {
        // if let Some(client) = node.remove_resource::<MqttManager>() {
        //     client.disconnect(None).wait().unwrap();
        // }

        Ok(())
    }
}