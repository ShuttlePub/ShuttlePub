//! The Accept delivery only reaches origins the current settings allow. An
//! inbox is checked when the Follow arrives, but a saved Follow can be
//! delivered after a restart under different `SHUTTLEPUB_REMOTE_ORIGINS`.

mod support;

use std::error::Error;

use driver::{
    community::LocalIdentity, config::Config, keys::DevelopmentKey, remote::RemoteClient,
};
use kernel::{
    KernelError,
    community::LocalActorProvider as _,
    id::{ActivityId, ActorId},
    producer::{Accept, Follow},
};
use port::delivery::ActivityDelivery as _;
use support::peer::{Peer, valid_peer};

type TestResult = Result<(), Box<dyn Error>>;

const LOCAL_ACTOR: &str = "http://127.0.0.1:3000/actor";

fn client(remote_origins: &str) -> Result<(RemoteClient, Accept), Box<dyn Error>> {
    let config = Config::from_lookup(|name: &str| match name {
        "SHUTTLEPUB_REMOTE_ORIGINS" => Some(remote_origins.to_owned()),
        _ => None,
    })
    .map_err(|error| format!("{error:?}"))?;
    let key = DevelopmentKey::generate().map_err(|error| format!("{error:?}"))?;
    let identity =
        LocalIdentity::new(LOCAL_ACTOR.parse()?, &key).map_err(|error| format!("{error:?}"))?;
    let local = identity.local_actor();
    let client = RemoteClient::new(
        config.remote_origins().clone(),
        key,
        local.public_key().id().to_owned(),
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok((client, accept_for(local)?))
}

fn accept_for(local: &kernel::community::LocalActor) -> Result<Accept, Box<dyn Error>> {
    let follow = Follow::new(
        ActivityId::parse("https://remote.example/follows/1").map_err(|e| format!("{e:?}"))?,
        ActorId::parse("https://remote.example/actor").map_err(|e| format!("{e:?}"))?,
        local.id().clone(),
    );
    Ok(Accept::new(local, follow).map_err(|e| format!("{e:?}"))?)
}

fn inbox(peer: &Peer) -> String {
    format!("{}/inbox", peer.origin)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_inbox_on_an_allowed_origin_receives_the_accept() -> TestResult {
    let peer = valid_peer().await?;
    let (client, accept) = client(&peer.origin)?;

    client
        .deliver(&inbox(&peer), &accept)
        .await
        .map_err(|error| format!("{error:?}"))?;

    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_inbox_outside_the_allowed_origins_is_not_contacted() -> TestResult {
    let peer = valid_peer().await?;
    let (client, accept) = client("https://elsewhere.example")?;

    let error = client
        .deliver(&inbox(&peer), &accept)
        .await
        .err()
        .ok_or("the delivery to an origin that is not allowed succeeded")?;

    assert!(matches!(
        error.current_context(),
        KernelError::OriginNotAllowed
    ));
    assert!(peer.deliveries()?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn no_allowed_origins_means_no_delivery_at_all() -> TestResult {
    let peer = valid_peer().await?;
    let (client, accept) = client("")?;

    let error = client
        .deliver(&inbox(&peer), &accept)
        .await
        .err()
        .ok_or("the delivery succeeded with no origin allowed")?;

    assert!(matches!(
        error.current_context(),
        KernelError::OriginNotAllowed
    ));
    assert!(peer.deliveries()?.is_empty());
    Ok(())
}
