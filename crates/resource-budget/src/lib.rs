//! Pure, deterministic resource accounting shared by replay and materialization.
//!
//! The counters are deliberately proxies for work and owned bytes. They are
//! not a claim about resident process memory. A caller may carry one budget
//! across several independent materializations so a history verifier cannot
//! reset a per-job limit for every historical publication.

/// The kind of work or owned data charged to a [`ResourceBudget`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetKind {
    /// Graph ancestry visits performed by the operation log.
    OperationAncestryWork,
    /// Causal witness traversal and ordered witness scans.
    CausalWitnessWork,
    /// Cross-field aggregate conflict checks.
    AggregateConflictChecks,
    /// Canonical operation payload bytes retained by a set.
    OperationBytes,
    /// Estimated bytes copied while applying project effects.
    ProjectCopyBytes,
    /// Estimated bytes retained by structural replay collections.
    WorkingSetBytes,
}

impl BudgetKind {
    /// Stable diagnostic label for persisted/application error boundaries.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OperationAncestryWork => "operation_ancestry_work",
            Self::CausalWitnessWork => "causal_witness_work",
            Self::AggregateConflictChecks => "aggregate_conflict_checks",
            Self::OperationBytes => "operation_bytes",
            Self::ProjectCopyBytes => "project_copy_bytes",
            Self::WorkingSetBytes => "working_set_bytes",
        }
    }
}

/// Limits for one cumulative budget. Values have explicit units: work fields
/// count bounded graph steps, while byte fields count estimated owned bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    operation_ancestry_work: usize,
    causal_witness_work: usize,
    aggregate_conflict_checks: usize,
    operation_bytes: usize,
    project_copy_bytes: usize,
    working_set_bytes: usize,
}

impl ResourceLimits {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        operation_ancestry_work: usize,
        causal_witness_work: usize,
        aggregate_conflict_checks: usize,
        operation_bytes: usize,
        project_copy_bytes: usize,
        working_set_bytes: usize,
    ) -> Self {
        Self {
            operation_ancestry_work,
            causal_witness_work,
            aggregate_conflict_checks,
            operation_bytes,
            project_copy_bytes,
            working_set_bytes,
        }
    }

    pub const fn operation_ancestry_work(self) -> usize {
        self.operation_ancestry_work
    }
    pub const fn causal_witness_work(self) -> usize {
        self.causal_witness_work
    }
    pub const fn aggregate_conflict_checks(self) -> usize {
        self.aggregate_conflict_checks
    }
    pub const fn operation_bytes(self) -> usize {
        self.operation_bytes
    }
    pub const fn project_copy_bytes(self) -> usize {
        self.project_copy_bytes
    }
    pub const fn working_set_bytes(self) -> usize {
        self.working_set_bytes
    }
}

/// Usage accumulated by a budget. It is a value type so callers can record
/// deterministic diagnostics without exposing mutable budget internals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceUsage {
    operation_ancestry_work: usize,
    causal_witness_work: usize,
    aggregate_conflict_checks: usize,
    operation_bytes: usize,
    project_copy_bytes: usize,
    working_set_bytes: usize,
}

impl ResourceUsage {
    pub const fn operation_ancestry_work(self) -> usize {
        self.operation_ancestry_work
    }
    pub const fn causal_witness_work(self) -> usize {
        self.causal_witness_work
    }
    pub const fn aggregate_conflict_checks(self) -> usize {
        self.aggregate_conflict_checks
    }
    pub const fn operation_bytes(self) -> usize {
        self.operation_bytes
    }
    pub const fn project_copy_bytes(self) -> usize {
        self.project_copy_bytes
    }
    pub const fn working_set_bytes(self) -> usize {
        self.working_set_bytes
    }

    const fn get(self, kind: BudgetKind) -> usize {
        match kind {
            BudgetKind::OperationAncestryWork => self.operation_ancestry_work,
            BudgetKind::CausalWitnessWork => self.causal_witness_work,
            BudgetKind::AggregateConflictChecks => self.aggregate_conflict_checks,
            BudgetKind::OperationBytes => self.operation_bytes,
            BudgetKind::ProjectCopyBytes => self.project_copy_bytes,
            BudgetKind::WorkingSetBytes => self.working_set_bytes,
        }
    }

    fn set(&mut self, kind: BudgetKind, value: usize) {
        match kind {
            BudgetKind::OperationAncestryWork => self.operation_ancestry_work = value,
            BudgetKind::CausalWitnessWork => self.causal_witness_work = value,
            BudgetKind::AggregateConflictChecks => self.aggregate_conflict_checks = value,
            BudgetKind::OperationBytes => self.operation_bytes = value,
            BudgetKind::ProjectCopyBytes => self.project_copy_bytes = value,
            BudgetKind::WorkingSetBytes => self.working_set_bytes = value,
        }
    }
}

/// A deterministic resource-limit diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimitExceeded {
    kind: BudgetKind,
    limit: usize,
    used: usize,
    requested: usize,
}

impl ResourceLimitExceeded {
    pub const fn kind(self) -> BudgetKind {
        self.kind
    }
    pub const fn limit(self) -> usize {
        self.limit
    }
    pub const fn used(self) -> usize {
        self.used
    }
    pub const fn requested(self) -> usize {
        self.requested
    }
}

/// Errors returned before work or allocation exceeds a declared boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceBudgetError {
    LimitExceeded(ResourceLimitExceeded),
    Cancelled,
}

/// A cancellation source owned by an outer application, never by storage or
/// an operating-system adapter. It is polled at deterministic budget checks.
pub trait CancellationHook {
    fn is_cancelled(&mut self) -> bool;
}

/// Default hook for non-cancellable pure calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NeverCancel;

impl CancellationHook for NeverCancel {
    fn is_cancelled(&mut self) -> bool {
        false
    }
}

/// Cumulative work and byte accounting. The generic hook keeps cancellation
/// replaceable without importing threads, storage, or platform APIs inward.
#[derive(Debug)]
pub struct ResourceBudget<H = NeverCancel> {
    limits: ResourceLimits,
    usage: ResourceUsage,
    hook: H,
}

impl ResourceBudget<NeverCancel> {
    pub const fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            usage: ResourceUsage {
                operation_ancestry_work: 0,
                causal_witness_work: 0,
                aggregate_conflict_checks: 0,
                operation_bytes: 0,
                project_copy_bytes: 0,
                working_set_bytes: 0,
            },
            hook: NeverCancel,
        }
    }
}

impl<H: CancellationHook> ResourceBudget<H> {
    pub const fn with_cancellation(limits: ResourceLimits, hook: H) -> Self {
        Self {
            limits,
            usage: ResourceUsage {
                operation_ancestry_work: 0,
                causal_witness_work: 0,
                aggregate_conflict_checks: 0,
                operation_bytes: 0,
                project_copy_bytes: 0,
                working_set_bytes: 0,
            },
            hook,
        }
    }

    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }
    pub const fn usage(&self) -> ResourceUsage {
        self.usage
    }

    /// Poll cancellation without consuming quota.
    pub fn check_cancelled(&mut self) -> Result<(), ResourceBudgetError> {
        if self.hook.is_cancelled() {
            Err(ResourceBudgetError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Charge before performing the corresponding work or allocation.
    pub fn charge(&mut self, kind: BudgetKind, amount: usize) -> Result<(), ResourceBudgetError> {
        self.check_cancelled()?;
        self.charge_global(kind, amount)
    }

    /// Charge the shared budget and a per-invocation usage scope atomically.
    /// The scope lets inner packages retain their historical hard limit even
    /// when the caller gives the cumulative budget a larger allowance.
    pub fn charge_with_limit(
        &mut self,
        kind: BudgetKind,
        amount: usize,
        local_usage: &mut ResourceUsage,
        local_limit: usize,
    ) -> Result<(), ResourceBudgetError> {
        self.check_cancelled()?;
        let local_used = local_usage.get(kind);
        let local_next =
            local_used
                .checked_add(amount)
                .ok_or(ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                    kind,
                    limit: local_limit,
                    used: local_used,
                    requested: amount,
                }))?;
        if local_next > local_limit {
            return Err(ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind,
                limit: local_limit,
                used: local_used,
                requested: amount,
            }));
        }
        self.charge_global(kind, amount)?;
        local_usage.set(kind, local_next);
        Ok(())
    }

    fn charge_global(
        &mut self,
        kind: BudgetKind,
        amount: usize,
    ) -> Result<(), ResourceBudgetError> {
        let used = self.usage.get(kind);
        let limit = match kind {
            BudgetKind::OperationAncestryWork => self.limits.operation_ancestry_work,
            BudgetKind::CausalWitnessWork => self.limits.causal_witness_work,
            BudgetKind::AggregateConflictChecks => self.limits.aggregate_conflict_checks,
            BudgetKind::OperationBytes => self.limits.operation_bytes,
            BudgetKind::ProjectCopyBytes => self.limits.project_copy_bytes,
            BudgetKind::WorkingSetBytes => self.limits.working_set_bytes,
        };
        let next = used
            .checked_add(amount)
            .ok_or(ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind,
                limit,
                used,
                requested: amount,
            }))?;
        if next > limit {
            return Err(ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind,
                limit,
                used,
                requested: amount,
            }));
        }
        match kind {
            BudgetKind::OperationAncestryWork => self.usage.operation_ancestry_work = next,
            BudgetKind::CausalWitnessWork => self.usage.causal_witness_work = next,
            BudgetKind::AggregateConflictChecks => self.usage.aggregate_conflict_checks = next,
            BudgetKind::OperationBytes => self.usage.operation_bytes = next,
            BudgetKind::ProjectCopyBytes => self.usage.project_copy_bytes = next,
            BudgetKind::WorkingSetBytes => self.usage.working_set_bytes = next,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct CancelAfter {
        remaining_checks: usize,
    }

    impl CancellationHook for CancelAfter {
        fn is_cancelled(&mut self) -> bool {
            if self.remaining_checks == 0 {
                true
            } else {
                self.remaining_checks -= 1;
                false
            }
        }
    }

    fn limits() -> ResourceLimits {
        ResourceLimits::new(10, 10, 10, 10, 10, 10)
    }

    #[test]
    fn charges_accumulate_by_kind_and_failed_charge_is_atomic() {
        let mut budget = ResourceBudget::new(limits());
        budget.charge(BudgetKind::OperationBytes, 6).unwrap();
        budget.charge(BudgetKind::OperationBytes, 4).unwrap();
        assert_eq!(budget.usage().operation_bytes(), 10);

        let error = budget.charge(BudgetKind::OperationBytes, 1).unwrap_err();
        assert_eq!(
            error,
            ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind: BudgetKind::OperationBytes,
                limit: 10,
                used: 10,
                requested: 1,
            })
        );
        assert_eq!(budget.usage().operation_bytes(), 10);
    }

    #[test]
    fn zero_charge_still_observes_cancellation() {
        let mut budget = ResourceBudget::with_cancellation(
            limits(),
            CancelAfter {
                remaining_checks: 0,
            },
        );
        assert_eq!(
            budget.charge(BudgetKind::WorkingSetBytes, 0),
            Err(ResourceBudgetError::Cancelled)
        );
        assert_eq!(budget.usage(), ResourceUsage::default());
    }

    #[test]
    fn checked_addition_rejects_overflow_without_mutating_usage() {
        let mut budget = ResourceBudget::new(ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ));
        budget
            .charge(BudgetKind::OperationBytes, usize::MAX)
            .unwrap();
        let error = budget.charge(BudgetKind::OperationBytes, 1).unwrap_err();
        assert_eq!(
            error,
            ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind: BudgetKind::OperationBytes,
                limit: usize::MAX,
                used: usize::MAX,
                requested: 1,
            })
        );
        assert_eq!(budget.usage().operation_bytes(), usize::MAX);
    }

    #[test]
    fn cancellation_hook_is_polled_between_successful_charges() {
        let mut budget = ResourceBudget::with_cancellation(
            limits(),
            CancelAfter {
                remaining_checks: 1,
            },
        );
        budget.charge(BudgetKind::CausalWitnessWork, 1).unwrap();
        assert_eq!(
            budget.charge(BudgetKind::CausalWitnessWork, 1),
            Err(ResourceBudgetError::Cancelled)
        );
        assert_eq!(budget.usage().causal_witness_work(), 1);
    }

    #[test]
    fn local_limit_failure_is_atomic_for_shared_and_local_usage() {
        let mut budget = ResourceBudget::new(limits());
        let mut local = ResourceUsage::default();
        budget
            .charge_with_limit(BudgetKind::WorkingSetBytes, 3, &mut local, 3)
            .unwrap();
        let error = budget
            .charge_with_limit(BudgetKind::WorkingSetBytes, 1, &mut local, 3)
            .unwrap_err();
        assert_eq!(
            error,
            ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind: BudgetKind::WorkingSetBytes,
                limit: 3,
                used: 3,
                requested: 1,
            })
        );
        assert_eq!(local.working_set_bytes(), 3);
        assert_eq!(budget.usage().working_set_bytes(), 3);
    }

    #[test]
    fn shared_limit_failure_is_atomic_after_local_preflight() {
        let mut budget = ResourceBudget::new(ResourceLimits::new(10, 10, 10, 10, 10, 1));
        let mut local = ResourceUsage::default();
        let error = budget
            .charge_with_limit(BudgetKind::WorkingSetBytes, 2, &mut local, 10)
            .unwrap_err();
        assert_eq!(
            error,
            ResourceBudgetError::LimitExceeded(ResourceLimitExceeded {
                kind: BudgetKind::WorkingSetBytes,
                limit: 1,
                used: 0,
                requested: 2,
            })
        );
        assert_eq!(local, ResourceUsage::default());
        assert_eq!(budget.usage(), ResourceUsage::default());
    }
}
