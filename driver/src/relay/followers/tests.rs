//! The follower journal, the read model and the projection between them, on
//! PostgreSQL. Each test gets its own empty database from `TestDatabase`
//! (`SHUTTLEPUB_TEST_DATABASE_URL`, or a container) and fails rather than
//! passing when neither is available. A projection runs by polling, so the
//! tests wait for the read model with a deadline instead of reading it right
//! after a record. Test names carry the issue's check numbers (`r01_`..`r10_`).

use std::{convert::Infallible, error::Error, time::Duration};

use error_stack::Report;
use kernel::{
    KernelError,
    identity::{ActivityId, ActorId},
    relay::{Follower, FollowerReader, FollowerRecorder},
};
use nitinol::{
    eventsource::{AskError, MissingSnapshotSlot},
    runtime::error::SendError,
    sqlx::{PostgresStore, sqlx},
};

use super::{FollowerJournal, FollowerReadModel, record_outcome, start_follower_projection};
use crate::persistence::{Database, DatabaseSettings, EventSourcing, test_support::TestDatabase};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const ACTOR: &str = "https://remote.example/actor";
const INBOX: &str = "https://remote.example/inbox";
const FOLLOW_ID: &str = "https://remote.example/follows/1";
const OTHER_INBOX: &str = "https://remote.example/shared-inbox";
const OTHER_FOLLOW_ID: &str = "https://remote.example/follows/2";
const PROJECTION_SCOPE: &str = "shuttlepub.relay.followers";
const PROJECTION_DEADLINE: Duration = Duration::from_secs(10);

fn unexpected<C: Error + Send + Sync + 'static>(report: Report<C>) -> Box<dyn Error> {
    format!("{report:?}").into()
}

fn actor() -> TestResult<ActorId> {
    ActorId::parse(ACTOR).map_err(unexpected)
}

fn activity(id: &str) -> TestResult<ActivityId> {
    ActivityId::parse(id).map_err(unexpected)
}

async fn test_database() -> TestResult<TestDatabase> {
    TestDatabase::start()
        .await
        .map_err(|error| format!("no PostgreSQL for the test: {error:?}").into())
}

/// Connects and migrates the way the server does at startup.
async fn connect(database: &TestDatabase) -> TestResult<Database> {
    let url = database.url().to_owned();
    let settings = DatabaseSettings::from_lookup(|name| {
        (name == "SHUTTLEPUB_DATABASE_URL").then(|| url.clone())
    })
    .map_err(unexpected)?;
    let connected = Database::connect(&settings).await.map_err(unexpected)?;
    connected.migrate().await.map_err(unexpected)?;
    Ok(connected)
}

async fn event_sourcing(database: &Database) -> TestResult<EventSourcing<PostgresStore>> {
    Ok(EventSourcing::start(database.event_store().map_err(unexpected)?).await)
}

async fn record(journal: &FollowerJournal, inbox: &str, follow: &str) -> TestResult {
    journal
        .record(&actor()?, inbox, &activity(follow)?)
        .await
        .map_err(unexpected)
}

async fn listed(database: &Database) -> TestResult<Vec<(String, String)>> {
    Ok(FollowerReadModel::new(database)
        .list()
        .await
        .map_err(unexpected)?
        .iter()
        .map(|entry| (entry.actor().as_str().to_owned(), entry.inbox().to_owned()))
        .collect())
}

fn follower_row(inbox: &str) -> Vec<(String, String)> {
    vec![(ACTOR.to_owned(), inbox.to_owned())]
}

/// Waits until the read model lists exactly `expected`.
async fn wait_until_listed(database: &Database, expected: &[(String, String)]) -> TestResult {
    let deadline = tokio::time::Instant::now() + PROJECTION_DEADLINE;
    let mut poll = tokio::time::interval(Duration::from_millis(50));
    loop {
        poll.tick().await;
        let current = listed(database).await?;
        if current == expected {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Err(format!("the read model lists {current:?}, not {expected:?}").into());
        }
    }
}

async fn journaled_events(database: &Database) -> TestResult<i64> {
    let id = Follower::aggregate_id(&actor()?);
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM nitinol_journal WHERE persistence_id = $1",
    )
    .bind(id.as_str())
    .fetch_one(database.pool())
    .await?)
}

async fn projection_offset(database: &Database) -> TestResult<Option<i64>> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT sequence FROM nitinol_offset WHERE scope = $1")
            .bind(PROJECTION_SCOPE)
            .fetch_optional(database.pool())
            .await?,
    )
}

/// Waits until the projection saved `expected` as its offset.
async fn wait_until_offset(database: &Database, expected: i64) -> TestResult {
    let deadline = tokio::time::Instant::now() + PROJECTION_DEADLINE;
    let mut poll = tokio::time::interval(Duration::from_millis(50));
    loop {
        poll.tick().await;
        let current = projection_offset(database).await?;
        if current == Some(expected) {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Err(format!("the projection offset is {current:?}, not {expected}").into());
        }
    }
}

// ----------------------------------------------------- failure translation ----

#[test]
fn accepted_command_and_an_already_created_follower_are_both_recorded() {
    // `AlreadyCreated`: the first write of the follower met one that exists,
    // i.e. the actor already is a follower.
    for (label, outcome) in [
        ("accepted", Ok(())),
        ("already created", Err(AskError::AlreadyCreated)),
    ] {
        assert!(record_outcome(outcome).is_ok(), "{label}");
    }
}

#[test]
fn failures_left_after_retrying_are_internal() {
    // What `ask_with_retry` still returns after its retries, transient
    // (`Send`) or permanent (`Activation`).
    let failures: [(&str, AskError<Infallible>); 2] = [
        ("send", AskError::Send(SendError)),
        ("activation", AskError::Activation(MissingSnapshotSlot)),
    ];
    for (label, failure) in failures {
        let outcome = record_outcome(Err(failure));
        assert_eq!(
            outcome.err().map(|report| *report.current_context()),
            Some(KernelError::Internal),
            "{label}"
        );
    }
}

// ------------------------------------------------------------- PostgreSQL ----

#[tokio::test]
async fn r01_recorded_follower_appears_in_the_list() -> TestResult {
    // Given a migrated database with the follower projection running.
    let database = test_database().await?;
    let connected = connect(&database).await?;
    let events = event_sourcing(&connected).await?;
    let projection = start_follower_projection(&events, &connected).await;

    // When a follower is recorded.
    record(&FollowerJournal::new(&events), INBOX, FOLLOW_ID).await?;

    // Then it is listed with its inbox once the projection caught up.
    wait_until_listed(&connected, &follower_row(INBOX)).await?;
    projection.stop().await.map_err(unexpected)?;
    Ok(())
}

#[tokio::test]
async fn r02_follower_recorded_again_adds_no_event_and_no_row() -> TestResult {
    // Given a follower that was recorded and projected.
    let database = test_database().await?;
    let connected = connect(&database).await?;
    let events = event_sourcing(&connected).await?;
    let projection = start_follower_projection(&events, &connected).await;
    let journal = FollowerJournal::new(&events);
    record(&journal, INBOX, FOLLOW_ID).await?;
    wait_until_listed(&connected, &follower_row(INBOX)).await?;

    // When the same actor is recorded again, once with the same Follow and
    // once with another Follow naming another inbox.
    record(&journal, INBOX, FOLLOW_ID).await?;
    record(&journal, OTHER_INBOX, OTHER_FOLLOW_ID).await?;

    // Then both succeed, the journal still holds one event for the actor, and
    // the list keeps the one row with the first inbox.
    assert_eq!(journaled_events(&connected).await?, 1);
    assert_eq!(listed(&connected).await?, follower_row(INBOX));
    projection.stop().await.map_err(unexpected)?;
    Ok(())
}

#[tokio::test]
async fn r04_recording_without_a_usable_database_is_internal() -> TestResult {
    // Given a journal whose database connections are closed.
    let database = test_database().await?;
    let connected = connect(&database).await?;
    let events = event_sourcing(&connected).await?;
    let journal = FollowerJournal::new(&events);
    connected.pool().close().await;

    // When a follower is recorded.
    let outcome = journal
        .record(&actor()?, INBOX, &activity(FOLLOW_ID)?)
        .await;

    // Then the failure is internal.
    assert_eq!(
        outcome.err().map(|report| *report.current_context()),
        Some(KernelError::Internal)
    );
    Ok(())
}

#[tokio::test]
async fn r07_followers_and_their_state_survive_a_restart_on_the_same_database() -> TestResult {
    // Given a follower recorded and projected by a first run.
    let database = test_database().await?;
    let first = connect(&database).await?;
    let first_events = event_sourcing(&first).await?;
    let first_projection = start_follower_projection(&first_events, &first).await;
    record(&FollowerJournal::new(&first_events), INBOX, FOLLOW_ID).await?;
    wait_until_listed(&first, &follower_row(INBOX)).await?;
    first_projection.stop().await.map_err(unexpected)?;

    // When a second run connects to the same database.
    let second = connect(&database).await?;
    let second_events = event_sourcing(&second).await?;
    let second_projection = start_follower_projection(&second_events, &second).await;

    // Then the follower is still listed, and recording the actor again is a
    // resend: it succeeds without a new event and keeps the first inbox.
    assert_eq!(listed(&second).await?, follower_row(INBOX));
    record(
        &FollowerJournal::new(&second_events),
        OTHER_INBOX,
        OTHER_FOLLOW_ID,
    )
    .await?;
    assert_eq!(journaled_events(&second).await?, 1);
    assert_eq!(listed(&second).await?, follower_row(INBOX));
    second_projection.stop().await.map_err(unexpected)?;
    Ok(())
}

#[tokio::test]
async fn r08_migrations_run_on_an_empty_database_and_again_on_a_migrated_one() -> TestResult {
    // Given an empty database.
    let database = test_database().await?;

    // When it is connected and migrated twice, as two consecutive starts do.
    let first = connect(&database).await?;
    let second = connect(&database).await?;

    // Then both succeed and the read model is there and empty.
    assert_eq!(listed(&first).await?, Vec::new());
    assert_eq!(listed(&second).await?, Vec::new());
    Ok(())
}

#[tokio::test]
async fn r10_event_projected_twice_leaves_the_list_unchanged() -> TestResult {
    // Given a follower recorded and projected.
    let database = test_database().await?;
    let connected = connect(&database).await?;
    let events = event_sourcing(&connected).await?;
    let projection = start_follower_projection(&events, &connected).await;
    record(&FollowerJournal::new(&events), INBOX, FOLLOW_ID).await?;
    wait_until_listed(&connected, &follower_row(INBOX)).await?;
    let projected = projection_offset(&connected)
        .await?
        .ok_or("the projection saved no offset")?;
    projection.stop().await.map_err(unexpected)?;

    // When the projection forgets its offset and projects the event again.
    sqlx::query("DELETE FROM nitinol_offset WHERE scope = $1")
        .bind(PROJECTION_SCOPE)
        .execute(connected.pool())
        .await?;
    let replay = start_follower_projection(&events, &connected).await;
    wait_until_offset(&connected, projected).await?;

    // Then the list is the same as after the first projection.
    assert_eq!(listed(&connected).await?, follower_row(INBOX));
    replay.stop().await.map_err(unexpected)?;
    Ok(())
}
