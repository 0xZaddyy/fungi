//! Complete groupings of the queued intents.

use std::collections::BTreeSet;

use crate::batch::Batch;

/// Groupings of the queued intents, each batch realized by one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanIntent<Id> {
    pub(crate) batches: BTreeSet<Batch<Id>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{InMemoryQueue, Queue};

    #[test]
    fn a_plan_splits_intents_across_its_batches() {
        let first: <InMemoryQueue as Queue>::Id = 0;
        let second = 1;

        let plan = PlanIntent {
            batches: BTreeSet::from([
                Batch {
                    intents: BTreeSet::from([first]),
                },
                Batch {
                    intents: BTreeSet::from([second]),
                },
            ]),
        };

        assert_eq!(
            plan.batches,
            BTreeSet::from([
                Batch {
                    intents: BTreeSet::from([second]),
                },
                Batch {
                    intents: BTreeSet::from([first]),
                },
            ])
        );
        assert_eq!(plan.clone(), plan);
    }
}
