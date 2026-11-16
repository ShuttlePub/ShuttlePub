//! `AcceptFollowService` with every port replaced by an in-memory fake: no
//! HTTP server, no network and no store. The fakes record their calls, and the
//! follower recorder and the delivery share one log, so that the tests can see
//! which ports a Follow reached and in which order. Test names carry the
//! issue's check numbers (`r01_`..`r05_`) where they cover one.

use std::{
    error::Error,
    sync::{Arc, Mutex, PoisonError},
};

use application::{
    ApplicationError,
    relay::{AcceptFollowService, IdentifiedFollow, InboundFollow},
};
use error_stack::Report;
use kernel::{
    KernelError,
    identity::{
        ActivityId, ActorId, DependOnLocalActorProvider, LocalActor, LocalActorProvider, PublicKey,
    },
    relay::{
        Accept, ActivityDelivery, DependOnActivityDelivery, DependOnFollowerRecorder,
        DependOnRemoteActorResolver, FollowerRecorder, RemoteActor, RemoteActorResolver,
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

/// A port call, in the order the service made it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PortCall {
    Record {
        actor: String,
        inbox: String,
        follow: String,
    },
    Deliver {
        inbox: String,
        accept: Box<Accept>,
    },
}

type CallLog = Arc<Mutex<Vec<PortCall>>>;

fn push(log: &CallLog, call: PortCall) {
    log.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(call);
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

struct FakeRecorder {
    outcome: Result<(), KernelError>,
    log: CallLog,
}

impl FollowerRecorder for FakeRecorder {
    async fn record(
        &self,
        actor: &ActorId,
        inbox: &str,
        follow: &ActivityId,
    ) -> Result<(), Report<KernelError>> {
        push(
            &self.log,
            PortCall::Record {
                actor: actor.as_str().to_owned(),
                inbox: inbox.to_owned(),
                follow: follow.as_str().to_owned(),
            },
        );
        self.outcome.map_err(Report::new)
    }
}

/// The inbox answers every delivery with `outcome` until it is changed, so a
/// test can let one delivery fail and the next one succeed.
struct FakeDelivery {
    outcome: Mutex<Result<(), KernelError>>,
    log: CallLog,
}

impl ActivityDelivery for FakeDelivery {
    async fn deliver(&self, inbox: &str, accept: &Accept) -> Result<(), Report<KernelError>> {
        push(
            &self.log,
            PortCall::Deliver {
                inbox: inbox.to_owned(),
                accept: Box::new(accept.clone()),
            },
        );
        let outcome = *self.outcome.lock().unwrap_or_else(PoisonError::into_inner);
        outcome.map_err(Report::new)
    }
}

struct Fakes {
    local: FakeLocal,
    resolver: FakeResolver,
    recorder: FakeRecorder,
    delivery: FakeDelivery,
    log: CallLog,
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

impl DependOnFollowerRecorder for Fakes {
    type FollowerRecorder = FakeRecorder;
    fn follower_recorder(&self) -> &Self::FollowerRecorder {
        &self.recorder
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
        recorded: Result<(), KernelError>,
        delivered: Result<(), KernelError>,
    ) -> TestResult<Self> {
        let local = LocalActor::new(Url::parse(LOCAL)?, "local pem").map_err(unexpected)?;
        let log = CallLog::default();
        Ok(Self {
            local: FakeLocal(local),
            resolver: FakeResolver {
                outcome: resolved,
                requested: Mutex::new(Vec::new()),
            },
            recorder: FakeRecorder {
                outcome: recorded,
                log: log.clone(),
            },
            delivery: FakeDelivery {
                outcome: Mutex::new(delivered),
                log: log.clone(),
            },
            log,
        })
    }

    fn working() -> TestResult<Self> {
        Self::new(Ok(follower()?), Ok(()), Ok(()))
    }

    fn deliveries_succeed_from_now_on(&self) {
        *self
            .delivery
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Ok(());
    }

    fn requested(&self) -> Vec<String> {
        self.resolver
            .requested
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn calls(&self) -> Vec<PortCall> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn recorded(&self) -> Vec<PortCall> {
        self.calls()
            .into_iter()
            .filter(|call| matches!(call, PortCall::Record { .. }))
            .collect()
    }

    fn delivered(&self) -> Vec<(String, Accept)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                PortCall::Deliver { inbox, accept } => Some((inbox, *accept)),
                PortCall::Record { .. } => None,
            })
            .collect()
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

fn follower_record() -> PortCall {
    PortCall::Record {
        actor: FOLLOWER.to_owned(),
        inbox: FOLLOWER_INBOX.to_owned(),
        follow: FOLLOW_ID.to_owned(),
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

// --------------------------------------------------------------- accepted ----

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
async fn r01_follower_is_recorded_before_the_accept_is_delivered() -> TestResult {
    // Given a Follow of the local actor from a resolvable follower.
    let fakes = Fakes::working()?;
    let identified = identify(&fakes, inbound()).await?;

    // When it is accepted.
    fakes.accept(identified).await.map_err(unexpected)?;

    // Then the follower, its inbox and the Follow id were recorded once, and
    // only after that was the Accept delivered.
    let calls = fakes.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0], follower_record());
    assert!(
        matches!(&calls[1], PortCall::Deliver { inbox, .. } if inbox == FOLLOWER_INBOX),
        "{calls:?}"
    );
    Ok(())
}

#[tokio::test]
async fn r02_follow_accepted_again_is_recorded_and_answered_again() -> TestResult {
    // Given a follower whose Follow was already accepted.
    let fakes = Fakes::working()?;
    let identified = identify(&fakes, inbound()).await?;
    fakes.accept(identified.clone()).await.map_err(unexpected)?;

    // When the same Follow is accepted again.
    fakes.accept(identified).await.map_err(unexpected)?;

    // Then it succeeds again: recorded, then answered with the same Accept.
    let calls = fakes.calls();
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert_eq!(calls[2], follower_record());
    let delivered = fakes.delivered();
    assert_eq!(delivered.len(), 2);
    assert_eq!(delivered[0], delivered[1]);
    Ok(())
}

// ---------------------------------------------------------------- failures ----

#[tokio::test]
async fn r04_recording_failure_is_internal_and_nothing_is_delivered() -> TestResult {
    // Given a follower recorder that fails.
    let fakes = Fakes::new(Ok(follower()?), Err(KernelError::Internal), Ok(()))?;
    let identified = identify(&fakes, inbound()).await?;

    // When the Follow is accepted.
    let outcome = fakes.accept(identified).await;

    // Then the failure is internal and no Accept was delivered.
    let error = outcome.err().ok_or("the Follow was accepted")?;
    assert_eq!(error.current_context(), &ApplicationError::Internal);
    assert_eq!(fakes.recorded(), [follower_record()]);
    assert!(fakes.delivered().is_empty(), "{:?}", fakes.calls());
    Ok(())
}

#[tokio::test]
async fn r05_delivery_failure_keeps_its_class_after_the_follower_was_recorded() -> TestResult {
    // Given an inbox that cannot be reached.
    let fakes = Fakes::new(Ok(follower()?), Ok(()), Err(KernelError::RemoteFailure))?;
    let identified = identify(&fakes, inbound()).await?;

    // When the Accept is delivered.
    let outcome = fakes.accept(identified).await;

    // Then the remote failure is reported after the one attempt, and the
    // follower had been recorded first.
    let error = outcome.err().ok_or("the Accept was delivered")?;
    assert_eq!(error.current_context(), &ApplicationError::RemoteFailure);
    let calls = fakes.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0], follower_record());
    assert!(matches!(calls[1], PortCall::Deliver { .. }), "{calls:?}");
    Ok(())
}

#[tokio::test]
async fn r05_follow_resent_after_a_failed_delivery_is_accepted() -> TestResult {
    // Given a Follow whose Accept could not be delivered.
    let fakes = Fakes::new(Ok(follower()?), Ok(()), Err(KernelError::RemoteFailure))?;
    let identified = identify(&fakes, inbound()).await?;
    let first = fakes.accept(identified).await;
    assert_eq!(
        first.err().map(|error| *error.current_context()),
        Some(ApplicationError::RemoteFailure)
    );

    // When the follower sends the Follow again and its inbox is reachable.
    fakes.deliveries_succeed_from_now_on();
    let identified = identify(&fakes, inbound()).await?;
    fakes.accept(identified).await.map_err(unexpected)?;

    // Then the resent Follow is recorded and answered like the first one.
    assert_eq!(fakes.recorded(), [follower_record(), follower_record()]);
    let delivered = fakes.delivered();
    assert_eq!(delivered.len(), 2);
    assert_eq!(delivered[0], delivered[1]);
    Ok(())
}

// --------------------------------------------------------------- rejected ----

#[tokio::test]
async fn r03_follow_rejected_before_resolving_never_reaches_the_remote_ports_or_the_recorder()
-> TestResult {
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

        // Then it is rejected with its class, and no other port was used.
        assert_eq!(error, expected, "{label}");
        assert!(fakes.requested().is_empty(), "{label}: resolver was called");
        assert!(
            fakes.calls().is_empty(),
            "{label}: recorded or delivered {:?}",
            fakes.calls()
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
async fn r03_key_id_of_another_key_is_untrusted_after_resolving_and_nothing_is_recorded()
-> TestResult {
    // Given a follower whose published key is not the claimed one.
    let fakes = Fakes::working()?;

    // When the Follow is identified under another keyId.
    let outcome = fakes
        .identify(inbound(), "https://remote.example/actor#other-key")
        .await;

    // Then it is untrusted; the follower was resolved but nothing recorded or
    // delivered.
    let error = outcome.err().ok_or("the Follow was identified")?;
    assert_eq!(
        error.current_context(),
        &ApplicationError::UntrustedIdentity
    );
    assert_eq!(fakes.requested(), [FOLLOWER]);
    assert!(fakes.calls().is_empty(), "{:?}", fakes.calls());
    Ok(())
}

#[tokio::test]
async fn r03_resolver_failures_keep_their_class_and_nothing_is_recorded() -> TestResult {
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
        let fakes = Fakes::new(Err(failure), Ok(()), Ok(()))?;

        // When a Follow is identified.
        let error = rejection(&fakes, inbound()).await?;

        // Then the class is kept and nothing is recorded or delivered.
        assert_eq!(error, expected, "{failure:?}");
        assert!(fakes.calls().is_empty(), "{failure:?}: {:?}", fakes.calls());
    }
    Ok(())
}
