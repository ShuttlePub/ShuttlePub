//! The intake, producer and note services and the Accept delivery reaction,
//! with every port replaced by an in-memory fake and the events kept in
//! nitinol's in-memory store: no HTTP server, no database and no network. The
//! fakes record their calls so that the tests can see which ports were used.

use std::{
    error::Error,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use application::{
    ApplicationError,
    delivery::{AcceptDelivery, accept_delivery_projection},
    eventsource::DependOnEventSourceSystem,
    intake::{IdentifiedFollow, InboundFollow, IntakeService},
    note::{DependOnNoteService, NoteService},
    producer::{DependOnProducerService, ProducerService},
};
use error_stack::Report;
use futures_util::TryStreamExt;
use kernel::{
    KernelError,
    community::{DependOnLocalActorProvider, LocalActor, LocalActorProvider, PublicKey},
    id::{ActivityId, ActorId},
    intake::RemoteActor,
    note::{Note, NoteEvent, ReceiveNote},
    producer::{Accept, AcceptFederation, ProducerEvent},
};
use nitinol::{
    contract::{Codec, Event},
    eventsource::{AskError, system::EventSourceSystem},
    persistence::{
        LoadQuery, LoadedEvent,
        provider::{PersistenceProvider, Store},
        store::InMemoryStore,
    },
    runtime::ProcessSystem,
};
use port::{
    delivery::{ActivityDelivery, DependOnActivityDelivery},
    intake::{DependOnRemoteActorResolver, RemoteActorResolver},
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

/// How long the Accept delivery, which runs in the background, may take to
/// record what happened to an Accept.
const REACTION_DEADLINE: Duration = Duration::from_secs(10);

fn unexpected<C: Error + Send + Sync + 'static>(report: Report<C>) -> Box<dyn Error> {
    format!("{report:?}").into()
}

// ------------------------------------------------------------------ fakes ----

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
    store: InMemoryStore,
    system: EventSourceSystem<InMemoryStore>,
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

impl DependOnEventSourceSystem for Fakes {
    type Store = InMemoryStore;
    fn event_source_system(&self) -> &EventSourceSystem<InMemoryStore> {
        &self.system
    }
}

impl DependOnProducerService for Fakes {
    type ProducerService = Self;
    fn producer_service(&self) -> &Self::ProducerService {
        self
    }
}

impl DependOnNoteService for Fakes {
    type NoteService = Self;
    fn note_service(&self) -> &Self::NoteService {
        self
    }
}

impl Fakes {
    async fn new(
        resolved: Result<RemoteActor, KernelError>,
        delivered: Result<(), KernelError>,
    ) -> TestResult<Self> {
        Self::on_store(InMemoryStore::default(), resolved, delivered).await
    }

    /// Fakes whose events go to `store`, which other fakes may share.
    async fn on_store(
        store: InMemoryStore,
        resolved: Result<RemoteActor, KernelError>,
        delivered: Result<(), KernelError>,
    ) -> TestResult<Self> {
        let local = LocalActor::new(Url::parse(LOCAL)?, "local pem").map_err(unexpected)?;
        let system = EventSourceSystem::builder(ProcessSystem::new().await)
            .with_persistence(PersistenceProvider::new(store.clone()))
            .build();
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
            store,
            system,
        })
    }

    async fn working() -> TestResult<Self> {
        Self::new(Ok(follower()?), Ok(())).await
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

    /// Every event of type `E` recorded so far, in journal order.
    async fn recorded<E: Event>(&self) -> TestResult<Vec<E>> {
        let events: Vec<LoadedEvent> = self
            .store
            .events()
            .load(LoadQuery::default())
            .await
            .map_err(|error| format!("{error:?}"))?
            .try_collect()
            .await
            .map_err(|error| format!("{error:?}"))?;
        events
            .iter()
            .filter(|event| event.event_type == E::EVENT_TYPE)
            .map(|event| Ok(<E::Codec as Codec<E>>::decode(&event.payload)?))
            .collect()
    }

    /// Waits until `count` events of type `E` are recorded.
    async fn wait_for_recorded<E: Event>(&self, count: usize) -> TestResult<Vec<E>> {
        let deadline = tokio::time::Instant::now() + REACTION_DEADLINE;
        loop {
            let events = self.recorded::<E>().await?;
            if events.len() >= count {
                return Ok(events);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    format!("{count} events not recorded within {REACTION_DEADLINE:?}").into(),
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Starts the Accept delivery over these fakes' events.
    async fn react(self: &Arc<Self>) {
        let module = Arc::clone(self);
        self.system
            .projector_props(accept_delivery_projection(), move || {
                AcceptDelivery::new(Arc::clone(&module))
            })
            .with_event::<ProducerEvent>()
            .catchup_from_global()
            .spawn()
            .await;
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
        .identify_follow(inbound, FOLLOWER_KEY)
        .await
        .map_err(|report| *report.current_context())
}

async fn rejection(fakes: &Fakes, inbound: InboundFollow) -> TestResult<ApplicationError> {
    match identify(fakes, inbound).await {
        Ok(_) => Err("the Follow was identified".into()),
        Err(error) => Ok(error),
    }
}

fn federation_of(actor: &str, inbox: &str, follow: &str) -> TestResult<AcceptFederation> {
    Ok(AcceptFederation {
        actor: ActorId::parse(actor).map_err(unexpected)?,
        inbox: inbox.to_owned(),
        follow: ActivityId::parse(follow).map_err(unexpected)?,
    })
}

// ---------------------------------------------------------- identifying ----

#[tokio::test]
async fn identified_follow_names_the_follower_and_the_actor_it_resolved_to() -> TestResult {
    // Given a Follow of the local actor from a resolvable follower.
    let fakes = Fakes::working().await?;

    // When it is identified.
    let identified = identify(&fakes, inbound()).await?;

    // Then the follower was resolved once and nothing was delivered.
    assert_eq!(fakes.requested(), [FOLLOWER]);
    assert_eq!(identified.sender(), &follower()?);
    assert_eq!(identified.follow().id().as_str(), FOLLOW_ID);
    assert_eq!(identified.follow().actor().as_str(), FOLLOWER);
    assert_eq!(identified.follow().object().as_str(), LOCAL);
    assert!(fakes.delivered().is_empty());
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
        let fakes = Fakes::working().await?;
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
        let fakes = Fakes::working().await?;
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
    let fakes = Fakes::working().await?;

    // When the Follow is identified under another keyId.
    let outcome = fakes
        .identify_follow(inbound(), "https://remote.example/actor#other-key")
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
        let fakes = Fakes::new(Err(failure), Ok(())).await?;

        // When a Follow is identified.
        let error = rejection(&fakes, inbound()).await?;

        // Then the class is kept and nothing is delivered.
        assert_eq!(error, expected, "{failure:?}");
        assert!(fakes.delivered().is_empty());
    }
    Ok(())
}

// -------------------------------------------------------------- recording ----

#[tokio::test]
async fn accepted_follow_records_the_federation_without_delivering_anything() -> TestResult {
    // Given an identified Follow.
    let fakes = Fakes::working().await?;
    let identified = identify(&fakes, inbound()).await?;

    // When it is accepted.
    fakes.accept_follow(identified).await.map_err(unexpected)?;

    // Then the federation is recorded with the follower's inbox, the service
    // counts as accepted, and no Accept went out while answering.
    assert_eq!(
        fakes.recorded::<ProducerEvent>().await?,
        [ProducerEvent::FederationAccepted {
            actor: ActorId::parse(FOLLOWER).map_err(unexpected)?,
            inbox: FOLLOWER_INBOX.to_owned(),
            follow: ActivityId::parse(FOLLOW_ID).map_err(unexpected)?,
        }]
    );
    let another_actor_of_the_service =
        ActorId::parse("https://remote.example/users/alice").map_err(unexpected)?;
    assert!(
        fakes
            .is_accepted(&another_actor_of_the_service)
            .await
            .map_err(unexpected)?
    );
    assert!(fakes.delivered().is_empty());
    Ok(())
}

#[tokio::test]
async fn note_whose_first_write_collides_is_known_and_recorded_once() -> TestResult {
    // Given three systems over one store, two of which activated the Note
    // before the first one recorded it.
    let store = InMemoryStore::default();
    let first = Fakes::on_store(store.clone(), Ok(follower()?), Ok(())).await?;
    let witness = Fakes::on_store(store.clone(), Ok(follower()?), Ok(())).await?;
    let late = Fakes::on_store(store, Ok(follower()?), Ok(())).await?;
    let command = ReceiveNote {
        activity: ActivityId::parse("https://remote.example/creates/1").map_err(unexpected)?,
        note: ActivityId::parse("https://remote.example/notes/1").map_err(unexpected)?,
        author: ActorId::parse(FOLLOWER).map_err(unexpected)?,
        producer: "remote.example".to_owned(),
        received_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000),
        raw: "{}".to_owned(),
    };
    let id = Note::aggregate_id(&command.note);
    let witness_note = witness.system.spawn_aggregate::<Note>(id.clone()).await;
    late.system.spawn_aggregate::<Note>(id).await;
    first.receive(command.clone()).await.map_err(unexpected)?;
    assert!(
        matches!(
            witness_note.ask(command.clone()).await,
            Err(AskError::AlreadyCreated)
        ),
        "a stale activation's first write must collide"
    );

    // When the late system receives the same Note.
    let outcome = late.receive(command).await;

    // Then it succeeds, and the Note is recorded once.
    outcome.map_err(unexpected)?;
    assert_eq!(late.recorded::<NoteEvent>().await?.len(), 1);
    Ok(())
}

// ------------------------------------------------------------ the reaction ----

#[tokio::test]
async fn reaction_delivers_the_accept_to_the_follower_inbox_and_records_it() -> TestResult {
    // Given the Accept delivery running over a federation just recorded.
    let fakes = Arc::new(Fakes::working().await?);
    fakes.react().await;
    let identified = identify(&fakes, inbound()).await?;
    fakes
        .accept_follow(identified.clone())
        .await
        .map_err(unexpected)?;

    // When the reaction has run.
    let events = fakes.wait_for_recorded::<ProducerEvent>(2).await?;

    // Then one Accept of that Follow went to the follower's inbox and its
    // delivery is recorded.
    assert_eq!(
        events[1],
        ProducerEvent::AcceptDelivered {
            follow: ActivityId::parse(FOLLOW_ID).map_err(unexpected)?,
        }
    );
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
    Ok(())
}

#[tokio::test]
async fn reaction_records_a_failed_delivery_and_goes_on_with_the_next_federation() -> TestResult {
    // Given the Accept delivery running, with every inbox failing.
    let fakes = Arc::new(Fakes::new(Ok(follower()?), Err(KernelError::RemoteFailure)).await?);
    fakes.react().await;

    // When two services' federations are recorded.
    for (actor, inbox, follow) in [
        (FOLLOWER, FOLLOWER_INBOX, FOLLOW_ID),
        (
            "https://other.example/actor",
            "https://other.example/inbox",
            "https://other.example/follows/1",
        ),
    ] {
        fakes
            .accept_federation(federation_of(actor, inbox, follow)?)
            .await
            .map_err(unexpected)?;
    }

    // Then both Accepts were attempted, and each failure is recorded with the
    // class of the delivery failure as its reason.
    let events = fakes.wait_for_recorded::<ProducerEvent>(4).await?;
    let failures: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|event| match event {
            ProducerEvent::AcceptFailed { follow, reason } => {
                Some((follow.as_str(), reason.as_str()))
            }
            _ => None,
        })
        .collect();
    let class = KernelError::RemoteFailure.to_string();
    assert_eq!(
        failures,
        [
            (FOLLOW_ID, class.as_str()),
            ("https://other.example/follows/1", class.as_str())
        ]
    );
    assert_eq!(fakes.delivered().len(), 2);
    Ok(())
}
