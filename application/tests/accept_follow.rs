//! `AcceptFollowService` with every port replaced by an in-memory fake: no
//! HTTP server and no network. The fakes record their calls so that the tests
//! can see which ports a rejected Follow reached.

use std::{
    error::Error,
    sync::{Mutex, PoisonError},
};

use application::{
    ApplicationError,
    services::AcceptFollowService,
    transfer::{IdentifiedFollow, InboundFollow},
};
use error_stack::Report;
use kernel::{
    KernelError,
    entities::{Accept, ActorId, LocalActor, PublicKey, RemoteActor},
    interface::{
        ActivityDelivery, DependOnActivityDelivery, DependOnLocalActorProvider,
        DependOnRemoteActorResolver, LocalActorProvider, RemoteActorResolver,
    },
};
use url::Url;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
type Edit = fn(&mut InboundFollow);

const LOCAL: &str = "https://local.example:8443/actor";
const LOCAL_HOST: &str = "local.example:8443";
const FOLLOWER: &str = "https://remote.example/actor";
const FOLLOWER_INBOX: &str = "https://remote.example/inbox";
const FOLLOWER_KEY: &str = "https://remote.example/actor#main-key";
const FOLLOW_ID: &str = "https://remote.example/follows/1";

fn unexpected<C: Error + Send + Sync + 'static>(report: Report<C>) -> Box<dyn Error> {
    format!("{report:?}").into()
}

struct FakeLocal(LocalActor);

impl LocalActorProvider for FakeLocal {
    fn local_actor(&self) -> &LocalActor {
        &self.0
    }
}

struct FakeResolver {
    outcome: Result<RemoteActor, KernelError>,
    requested: Mutex<Vec<String>>,
}

impl RemoteActorResolver for FakeResolver {
    async fn resolve(&self, id: &ActorId) -> Result<RemoteActor, Report<KernelError>> {
        self.requested
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id.as_str().to_owned());
        self.outcome.clone().map_err(Report::new)
    }
}

struct FakeDelivery {
    outcome: Result<(), KernelError>,
    delivered: Mutex<Vec<(String, Accept)>>,
}

impl ActivityDelivery for FakeDelivery {
    async fn deliver(&self, inbox: &str, accept: &Accept) -> Result<(), Report<KernelError>> {
        self.delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((inbox.to_owned(), accept.clone()));
        self.outcome.map_err(Report::new)
    }
}

struct Fakes {
    local: FakeLocal,
    resolver: FakeResolver,
    delivery: FakeDelivery,
}

impl DependOnLocalActorProvider for Fakes {
    type LocalActorProvider = FakeLocal;
    fn local_actor_provider(&self) -> &Self::LocalActorProvider {
        &self.local
    }
}

impl DependOnRemoteActorResolver for Fakes {
    type RemoteActorResolver = FakeResolver;
    fn remote_actor_resolver(&self) -> &Self::RemoteActorResolver {
        &self.resolver
    }
}

impl DependOnActivityDelivery for Fakes {
    type ActivityDelivery = FakeDelivery;
    fn activity_delivery(&self) -> &Self::ActivityDelivery {
        &self.delivery
    }
}

impl Fakes {
    fn new(
        resolved: Result<RemoteActor, KernelError>,
        delivered: Result<(), KernelError>,
    ) -> TestResult<Self> {
        let local = LocalActor::new(Url::parse(LOCAL)?, "local pem").map_err(unexpected)?;
        Ok(Self {
            local: FakeLocal(local),
            resolver: FakeResolver {
                outcome: resolved,
                requested: Mutex::new(Vec::new()),
            },
            delivery: FakeDelivery {
                outcome: delivered,
                delivered: Mutex::new(Vec::new()),
            },
        })
    }

    fn working() -> TestResult<Self> {
        Self::new(Ok(follower()?), Ok(()))
    }

    fn requested(&self) -> Vec<String> {
        self.resolver
            .requested
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn delivered(&self) -> Vec<(String, Accept)> {
        self.delivery
            .delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn follower() -> TestResult<RemoteActor> {
    RemoteActor::new(
        &ActorId::parse(FOLLOWER).map_err(unexpected)?,
        FOLLOWER,
        FOLLOWER_INBOX,
        PublicKey::new(FOLLOWER_KEY, FOLLOWER, "follower pem"),
    )
    .map_err(unexpected)
}

fn inbound() -> InboundFollow {
    InboundFollow {
        id: FOLLOW_ID.to_owned(),
        actor: FOLLOWER.to_owned(),
        object: LOCAL.to_owned(),
        host: Some(LOCAL_HOST.to_owned()),
    }
}

async fn identify(
    fakes: &Fakes,
    inbound: InboundFollow,
) -> Result<IdentifiedFollow, ApplicationError> {
    fakes
        .identify(inbound, FOLLOWER_KEY)
        .await
        .map_err(|report| *report.current_context())
}

async fn rejection(fakes: &Fakes, inbound: InboundFollow) -> TestResult<ApplicationError> {
    match identify(fakes, inbound).await {
        Ok(_) => Err("the Follow was identified".into()),
        Err(error) => Ok(error),
    }
}

#[tokio::test]
async fn identified_follow_is_answered_with_an_accept_delivered_to_the_follower_inbox() -> TestResult
{
    // Given a Follow of the local actor from a resolvable follower.
    let fakes = Fakes::working()?;

    // When it is identified and accepted.
    let identified = identify(&fakes, inbound()).await?;
    fakes.accept(identified.clone()).await.map_err(unexpected)?;

    // Then the follower was resolved once and one Accept went to its inbox.
    assert_eq!(fakes.requested(), [FOLLOWER]);
    assert_eq!(identified.actor(), &follower()?);
    assert_eq!(identified.follow().id().as_str(), FOLLOW_ID);
    let delivered = fakes.delivered();
    assert_eq!(delivered.len(), 1);
    let (inbox, accept) = &delivered[0];
    assert_eq!(inbox, FOLLOWER_INBOX);
    assert_eq!(
        accept.id().as_str(),
        "https://local.example:8443/actor/accepts/https:%2F%2Fremote.example%2Ffollows%2F1"
    );
    assert_eq!(accept.actor().as_str(), LOCAL);
    assert_eq!(accept.object(), identified.follow());
    assert_eq!(accept.object().actor().as_str(), FOLLOWER);
    assert_eq!(accept.object().object().as_str(), LOCAL);
    Ok(())
}

#[tokio::test]
async fn follow_rejected_before_resolving_never_reaches_the_remote_ports() -> TestResult {
    let cases: [(&str, Edit, ApplicationError); 6] = [
        (
            "id is not a URL",
            |f| f.id = "not a url".into(),
            ApplicationError::Malformed,
        ),
        (
            "object is another actor",
            |f| f.object = "https://local.example:8443/someone-else".into(),
            ApplicationError::UntrustedIdentity,
        ),
        (
            "no Host",
            |f| f.host = None,
            ApplicationError::UntrustedIdentity,
        ),
        (
            "Host without the port",
            |f| f.host = Some("local.example".into()),
            ApplicationError::UntrustedIdentity,
        ),
        (
            "actor is not a URL",
            |f| f.actor = "not a url".into(),
            ApplicationError::Malformed,
        ),
        (
            "id check comes before the object check",
            |f| {
                f.id = "not a url".into();
                f.object = "https://local.example:8443/someone-else".into();
            },
            ApplicationError::Malformed,
        ),
    ];
    for (label, edit, expected) in cases {
        // Given a Follow that fails a check made before the follower is resolved.
        let fakes = Fakes::working()?;
        let mut follow = inbound();
        edit(&mut follow);

        // When it is identified.
        let error = rejection(&fakes, follow).await?;

        // Then it is rejected with its class, and no remote port was used.
        assert_eq!(error, expected, "{label}");
        assert!(fakes.requested().is_empty(), "{label}: resolver was called");
        assert!(
            fakes.delivered().is_empty(),
            "{label}: something was delivered"
        );
    }
    Ok(())
}

#[tokio::test]
async fn object_and_host_checks_come_before_the_actor_url_check() -> TestResult {
    let cases: [(&str, Edit); 2] = [
        ("object", |f| {
            f.object = "https://elsewhere.example/actor".into()
        }),
        ("host", |f| f.host = Some("other.example".into())),
    ];
    for (label, edit) in cases {
        // Given a Follow whose actor is not a URL and that also fails `label`.
        let fakes = Fakes::working()?;
        let mut follow = inbound();
        follow.actor = "not a url".into();
        edit(&mut follow);

        // When it is identified.
        let error = rejection(&fakes, follow).await?;

        // Then the earlier identity check decides.
        assert_eq!(error, ApplicationError::UntrustedIdentity, "{label}");
    }
    Ok(())
}

#[tokio::test]
async fn key_id_of_another_key_is_untrusted_after_resolving() -> TestResult {
    // Given a follower whose published key is not the claimed one.
    let fakes = Fakes::working()?;

    // When the Follow is identified under another keyId.
    let outcome = fakes
        .identify(inbound(), "https://remote.example/actor#other-key")
        .await;

    // Then it is untrusted; the follower was resolved but nothing delivered.
    let error = outcome.err().ok_or("the Follow was identified")?;
    assert_eq!(
        error.current_context(),
        &ApplicationError::UntrustedIdentity
    );
    assert_eq!(fakes.requested(), [FOLLOWER]);
    assert!(fakes.delivered().is_empty());
    Ok(())
}

#[tokio::test]
async fn resolver_failures_keep_their_class() -> TestResult {
    for (failure, expected) in [
        (
            KernelError::OriginNotAllowed,
            ApplicationError::OriginNotAllowed,
        ),
        (KernelError::RemoteFailure, ApplicationError::RemoteFailure),
        (KernelError::Malformed, ApplicationError::Malformed),
        (
            KernelError::UntrustedIdentity,
            ApplicationError::UntrustedIdentity,
        ),
    ] {
        // Given a resolver that fails with `failure`.
        let fakes = Fakes::new(Err(failure), Ok(()))?;

        // When a Follow is identified.
        let error = rejection(&fakes, inbound()).await?;

        // Then the class is kept and nothing is delivered.
        assert_eq!(error, expected, "{failure:?}");
        assert!(fakes.delivered().is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn delivery_failure_keeps_its_class() -> TestResult {
    // Given an inbox that cannot be reached.
    let fakes = Fakes::new(Ok(follower()?), Err(KernelError::RemoteFailure))?;
    let identified = identify(&fakes, inbound()).await?;

    // When the Accept is delivered.
    let outcome = fakes.accept(identified).await;

    // Then the remote failure is reported after the one attempt.
    let error = outcome.err().ok_or("the Accept was delivered")?;
    assert_eq!(error.current_context(), &ApplicationError::RemoteFailure);
    assert_eq!(fakes.delivered().len(), 1);
    Ok(())
}
