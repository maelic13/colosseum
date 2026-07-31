use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use colosseum_application::{
    ApplicationError, CommitUnit, CommitUnitDependencies, CommittedRunSnapshot, CompletedUnit,
    EngineInspection, EngineSession, EngineSessionFactory, ExecutionUnit, InspectEngine,
    PortFuture, ProgressEvent, ProgressSink, RunRepository, RunState, RuntimeParticipant,
    UnitOutcome,
};
use colosseum_core::{ParticipantId, RunId, UnitId};

fn block_on<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn completed() -> CompletedUnit {
    CompletedUnit {
        run_id: RunId::from_u128(1),
        unit: ExecutionUnit {
            id: UnitId::from_u128(2),
            game_id: None,
            sequence: 7,
            payload: Default::default(),
        },
        outcome: UnitOutcome::Completed {
            result: "1/2-1/2".into(),
        },
    }
}

struct FakeRepository {
    order: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
}

impl RunRepository for FakeRepository {
    fn commit_unit(
        &self,
        completed: &CompletedUnit,
    ) -> PortFuture<'_, Result<CommittedRunSnapshot, ApplicationError>> {
        let order = Arc::clone(&self.order);
        let fail = self.fail;
        let run_id = completed.run_id;
        Box::pin(async move {
            order.lock().unwrap().push("commit");
            if fail {
                return Err(ApplicationError::InfrastructureFault {
                    operation: "commit".into(),
                    message: "injected".into(),
                });
            }
            Ok(CommittedRunSnapshot {
                run_id,
                durable_sequence: 7,
                completed_units: 1,
                failed_units: 0,
                state: RunState::Running,
                anomalies: Vec::new(),
            })
        })
    }

    fn snapshot(
        &self,
        _run_id: RunId,
    ) -> PortFuture<'_, Result<CommittedRunSnapshot, ApplicationError>> {
        unreachable!()
    }
}

struct FakeProgress(Arc<Mutex<Vec<&'static str>>>);

impl ProgressSink for FakeProgress {
    fn publish(&self, event: ProgressEvent) {
        assert!(matches!(event, ProgressEvent::UnitCommitted { .. }));
        self.0.lock().unwrap().push("publish");
    }
}

#[test]
fn durable_commit_precedes_progress_publication() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let repository = FakeRepository {
        order: Arc::clone(&order),
        fail: false,
    };
    let progress = FakeProgress(Arc::clone(&order));

    block_on(CommitUnit::execute(
        CommitUnitDependencies {
            repository: &repository,
            progress: &progress,
        },
        &completed(),
    ))
    .unwrap();

    assert_eq!(*order.lock().unwrap(), ["commit", "publish"]);
}

#[test]
fn failed_commit_is_never_published() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let repository = FakeRepository {
        order: Arc::clone(&order),
        fail: true,
    };
    let progress = FakeProgress(Arc::clone(&order));

    assert!(
        block_on(CommitUnit::execute(
            CommitUnitDependencies {
                repository: &repository,
                progress: &progress,
            },
            &completed(),
        ))
        .is_err()
    );
    assert_eq!(*order.lock().unwrap(), ["commit"]);
}

struct FakeSession {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl EngineSession for FakeSession {
    fn inspect(&mut self) -> PortFuture<'_, Result<EngineInspection, ApplicationError>> {
        self.calls.lock().unwrap().push("inspect");
        Box::pin(async {
            Ok(EngineInspection {
                name: Some("Stub".into()),
                ..EngineInspection::default()
            })
        })
    }

    fn is_ready(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        self.calls.lock().unwrap().push("ready");
        Box::pin(async { Ok(()) })
    }

    fn set_option(
        &mut self,
        _name: &str,
        _value: Option<&str>,
    ) -> PortFuture<'_, Result<(), ApplicationError>> {
        unreachable!()
    }

    fn new_game(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        unreachable!()
    }

    fn search(
        &mut self,
        _request: colosseum_application::SearchRequest,
    ) -> PortFuture<'_, Result<colosseum_application::SearchObservation, ApplicationError>> {
        unreachable!()
    }

    fn stop(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        unreachable!()
    }

    fn shutdown(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        self.calls.lock().unwrap().push("shutdown");
        Box::pin(async { Ok(()) })
    }
}

struct FakeSessions(Arc<Mutex<Vec<&'static str>>>);

impl EngineSessionFactory for FakeSessions {
    fn open(
        &self,
        _participant: &RuntimeParticipant,
    ) -> PortFuture<'_, Result<Box<dyn EngineSession>, ApplicationError>> {
        self.0.lock().unwrap().push("open");
        let calls = Arc::clone(&self.0);
        Box::pin(async move { Ok(Box::new(FakeSession { calls }) as Box<dyn EngineSession>) })
    }
}

#[test]
fn inspection_use_case_is_runtime_independent_and_ordered() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sessions = FakeSessions(Arc::clone(&calls));
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch: colosseum_application::EngineLaunchSpec::path_only("stub".into()),
    };

    let result = block_on(InspectEngine::execute(&sessions, &participant)).unwrap();
    assert_eq!(result.name.as_deref(), Some("Stub"));
    assert_eq!(
        *calls.lock().unwrap(),
        ["open", "inspect", "ready", "shutdown"]
    );
}
