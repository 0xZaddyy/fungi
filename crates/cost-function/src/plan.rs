//! Complete groupings of the queued intents.

use crate::batch::Batch;

/// Groupings of the queued intents, each batch realized by one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan<Id> {
    pub(crate) batches: Vec<Batch<Id>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{InMemoryQueue, Queue};

    #[test]
    fn a_plan_splits_intents_across_its_batches() {
        let first: <InMemoryQueue as Queue>::Id = 0;
        let second = 1;

        let plan = Plan {
            batches: vec![
                Batch {
                    intents: vec![first],
                },
                Batch {
                    intents: vec![second],
                },
            ],
        };

        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.batches[0].intents, [first]);
        assert_eq!(plan.batches[1].intents, [second]);
        assert_eq!(plan.clone(), plan);
    }
}
