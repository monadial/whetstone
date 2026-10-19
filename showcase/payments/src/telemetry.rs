//! Payment metrics, exported in the Prometheus text format.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::payment::PaymentStatus;

#[derive(Debug, Default)]
pub struct Metrics {
    payments_created: AtomicU64,
    succeeded: AtomicU64,
    declined: AtomicU64,
    unknown: AtomicU64,
}

impl Metrics {
    pub fn payment_created(&self) {
        self.payments_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a finished charge by the status it left the payment in.
    pub fn charge_finished(&self, status: &PaymentStatus) {
        let counter = match status {
            PaymentStatus::Succeeded { .. } => &self.succeeded,
            PaymentStatus::Declined { .. } => &self.declined,
            PaymentStatus::Pending | PaymentStatus::Unknown => &self.unknown,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn render(&self) -> String {
        let count = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        format!(
            "# TYPE payments_created_total counter\n\
             payments_created_total {}\n\
             # TYPE payments_charges_total counter\n\
             payments_charges_total{{outcome=\"succeeded\"}} {}\n\
             payments_charges_total{{outcome=\"declined\"}} {}\n\
             payments_charges_total{{outcome=\"unknown\"}} {}\n",
            count(&self.payments_created),
            count(&self.succeeded),
            count(&self.declined),
            count(&self.unknown),
        )
    }
}
