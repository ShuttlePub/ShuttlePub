//! Starting and stopping the Accept delivery, the reaction that answers each
//! recorded federation (`application::delivery::AcceptDelivery`).

use std::sync::Arc;

use application::{
    delivery::{AcceptDelivery, accept_delivery_projection},
    eventsource::DependOnEventSourceSystem,
};
use error_stack::{Report, ResultExt};
use kernel::producer::ProducerEvent;
use nitinol::runtime::process::AnyProxy;

use crate::{app::AppModule, database::PoolTransactions, error::ServerError};

/// The running Accept delivery.
pub struct AcceptDeliveryHandle(AnyProxy);

impl AcceptDeliveryHandle {
    /// Asks the delivery to stop.
    pub async fn stop(self) -> Result<(), Report<ServerError>> {
        self.0.stop().await.change_context(ServerError::Reaction)
    }
}

/// Starts the Accept delivery, which saves its position in a transaction
/// `transactions` opens. It resumes after the last position saved and follows
/// the whole journal.
pub async fn spawn_accept_delivery(
    app: &AppModule,
    transactions: PoolTransactions,
) -> AcceptDeliveryHandle {
    let handler = Arc::clone(&app.0);
    // `with_tx_provider` drops the event handlers registered before it, so it
    // has to come before `with_event`.
    let proxy = app
        .event_source_system()
        .projector_props(accept_delivery_projection(), move || {
            AcceptDelivery::new(Arc::clone(&handler))
        })
        .with_tx_provider(transactions)
        .with_event::<ProducerEvent>()
        .catchup_from_global()
        .spawn()
        .await;
    AcceptDeliveryHandle(AnyProxy::from(proxy))
}
